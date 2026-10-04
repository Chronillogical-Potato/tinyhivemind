import { initPanel } from "./panel.js";
"use strict";
// Hive Lab run viewer. Runs load from the dev server (see server/runs.js), or from dropped files.
// Input is flat JSONL stamped events; see crates/tinyhivemind-core/src/telemetry/types.rs.

const SVGNS = "http://www.w3.org/2000/svg";
const $ = (id) => document.getElementById(id);
const runs = []; // {name, events, a}  a = analysis
const GUTTER = 84, PADR = 14, MIN_W = 520;

// ---------- tiny DOM helpers ----------
function h(tag, attrs, ...kids) {
  const n = document.createElement(tag);
  for (const k in attrs || {}) {
    if (k === "class") n.className = attrs[k]; else n.setAttribute(k, attrs[k]);
  }
  for (const c of kids) if (c != null) n.append(c);
  return n;
}
function s(tag, attrs, text) {
  const n = document.createElementNS(SVGNS, tag);
  for (const k in attrs || {}) n.setAttribute(k, attrs[k]);
  if (text != null) n.textContent = text;
  return n;
}
const fmt = (n) => (n == null || Number.isNaN(n) ? "-" : Math.round(n).toLocaleString("en-US"));
const seatColor = (a, seat) => `var(--s${a.seats.indexOf(seat) % 8})`;

// ---------- tooltip ----------
const tip = $("tip");
function showTip(ev, title, raw) {
  tip.replaceChildren(h("b", {}, title), h("pre", {}, typeof raw === "string" ? raw : JSON.stringify(raw, null, 1)));
  tip.style.display = "block";
  const r = ev.currentTarget.getBoundingClientRect();
  const x = ev.type === "pointerenter" || ev.type === "pointermove" ? ev.clientX : r.left + r.width / 2;
  const y = ev.type === "pointerenter" || ev.type === "pointermove" ? ev.clientY : r.bottom;
  const w = tip.offsetWidth, hh = tip.offsetHeight;
  let left = Math.min(Math.max(8, x + 12), window.innerWidth - w - 8);
  let top = y + 14;
  if (top + hh > window.innerHeight - 8) top = Math.max(8, y - hh - 14);
  tip.style.left = left + "px"; tip.style.top = top + "px";
}
const hideTip = () => { tip.style.display = "none"; };
function hover(node, title, raw, label) {
  node.classList.add("hit");
  node.setAttribute("tabindex", "0");
  node.setAttribute("role", "img");
  node.setAttribute("aria-label", label || title);
  node.addEventListener("pointerenter", (e) => showTip(e, title, raw));
  node.addEventListener("pointermove", (e) => showTip(e, title, raw));
  node.addEventListener("focus", (e) => showTip(e, title, raw));
  node.addEventListener("pointerleave", hideTip);
  node.addEventListener("blur", hideTip);
  return node;
}
document.addEventListener("keydown", (e) => { if (e.key === "Escape") hideTip(); });

// ---------- parsing ----------
function parseJsonl(text, fileName) {
  const byRun = new Map();
  let bad = 0;
  text.split(/\r?\n/).forEach((line, i) => {
    line = line.trim();
    if (!line) return;
    let o;
    try { o = JSON.parse(line); } catch { bad++; return; }
    if (!o || typeof o !== "object" || typeof o.event !== "string" || typeof o.at_ms !== "number") { bad++; return; }
    if (typeof o.seq !== "number") o.seq = i;
    const key = typeof o.run === "string" && o.run ? o.run : fileName;
    if (!byRun.has(key)) byRun.set(key, []);
    byRun.get(key).push(o);
  });
  return { byRun, bad };
}

function addRuns(parsed, fileName, label, source) {
  let added = 0;
  for (const [key, events] of parsed.byRun) {
    let name = label && parsed.byRun.size === 1 ? label : key;
    const key0 = name;
    if (runs.some((r) => r.name === name)) name = `${key0} (${fileName})`;
    for (let n = 2; runs.some((r) => r.name === name); n++) name = `${key0} (${fileName} #${n})`;
    events.sort((x, y) => x.seq - y.seq);
    runs.push({ name, events, a: analyze(events), source });
    added++;
  }
  return added;
}

