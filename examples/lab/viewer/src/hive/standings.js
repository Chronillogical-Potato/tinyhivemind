// Quorum standings per topic: a support bar and an object bar, a dashed
// threshold line at the episode's quorum, and a "committed" mark once the
// room records the decision. Standings carry over from the last round that
// reported any (see simulation.js).
import { clip, h, s, svgRoot } from "./dom.js";

const MAX_TOPICS = 8;
const LABEL_W = 96, ROW = 30, RIGHT = 84;

/** Draw `standings` with `threshold` into `host`; `answer` marks the right topic for math. */
export function renderStandings(host, standings, threshold, answer) {
  const rows = [...standings].sort((a, b) => b.support - a.support || a.topic.localeCompare(b.topic)).slice(0, MAX_TOPICS);
  if (!rows.length) {
    host.replaceChildren(h("p", { class: "muted" }, "No proposals yet."));
    return;
  }
  const width = Math.max(240, Math.floor(host.clientWidth || 300));
  const span = Math.max(threshold || 1, ...rows.map((r) => Math.max(r.support, r.object))) + 1;
  const plot = width - LABEL_W - RIGHT;
  const x = (v) => LABEL_W + (v / span) * plot;
  const height = rows.length * ROW + 18;
  const svg = svgRoot(width, height, `Quorum standings for ${rows.length} topics; threshold ${threshold}`);
  rows.forEach((r, i) => {
    const y = i * ROW + 4;
    const tone = answer == null ? "" : r.topic === answer ? " right" : " wrong";
    svg.append(s("text", { x: 0, y: y + 13, class: `hv-topic${tone}` }, clip(r.topic, 13)));
    svg.append(s("rect", { x: LABEL_W, y, width: Math.max(0, x(r.support) - LABEL_W), height: 12, rx: 2, class: "hv-support" }));
    svg.append(s("rect", { x: LABEL_W, y: y + 14, width: Math.max(0, x(r.object) - LABEL_W), height: 6, rx: 2, class: "hv-object" }));
    const note = `${r.support} for · ${r.object} against${r.committed ? " · committed" : ""}`;
    svg.append(s("text", { x: x(Math.max(r.support, r.object)) + 6, y: y + 11, class: r.committed ? "hv-committed" : "dim" }, r.committed ? "committed" : `${r.support}/${r.object}`));
    svg.append(s("title", {}, `${r.topic}: ${note}`));
  });
  if (threshold) {
    svg.append(s("line", { x1: x(threshold), x2: x(threshold), y1: 0, y2: height - 14, class: "hv-threshold" }));
    svg.append(s("text", { x: x(threshold), y: height - 2, "text-anchor": "middle", class: "dim" }, `quorum ${threshold}`));
  }
  const more = standings.length > rows.length ? h("p", { class: "muted" }, `${standings.length - rows.length} more topics`) : null;
  host.replaceChildren(svg, h("div", { class: "legend" },
    h("span", {}, h("i", { class: "sw", style: "background:var(--good)" }), "support"),
    h("span", {}, h("i", { class: "sw", style: "background:var(--bad)" }), "object")), more);
}
