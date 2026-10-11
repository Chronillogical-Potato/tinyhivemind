// Compare view: several math packages over the same seeds, streamed from
// `hive_view --compare`. Bars for accuracy, convergence and mean rounds, and
// a per-seed strip (correct / wrong / no decision) whose cells open that run.
import { clip, h, s, svgRoot } from "./dom.js";

const BASELINE = "single-seat";

/** Per-package `{package, seats, accuracy, converged, mean_rounds, mean_turns}` from seed events. */
export function summarize(seeds) {
  const by = new Map();
  for (const ev of seeds) for (const r of ev.results ?? []) {
    if (!by.has(r.package)) by.set(r.package, { package: r.package, seats: r.seats, n: 0, right: 0, conv: 0, rounds: 0, turns: 0 });
    const a = by.get(r.package);
    a.n++;
    a.right += r.correct ? 1 : 0;
    a.conv += r.ending === "converged" ? 1 : 0;
    a.rounds += r.rounds ?? 0;
    a.turns += r.turns ?? 0;
  }
  return [...by.values()].map((a) => ({
    package: a.package, seats: a.seats, accuracy: a.right / a.n, converged: a.conv / a.n, mean_rounds: a.rounds / a.n, mean_turns: a.turns / a.n,
  }));
}

/** Outcome class of one result: right, wrong, or none (no decision). */
export const outcome = (r) => (r.ending !== "converged" || r.correct == null ? "none" : r.correct ? "right" : "wrong");

/** Packages in display order: the baseline first, then as requested. */
export function order(rows, requested) {
  const rank = (p) => (p === BASELINE ? -1 : requested.indexOf(p) >= 0 ? requested.indexOf(p) : 99);
  return [...rows].sort((a, b) => rank(a.package) - rank(b.package));
}

function bars(title, rows, key, max, fmt) {
  const width = 300, LW = 96, row = 22;
  const svg = svgRoot(width, rows.length * row + 4, `${title} by package`);
  rows.forEach((r, i) => {
    const y = i * row + 2;
    const w = max ? ((width - LW - 48) * (r[key] ?? 0)) / max : 0;
    svg.append(s("text", { x: 0, y: y + 13 }, clip(r.package, 14)));
    svg.append(s("rect", { x: LW, y: y + 2, width: Math.max(1, w), height: 14, rx: 2, class: `hv-cbar${r.package === BASELINE ? " base" : ""}` }));
    svg.append(s("text", { x: LW + w + 5, y: y + 13, class: "dim" }, fmt(r[key] ?? 0)));
  });
  return h("figure", { class: "hv-fig" }, h("figcaption", {}, title), svg);
}

function strip(host, seeds, packages, onOpen, openable) {
  const LW = 96;
  const width = Math.max(320, Math.floor(host.clientWidth || 640));
  const cell = Math.max(4, Math.min(16, Math.floor((width - LW) / Math.max(1, seeds.length))));
  const svg = svgRoot(LW + cell * seeds.length + 2, packages.length * (cell + 6) + 4, "Per-seed outcomes");
  packages.forEach((p, row) => {
    const y = row * (cell + 6) + 2;
    svg.append(s("text", { x: 0, y: y + cell / 2 + 4 }, clip(p, 14)));
    seeds.forEach((ev, col) => {
      const r = (ev.results ?? []).find((x) => x.package === p);
      if (!r) return;
      const can = openable.has(p);
      const rect = s("rect", { x: LW + col * cell, y, width: cell - 1, height: cell, rx: 1, class: `hv-cell ${outcome(r)}${can ? " open" : ""}`, tabindex: can ? 0 : null, role: can ? "button" : null });
      rect.append(s("title", {}, `${p} · seed ${ev.seed} · ${ev.problem ?? ""} · ${r.ending}${r.correct == null ? "" : r.correct ? " · correct" : " · wrong"} · ${r.rounds} rounds`));
      if (can) {
        const go = () => onOpen(p, ev.seed, ev.problem);
        rect.addEventListener("click", go);
        rect.addEventListener("keydown", (e) => { if (e.key === "Enter") go(); });
      }
      svg.append(rect);
    });
  });
  return svg;
}

/** Draw the compare results so far into `host`. */
export function renderCompare(host, { seeds, summary, requested, done }, onOpen, openable) {
  if (!seeds.length) {
    host.replaceChildren(h("p", { class: "muted" }, done ? "No results." : "Waiting for the first seed…"));
    return;
  }
  const rows = order(summary ?? summarize(seeds), requested);
  const maxRounds = Math.max(1, ...rows.map((r) => r.mean_rounds));
  const pct = (v) => `${Math.round(v * 100)}%`;
  const names = rows.map((r) => r.package);
  host.replaceChildren(
    h("p", { class: "muted" }, `${seeds.length} seeds${summary ? "" : " so far (streaming…)"}. Single-seat is the baseline: one seat answering alone.`),
    h("div", { class: "hv-figs" },
      bars("Accuracy", rows, "accuracy", 1, pct),
      bars("Converged", rows, "converged", 1, pct),
      bars("Mean rounds", rows, "mean_rounds", maxRounds, (v) => v.toFixed(1))),
    h("h3", {}, "Per seed"),
    h("div", { class: "scroll" }, strip(host, seeds, names, onOpen, openable)),
    h("div", { class: "legend" },
      h("span", {}, h("i", { class: "sw hv-cell right" }), "correct"),
      h("span", {}, h("i", { class: "sw hv-cell wrong" }), "wrong"),
      h("span", {}, h("i", { class: "sw hv-cell none" }), "no decision"),
      h("span", { class: "muted" }, "Click a cell to replay that seed.")));
}