// ---------- analysis ----------
function percentile(sorted, p) {
  if (!sorted.length) return null;
  return sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1))];
}

function analyze(events) {
  const seatSet = [];
  const note = (x) => { if (x && !seatSet.includes(x)) seatSet.push(x); };
  const byId = new Map(), open = new Map(); // seat -> queue of started turns, matched by order
  const turns = [], tools = [], rounds = [], conducted = [], outcomes = [], marks = [], others = [];
  let tokIn = 0, tokOut = 0;
  for (const e of events) {
    switch (e.event) {
      case "turn_started": {
        note(e.seat);
        const t = { seat: e.seat, id: e.turn, start: e.at_ms, end: null, open: true, started: e, finished: null };
        turns.push(t);
        if (typeof e.turn === "number") byId.set(e.turn, t);
        if (!open.has(e.seat)) open.set(e.seat, []);
        open.get(e.seat).push(t);
        break;
      }
      case "turn_finished": {
        note(e.seat);
        const q = open.get(e.seat);
        let t = typeof e.turn === "number" ? byId.get(e.turn) : q && q.shift(); // turn id wins; seat order is the fallback
        if (t && q && q.includes(t)) q.splice(q.indexOf(t), 1);
        if (!t) { // finish with no start: reconstruct from latency
          t = { seat: e.seat, id: e.turn, start: e.at_ms - (e.latency_ms || 0), started: null };
          turns.push(t);
        }
        t.end = e.at_ms; t.open = false; t.finished = e;
        t.input = e.input_tokens || 0; t.output = e.output_tokens || 0;
        t.latency = typeof e.latency_ms === "number" ? e.latency_ms : t.end - t.start;
        tokIn += t.input; tokOut += t.output;
        break;
      }
      case "tool_call": note(e.seat); tools.push(e); break;
      case "round": (e.seats || []).forEach((x) => note(x.agent_id)); rounds.push(e); break;
      case "conducted": conducted.push(e); note(e.conducted && e.conducted.seat); break;
      case "converged": case "deadlocked": case "exhausted": case "idle": outcomes.push(e); break;
      case "checkpoint": case "mark": marks.push(e); break;
      default: others.push(e);
    }
  }
  let t0 = Infinity, t1 = -Infinity;
  for (const e of events) { t0 = Math.min(t0, e.at_ms); t1 = Math.max(t1, e.at_ms); }
  for (const t of turns) { t0 = Math.min(t0, t.start); }
  if (!events.length) { t0 = 0; t1 = 0; }
  for (const t of turns) if (t.end == null) t.end = t1; // unfinished turns run to the end
  for (const t of tools) { t.span0 = t.at_ms - (t.latency_ms || 0); t0 = Math.min(t0, t.span0); }
  const finished = turns.filter((t) => !t.open);
  const lat = finished.map((t) => t.latency).sort((x, y) => x - y);
  const edges = [];
  for (const t of turns) { edges.push([t.start, 1], [t.end, -1]); }
  edges.sort((x, y) => x[0] - y[0] || x[1] - y[1]); // ends before starts at a tie
  let cur = 0, maxConc = 0;
  for (const [, d] of edges) { cur += d; maxConc = Math.max(maxConc, cur); }
  const perSeat = {};
  for (const seat of seatSet) perSeat[seat] = { input: 0, output: 0, turns: 0 };
  for (const t of finished) { const p = perSeat[t.seat]; p.input += t.input; p.output += t.output; p.turns++; }
  return {
    seats: seatSet, turns, tools, rounds, conducted, outcomes, marks, others, perSeat, t0, t1,
    summary: {
      turns: turns.length, tokIn, tokOut, tokTotal: tokIn + tokOut, wall: t1 - t0,
      p50: percentile(lat, 50), p95: percentile(lat, 95), maxConc,
      tools: tools.length, refused: tools.filter((t) => t.refused).length,
      rounds: rounds.length, conducted: conducted.length,
      outcome: outcomes.map((o) => o.event).filter((x) => x !== "idle").join(", ") || "none",
    },
  };
}

// ---------- scales ----------
function niceStep(span, target) {
  const raw = span / target || 1, pow = Math.pow(10, Math.floor(Math.log10(raw)));
  const f = raw / pow;
  return (f < 1.5 ? 1 : f < 3.5 ? 2 : f < 7.5 ? 5 : 10) * pow;
}
function makeScale(span, width) {
  const inner = width - GUTTER - PADR;
  return (ms) => GUTTER + (Math.max(0, ms) / (span || 1)) * inner;
}
function axis(svg, x, span, top, height, gridBottom) {
  const g = s("g", { class: "axis" });
  const step = niceStep(span, Math.max(3, Math.floor((svg.viewBox.baseVal.width - GUTTER) / 90)));
  for (let v = 0; v <= span + 1e-6; v += step) {
    g.append(s("line", { x1: x(v), x2: x(v), y1: top, y2: gridBottom, class: "grid" }));
    g.append(s("text", { x: x(v), y: height - 4, "text-anchor": "middle" }, v >= 1000 ? `${+(v / 1000).toFixed(2)}s` : `${Math.round(v)}ms`));
  }
  svg.append(g);
}
function newSvg(width, height, label) {
  const svg = s("svg", { viewBox: `0 0 ${width} ${height}`, width, height, role: "group", "aria-label": label });
  return svg;
}
const chartWidth = (host) => Math.max(MIN_W, Math.floor(host.clientWidth || MIN_W));

// ---------- waterfall ----------
function layoutRows(items) { // greedy sub-rows so overlapping turns of one seat do not collide
  const ends = [];
  for (const t of items) {
    let r = ends.findIndex((e) => e <= t.start);
    if (r < 0) { r = ends.length; ends.push(0); }
    ends[r] = t.end; t.row = r;
  }
  return Math.max(1, ends.length);
}

function waterfall(run, span, host) {
  const a = run.a, W = chartWidth(host), x = makeScale(span, W);
  const lanes = a.seats.map((seat) => {
    const mine = a.turns.filter((t) => t.seat === seat).sort((p, q) => p.start - q.start);
    return { seat, mine, rows: layoutRows(mine) };
  });
  const ROW = 20, TOP = 6;
  let y = TOP; lanes.forEach((l) => { l.y = y; y += l.rows * ROW + 8; });
  const H = y + 22, svg = newSvg(W, H, `Per-seat waterfall for ${run.name}`);
  axis(svg, x, span, 0, H, y);
  for (const l of lanes) {
    svg.append(s("text", { x: 6, y: l.y + 13, class: "lane" }, l.seat.length > 11 ? l.seat.slice(0, 10) + "…" : l.seat));
    svg.append(s("line", { x1: GUTTER, x2: W - PADR, y1: l.y + l.rows * ROW + 4, y2: l.y + l.rows * ROW + 4, class: "grid" }));
    for (const t of l.mine) {
      const x0 = x(t.start - a.t0), x1 = x(t.end - a.t0), by = l.y + t.row * ROW;
      const bar = s("rect", { x: x0, y: by, width: Math.max(2, x1 - x0), height: 14, rx: 3, fill: seatColor(a, l.seat),
        "fill-opacity": t.open ? 0.35 : 0.85, stroke: seatColor(a, l.seat), "stroke-dasharray": t.open ? "4 2" : "none" });
      const info = { seat: t.seat, start_ms: t.start - a.t0, end_ms: t.end - a.t0, started: t.started, finished: t.finished };
      svg.append(hover(bar, `${t.seat} turn${t.open ? " (never finished)" : ""}`, info,
        `${t.seat} turn from ${t.start - a.t0} to ${t.end - a.t0} milliseconds${t.open ? ", unfinished" : ""}`));
    }
    for (const c of a.tools.filter((c) => c.seat === l.seat)) {
      const host_t = (typeof c.turn === "number" && l.mine.find((t) => t.id === c.turn))
        || l.mine.find((t) => c.at_ms >= t.start && c.at_ms <= t.end) || l.mine[0];
      const row = host_t ? host_t.row : 0, by = l.y + row * ROW;
      const x0 = x(c.span0 - a.t0), x1 = x(c.at_ms - a.t0), w = Math.max(3, x1 - x0);
      const g = s("g", {});
      g.append(s("rect", { x: x0, y: by - 3, width: w, height: 20, fill: c.refused ? "var(--bad)" : "var(--tick)",
        "fill-opacity": c.refused ? 1 : 0.8, stroke: "var(--panel)", "stroke-width": 1 }));
      if (c.refused) g.append(s("text", { x: x0 + w / 2, y: by - 4, "text-anchor": "middle", fill: "var(--bad)", style: "fill:var(--bad);font-weight:700;font-size:15px" }, "×"));
      svg.append(hover(g, `${c.tool}${c.refused ? " (refused" + (c.reason ? ": " + c.reason : "") + ")" : ""}`, c, `${c.seat} called ${c.tool}${c.refused ? ", refused" + (c.reason ? " because " + c.reason : "") : ""}`));
    }
  }
  host.replaceChildren(svg);
}

// ---------- timeline ----------
const KIND_COLOR = ["--s0", "--s1", "--s2", "--s3", "--s4", "--s5", "--s6", "--s7"];
const OUTCOME_GLYPH = { converged: "C", deadlocked: "D", exhausted: "E", idle: "I" };
const OUTCOME_FILL = { converged: "--good", deadlocked: "--bad", exhausted: "--bad", idle: "--muted" };

function marker(shape, cx, cy, r, fill) {
  const p = { fill, stroke: "var(--panel)", "stroke-width": 1.5 };
  if (shape === "diamond") return s("path", { ...p, d: `M${cx} ${cy - r}L${cx + r} ${cy}L${cx} ${cy + r}L${cx - r} ${cy}Z` });
  if (shape === "square") return s("rect", { ...p, x: cx - r + 1, y: cy - r + 1, width: 2 * r - 2, height: 2 * r - 2 });
  if (shape === "triangle") return s("path", { ...p, d: `M${cx} ${cy - r}L${cx + r} ${cy + r - 1}L${cx - r} ${cy + r - 1}Z` });
  return s("circle", { ...p, cx, cy, r });
}
function detail(e) { const { run, seq, at_ms, event, ...rest } = e; return rest; }

function timeline(run, span, host) {
  const a = run.a, W = chartWidth(host), x = makeScale(span, W);
  const kinds = [...new Set(a.conducted.map((e) => (e.conducted && e.conducted.kind) || "unknown"))];
  const rows = [
    { key: "rounds", label: "rounds", h: 26 },
    ...kinds.map((k) => ({ key: "c:" + k, label: k, h: 18 })),
    { key: "outcome", label: "outcome", h: 22 },
    { key: "marks", label: "marks", h: 22 },
  ];
  let y = 4; for (const r of rows) { r.y = y; y += r.h + 4; }
  const H = y + 22, svg = newSvg(W, H, `Room timeline for ${run.name}`);
  axis(svg, x, span, 0, H, y);
  for (const r of rows) svg.append(s("text", { x: 6, y: r.y + r.h / 2 + 4, class: "dim" }, r.label.length > 11 ? r.label.slice(0, 10) + "…" : r.label));
  const R = rows[0];
  a.rounds.forEach((e, i) => {
    const t0 = e.at_ms - a.t0, t1 = (a.rounds[i + 1] ? a.rounds[i + 1].at_ms : a.t1) - a.t0;
    const x0 = x(t0), w = Math.max(3, x(t1) - x0 - 1), full = e.visibility === "full";
    const g = s("g", {});
    g.append(s("rect", { x: x0, y: R.y, width: w, height: R.h, rx: 3, fill: full ? "var(--full)" : "var(--blind)", stroke: "var(--accent)",
      "stroke-dasharray": full ? "none" : "3 2" }));
    const label = `R${i + 1} ${e.phase}/${e.visibility}`;
    if (w > label.length * 6 + 6) g.append(s("text", { x: x0 + 4, y: R.y + 16 }, label));
    else if (w > 22) g.append(s("text", { x: x0 + 4, y: R.y + 16 }, `R${i + 1}`));
    svg.append(hover(g, `Round ${i + 1}: ${e.phase}, ${e.visibility}`, detail(e), `Round ${i + 1}, ${e.phase} phase, ${e.visibility} visibility, ${(e.seats || []).length} seats`));
  });
  for (const e of a.conducted) {
    const k = (e.conducted && e.conducted.kind) || "unknown", r = rows.find((q) => q.key === "c:" + k);
    const ci = kinds.indexOf(k) % 8, cx = x(e.at_ms - a.t0), cy = r.y + r.h / 2;
    svg.append(hover(marker("diamond", cx, cy, 6, `var(${KIND_COLOR[ci]})`), `conducted: ${k}`, detail(e), `Conductor ${k} at ${e.at_ms - a.t0} milliseconds`));
  }
  const O = rows.find((q) => q.key === "outcome"), M = rows.find((q) => q.key === "marks");
  for (const e of a.outcomes) {
    const cx = x(e.at_ms - a.t0), cy = O.y + O.h / 2, g = s("g", {});
    g.append(marker("circle", cx, cy, 9, `var(${OUTCOME_FILL[e.event]})`));
    g.append(s("text", { x: cx, y: cy + 4, "text-anchor": "middle", style: "fill:#fff;font-weight:700;font-size:10px" }, OUTCOME_GLYPH[e.event]));
    svg.append(hover(g, e.event, detail(e), `Room ${e.event} at ${e.at_ms - a.t0} milliseconds`));
  }
  for (const e of a.marks) {
    const cx = x(e.at_ms - a.t0), cy = M.y + M.h / 2, isMark = e.event === "mark";
    svg.append(hover(marker(isMark ? "triangle" : "square", cx, cy, 7, isMark ? "var(--s1)" : "var(--accent)"),
      `${e.event}: ${e.label}`, detail(e), `${e.event} ${e.label}`));
  }
  host.replaceChildren(svg);
  const lg = h("div", { class: "legend" });
  const item = (swatch, text) => lg.append(h("span", {}, swatch, text));
  item(h("i", { class: "sw", style: "background:var(--blind);border:1px dashed var(--accent)" }), "blind round");
  item(h("i", { class: "sw", style: "background:var(--full);border:1px solid var(--accent)" }), "full-visibility round");
  kinds.forEach((k, i) => item(h("i", { class: "sw", style: `background:var(${KIND_COLOR[i % 8]});transform:rotate(45deg) scale(.8)` }), k));
  item(document.createTextNode(""), "C converged, D deadlocked, E exhausted, I idle; triangle mark, square checkpoint");
  host.append(lg);
}

// ---------- tokens ----------
function tokenCharts(run, span, host, shareHost) {
  const a = run.a, W = chartWidth(host), x = makeScale(span, W);
  const fin = a.turns.filter((t) => !t.open).sort((p, q) => p.end - q.end);
  let ci = 0, co = 0;
  const pts = [[0, 0, 0]];
  for (const t of fin) { ci += t.input; co += t.output; pts.push([t.end - a.t0, ci, co]); }
  const max = Math.max(1, ci + co), H = 190, top = 8, bottom = H - 22;
  const y = (v) => bottom - (v / max) * (bottom - top);
  const svg = newSvg(W, H, `Cumulative tokens for ${run.name}`);
  axis(svg, x, span, 0, H, bottom);
  for (const frac of [0.5, 1]) {
    svg.append(s("line", { x1: GUTTER, x2: W - PADR, y1: y(max * frac), y2: y(max * frac), class: "grid" }));
    svg.append(s("text", { x: GUTTER - 6, y: y(max * frac) + 4, "text-anchor": "end", class: "dim" }, fmt(max * frac)));
  }
  const line = (pick, color, dash) => {
    let d = `M${x(0)} ${y(0)}`, prev = 0;
    for (const p of pts.slice(1)) { d += `H${x(p[0])}V${y(pick(p))}`; prev = pick(p); }
    d += `H${x(span)}`;
    svg.append(s("path", { d, fill: "none", stroke: color, "stroke-width": 2, "stroke-dasharray": dash || "none" }));
  };
  line((p) => p[1] + p[2], "var(--ink)");
  line((p) => p[1], "var(--s0)", "5 3");
  line((p) => p[2], "var(--s1)", "2 3");
  for (const t of fin) {
    const cum = pts.find((p) => p[0] === t.end - a.t0) || pts[pts.length - 1];
    svg.append(hover(s("circle", { cx: x(t.end - a.t0), cy: y(cum[1] + cum[2]), r: 4, fill: seatColor(a, t.seat), stroke: "var(--panel)" }),
      `${t.seat}: +${fmt(t.input + t.output)} tokens`, { seat: t.seat, input_tokens: t.input, output_tokens: t.output, cumulative: cum[1] + cum[2] },
      `${t.seat} turn added ${t.input + t.output} tokens, ${cum[1] + cum[2]} cumulative`));
  }
  host.replaceChildren(svg);
  const lg = h("div", { class: "legend" });
  [["var(--ink)", "total"], ["var(--s0)", "input"], ["var(--s1)", "output"]].forEach(([c, t]) => lg.append(h("span", {}, h("i", { class: "sw", style: `background:${c}` }), t)));
  host.append(lg);

  const total = Math.max(1, a.summary.tokTotal);
  const biggest = Math.max(1, ...a.seats.map((q) => a.perSeat[q].input + a.perSeat[q].output));
  const grid = h("div", { class: "share", role: "list", "aria-label": "Token share per seat" });
  for (const seat of a.seats) {
    const p = a.perSeat[seat], tot = p.input + p.output, c = seatColor(a, seat);
    const track = h("div", { class: "track", style: `width:${(tot / biggest) * 100}%` },
      h("div", { style: `background:${c};width:${tot ? (p.input / tot) * 100 : 0}%` }),
      h("div", { style: `background:${c};opacity:.5;width:${tot ? (p.output / tot) * 100 : 0}%` }));
    grid.append(h("div", { role: "listitem" }, h("strong", {}, seat)), h("div", {}, track));
    grid.append(h("div", { class: "nums" }, `${fmt(tot)} tokens, ${((tot / total) * 100).toFixed(1)}% (in ${fmt(p.input)}, out ${fmt(p.output)}, ${p.turns} turns)`));
  }
  shareHost.replaceChildren(grid, h("p", { class: "muted" }, "Solid segment is input tokens, faded segment is output tokens."));
}

// ---------- summary ----------
const ROWS = [
  ["Turns", "turns"], ["Tokens in", "tokIn"], ["Tokens out", "tokOut"], ["Tokens total", "tokTotal"],
  ["Wall ms", "wall"], ["Turn latency p50 ms", "p50"], ["Turn latency p95 ms", "p95"],
  ["Max concurrent turns", "maxConc"], ["Tool calls", "tools"], ["Refused calls", "refused"],
  ["Rounds", "rounds"], ["Conductor events", "conducted"], ["Outcome", "outcome"],
];
function renderSummary() {
  const host = $("summary"), pick = $("pick");
  const selA = $("selA"), selB = $("selB");
  const two = runs.length >= 2;
  pick.hidden = !two;
  if (two) {
    const keep = [selA.value, selB.value];
    for (const sel of [selA, selB]) sel.replaceChildren(...runs.map((r, i) => h("option", { value: i }, r.name)));
    selA.value = keep[0] && keep[0] < runs.length ? keep[0] : 0;
    selB.value = keep[1] && keep[1] < runs.length ? keep[1] : 1;
  }
  const shown = two ? [runs[+selA.value], runs[+selB.value]] : runs;
  const head = h("tr", {}, h("th", { scope: "col" }, "Metric"));
  shown.forEach((r, i) => head.append(h("th", { scope: "col" }, two ? `${i ? "B" : "A"}: ${r.name}` : r.name)));
  if (two) head.append(h("th", { scope: "col" }, "Delta (B - A)"), h("th", { scope: "col" }, "Delta %"));
  const body = h("tbody");
  for (const [label, key] of ROWS) {
    const tr = h("tr", {}, h("th", { scope: "row", class: "l" }, label));
    shown.forEach((r) => tr.append(h("td", {}, typeof r.a.summary[key] === "string" ? r.a.summary[key] : fmt(r.a.summary[key]))));
    if (two) {
      const va = shown[0].a.summary[key], vb = shown[1].a.summary[key];
      if (typeof va === "number" && typeof vb === "number" && va != null && vb != null) {
        const d = vb - va, pct = va ? (d / va) * 100 : null;
        const cls = d > 0 ? "up" : d < 0 ? "down" : "";
        const arrow = d > 0 ? "▲ " : d < 0 ? "▼ " : "";
        tr.append(h("td", { class: cls }, `${arrow}${d > 0 ? "+" : ""}${fmt(d)}`), h("td", { class: cls }, pct == null ? "-" : `${pct > 0 ? "+" : ""}${pct.toFixed(1)}%`));
      } else tr.append(h("td", {}, "-"), h("td", {}, "-"));
    }
    body.append(tr);
  }
  host.replaceChildren(h("table", {}, h("thead", {}, head), body));
  if (two) host.append(h("p", { class: "muted" }, "Red up arrow means B is higher, green down arrow means B is lower. Neither colour judges which is better."));
}

// ---------- per-run section ----------
function eventTable(run) {
  const a = run.a, tb = h("tbody"), cap = 1000;
  run.events.slice(0, cap).forEach((e) => {
    tb.append(h("tr", {}, h("td", {}, String(e.seq)), h("td", {}, fmt(e.at_ms - a.t0)), h("td", { class: "l" }, e.event), h("td", { class: "l" }, JSON.stringify(detail(e)))));
  });
  const head = h("tr", {}, ...["seq", "+ms", "event", "fields"].map((t, i) => h("th", { scope: "col", class: i > 1 ? "l" : "" }, t)));
  return h("details", {}, h("summary", {}, `Event table (${run.events.length} events${run.events.length > cap ? `, first ${cap}` : ""})`),
    h("div", { class: "scroll", tabindex: "0", role: "region", "aria-label": `Events of ${run.name}` }, h("table", {}, h("thead", {}, head), tb)));
}

function renderRuns() {
  const span = Math.max(1, ...runs.map((r) => r.a.t1 - r.a.t0)); // shared axis keeps runs comparable
  const out = $("runs");
  out.replaceChildren();
  runs.forEach((run, i) => {
    const wf = h("div", { class: "scroll", tabindex: "0", role: "region", "aria-label": `Waterfall of ${run.name}` });
    const tl = h("div", { class: "scroll", tabindex: "0", role: "region", "aria-label": `Timeline of ${run.name}` });
    const tk = h("div", { class: "scroll", tabindex: "0", role: "region", "aria-label": `Cumulative tokens of ${run.name}` });
    const sh = h("div", {});
    const lg = h("div", { class: "legend" });
    run.a.seats.forEach((seat) => lg.append(h("span", {}, h("i", { class: "sw", style: `background:${seatColor(run.a, seat)}` }), seat)));
    const sec = h("section", { class: "card", "aria-labelledby": `run-${i}` },
      h("h2", { id: `run-${i}` }, `Run: ${run.name}`),
      h("p", { class: "muted" }, `${run.events.length} events, ${run.a.seats.length} seats, ${fmt(run.a.t1 - run.a.t0)} ms. Axes are shared across loaded runs (${fmt(span)} ms).`),
      h("h3", {}, "Per-seat waterfall"), lg, wf,
      h("p", { class: "muted" }, "Bars are turns. Dark ticks are tool calls; red ticks marked × were refused. Overlapping turns of one seat stack."),
      h("h3", {}, "Room timeline"), tl,
      h("h3", {}, "Cumulative tokens"), tk,
      h("h3", {}, "Token share per seat"), sh,
      eventTable(run));
    out.append(sec);
    const draw = () => { waterfall(run, span, wf); timeline(run, span, tl); tokenCharts(run, span, tk, sh); };
    run.redraw = draw; draw();
  });
}

function refresh() {
  $("out").hidden = runs.length === 0;
  if (runs.length) { renderSummary(); renderRuns(); }
}

// ---------- loading ----------
function status(msg, err) { const n = $("status"); n.textContent = msg; n.className = err ? "err" : "muted"; }

async function ingest(items) { // items: [{name, text}]
  let added = 0, bad = 0;
  for (const it of items) {
    const p = parseJsonl(it.text, it.name);
    bad += p.bad; added += addRuns(p, it.name, it.label, it.source);
  }
  refresh();
  const note = bad ? ` ${bad} unreadable line${bad > 1 ? "s" : ""} skipped.` : "";
  if (!added) status(`No events found in ${items.map((i) => i.name).join(", ")}.${note}`, true);
  else status(`Loaded ${added} run${added > 1 ? "s" : ""}; ${runs.length} total.${note}`, bad > 0);
}

async function loadFiles(files) {
  const items = [];
  for (const f of files) items.push({ name: f.name, text: await f.text() });
  await ingest(items);
}

$("picker").addEventListener("change", (e) => { loadFiles(e.target.files); e.target.value = ""; });
$("clear").addEventListener("click", () => { runs.length = 0; refresh(); status("Cleared.", false); panel.cleared(); });
$("selA").addEventListener("change", renderSummary);
$("selB").addEventListener("change", renderSummary);
const drop = $("drop");
["dragenter", "dragover"].forEach((t) => drop.addEventListener(t, (e) => { e.preventDefault(); drop.classList.add("over"); }));
["dragleave", "drop"].forEach((t) => drop.addEventListener(t, (e) => { e.preventDefault(); drop.classList.remove("over"); }));
drop.addEventListener("drop", (e) => loadFiles(e.dataTransfer.files));
document.addEventListener("dragover", (e) => e.preventDefault());
document.addEventListener("drop", (e) => { if (!drop.contains(e.target)) { e.preventDefault(); loadFiles(e.dataTransfer.files); } });
let resizeTimer;
window.addEventListener("resize", () => { clearTimeout(resizeTimer); resizeTimer = setTimeout(() => runs.forEach((r) => r.redraw && r.redraw()), 150); });

const panel = initPanel({
  runs,
  ingest,
  status,
  // Drop every run that came from `source`, keeping the rest in order.
  remove(source) {
    for (let i = runs.length - 1; i >= 0; i--) if (runs[i].source === source) runs.splice(i, 1);
    refresh();
  },
  // Swap the runs of `source` for freshly parsed ones, keeping their position.
  replace(source, item) {
    const at = runs.findIndex((r) => r.source === source);
    const rest = runs.splice(at < 0 ? runs.length : at);
    const keep = rest.filter((r) => r.source !== source);
    const p = parseJsonl(item.text, item.name);
    addRuns(p, item.name, item.label, item.source);
    runs.push(...keep);
    refresh();
  },
  select(a, b) { // pick the A and B columns of the summary by run name
    const names = runs.map((r) => r.name);
    if (a != null && names.includes(a)) $("selA").value = a;
    if (b != null && names.includes(b)) $("selB").value = b;
    renderSummary();
  },
});
