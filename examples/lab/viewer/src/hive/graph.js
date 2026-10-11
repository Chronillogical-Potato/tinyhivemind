// The hive as plain SVG: desks are boxes, seats are nodes clustered in their
// home desk, topology links are solid arrows and referral links dashed ones.
// The current round's seats glow (dashed ring = blind round, solid = full),
// and an edge the conversation just crossed animates. Speech bubbles sit in
// their own layer so hovering a seat redraws only them.
import { clip, h, s, svgRoot, wrap } from "./dom.js";
import { exitPoint, glyph, layout } from "./layout.js";

const MARGIN = { compact: 10, roomy: 70 };

function roleColor(structure) {
  const roles = (structure.roles ?? []).map((r) => r.id);
  return (role) => `var(--s${Math.max(0, roles.indexOf(role)) % 8})`;
}

function edgeList(structure, crossings) {
  const edges = [
    ...(structure.topology ?? []).map((e) => ({ ...e, kind: "topology" })),
    ...(structure.referrals ?? []).map((e) => ({ ...e, kind: "referral" })),
  ];
  for (const c of crossings) {
    const hit = edges.filter((e) => (e.from === c.from && e.to === c.to) || (e.from === c.to && e.to === c.from));
    if (hit.length) {
      const best = hit.find((e) => e.kind === "referral") ?? hit[0];
      best.live = true;
      best.liveFrom = c.from;
    } else edges.push({ ...c, kind: "handoff", live: true, liveFrom: c.from });
  }
  return edges;
}

function edgePath(a, b, kind) {
  const ca = { x: a.x + a.w / 2, y: a.y + a.h / 2 }, cb = { x: b.x + b.w / 2, y: b.y + b.h / 2 };
  const [lo, hi] = a.id < b.id ? [ca, cb] : [cb, ca];
  const len = Math.hypot(hi.x - lo.x, hi.y - lo.y) || 1;
  const nx = -(hi.y - lo.y) / len, ny = (hi.x - lo.x) / len;
  const bend = (kind === "topology" ? 1 : -1) * Math.max(16, len * 0.16);
  const mx = (ca.x + cb.x) / 2 + nx * bend, my = (ca.y + cb.y) / 2 + ny * bend;
  const p = exitPoint(a, mx, my), q = exitPoint(b, mx, my);
  return { d: `M${p.x},${p.y} Q${mx},${my} ${q.x},${q.y}`, mx, my };
}

function arrowDefs() {
  const defs = s("defs");
  for (const [id, color] of [["topology", "var(--muted)"], ["referral", "var(--accent)"], ["handoff", "var(--accent)"]]) {
    const m = s("marker", { id: `hv-arrow-${id}`, viewBox: "0 0 10 10", refX: 9, refY: 5, markerWidth: 9, markerHeight: 9, markerUnits: "userSpaceOnUse", orient: "auto-start-reverse" });
    m.append(s("path", { d: "M0,0 L10,5 L0,10 z", fill: color }));
    defs.append(m);
  }
  return defs;
}

function drawEdges(g, structure, lay, crossings) {
  for (const e of edgeList(structure, crossings)) {
    const a = lay.desks.get(e.from), b = lay.desks.get(e.to);
    if (!a || !b || a === b) continue;
    const { d } = edgePath(a, b, e.kind);
    const reverse = e.live && e.liveFrom === e.to;
    g.append(s("path", { d, class: "hv-edge-halo" }));
    const path = s("path", {
      d,
      class: `hv-edge ${e.kind}${e.live ? " live" : ""}${reverse ? " reverse" : ""}`,
      "marker-end": `url(#hv-arrow-${e.kind})`,
    });
    path.append(s("title", {}, `${e.kind}: ${e.from} → ${e.to}${e.live ? " (crossed this round)" : ""}`));
    g.append(path);
  }
}

function drawDesks(g, structure, lay, state) {
  const meta = new Map((structure.desks ?? []).map((d) => [d.id, d]));
  const busy = new Set([...state.active.values()].map((t) => t.desk));
  for (const desk of lay.desks.values()) {
    const m = meta.get(desk.id);
    g.append(s("rect", { x: desk.x, y: desk.y, width: desk.w, height: desk.h, rx: 8, class: `hv-desk${busy.has(desk.id) ? " busy" : ""}` }));
    const count = m?.members?.length ?? 0;
    g.append(s("text", { x: desk.x + 10, y: desk.y + 16, class: "hv-desk-name" }, clip(desk.name, 22)));
    g.append(s("text", { x: desk.x + desk.w - 10, y: desk.y + 16, "text-anchor": "end", class: "dim" }, `${count} · ${m?.responder_mode ?? ""}`));
  }
}

function seatTitle(seat, state) {
  const bits = [`${seat.label ?? seat.id} (${seat.role})`, `desks: ${(seat.desks ?? []).join(", ")}`, `turns: ${state.spoke.get(seat.id) ?? 0}`];
  const a = state.answers.get(seat.id);
  if (a) bits.push(`answer: ${a.answer}${a.correct == null ? "" : a.correct ? " (correct)" : " (wrong)"}`);
  return bits.join("\n");
}

function drawSeats(g, structure, lay, state, ui, onHover) {
  const color = roleColor(structure);
  const blind = state.round?.visibility === "blind";
  const { r, compact } = lay.size;
  for (const { seat, x, y } of lay.seats.values()) {
    const turn = state.active.get(seat.id);
    const ans = state.answers.get(seat.id);
    const node = s("g", { class: "hv-seat", transform: `translate(${x},${y})`, tabindex: 0, role: "button",
      "aria-pressed": String(ui.selected === seat.id), "aria-label": `${seat.label ?? seat.id}, ${seat.role}` });
    if (turn) node.append(s("circle", { r: r + 6, class: "hv-halo" }));
    const fill = ans?.correct === true ? "hv-right" : ans?.correct === false ? "hv-wrong" : turn ? (blind ? "hv-blind" : "hv-full") : state.spoke.has(seat.id) ? "hv-spoke" : "";
    node.append(s("circle", { r, stroke: color(seat.role), class: `hv-node ${fill}${turn ? " active" : ""}${turn && blind ? " blind" : ""}${ui.selected === seat.id ? " selected" : ""}` }));
    if (compact) node.append(s("text", { y: 3.5, "text-anchor": "middle", class: "hv-glyph" }, glyph(seat.label ?? seat.id)));
    else {
      node.append(s("text", { y: r + 13, "text-anchor": "middle" }, clip(seat.label ?? seat.id, 13)));
      if (ans) {
        const w = Math.max(22, String(ans.answer).length * 7 + 10);
        node.append(s("rect", { x: r - 4, y: -r - 8, width: w, height: 15, rx: 7, class: `hv-chip ${ans.correct === false ? "wrong" : ans.correct ? "right" : ""}` }));
        node.append(s("text", { x: r - 4 + w / 2, y: -r + 3, "text-anchor": "middle", class: "hv-chip-text" }, clip(String(ans.answer), 8)));
      }
    }
    node.append(s("title", {}, seatTitle(seat, state)));
    node.addEventListener("click", () => ui.onSelect(seat.id));
    node.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); ui.onSelect(seat.id); } });
    node.addEventListener("pointerenter", () => onHover(seat.id));
    node.addEventListener("pointerleave", () => onHover(null));
    node.addEventListener("focus", () => onHover(seat.id));
    node.addEventListener("blur", () => onHover(null));
    g.append(node);
  }
}

function lastLine(state, seat) {
  for (let i = state.lines.length - 1; i >= 0; i--) if (state.lines[i].seat === seat) return state.lines[i];
  return null;
}

/** Which seats get a bubble: hovered and selected first, then the round's speakers. */
function bubbleSeats(state, ui, hover, compact) {
  const out = [];
  const add = (id) => { if (id && !out.includes(id)) out.push(id); };
  add(hover);
  add(ui.selected);
  const limit = compact ? 3 : 8;
  const desks = new Set();
  const active = [...state.active.values()];
  for (const t of active) if (out.length < limit + (hover ? 1 : 0) && !desks.has(t.desk)) { desks.add(t.desk); add(t.seat); }
  for (const t of active) if (out.length < limit + (hover ? 1 : 0)) add(t.seat);
  return out;
}

function drawBubbles(layer, lay, state, ui, hover, top, bottom) {
  layer.replaceChildren();
  // Desk headers and seats are obstacles, so a bubble never hides a desk's
  // name or a node; only a hovered or selected seat's bubble may overlap.
  const placed = [...lay.desks.values()].map((d) => ({ x: d.x, y: d.y, w: d.w, h: 22 }));
  if (lay.size.compact) for (const p of lay.seats.values()) placed.push({ x: p.x - lay.size.r, y: p.y - lay.size.r, w: lay.size.r * 2, h: lay.size.r * 2 });
  const overlaps = (b) => placed.some((p) => b.x < p.x + p.w + 4 && p.x < b.x + b.w + 4 && b.y < p.y + p.h + 4 && p.y < b.y + b.h + 4);
  for (const id of bubbleSeats(state, ui, hover, lay.size.compact)) {
    const at = lay.seats.get(id);
    const line = state.active.get(id) ?? lastLine(state, id);
    if (!at || !line) continue;
    const old = !state.active.has(id);
    const text = wrap(line.content ?? "", lay.size.compact ? 44 : 34, 3);
    const w = Math.min(lay.width - 4, (lay.size.compact ? 290 : 230));
    const bh = 20 + text.length * 14;
    const x = Math.max(2, Math.min(lay.width - w - 2, at.x - w / 2));
    const gap = lay.size.r + 8;
    const tries = [-gap - bh, gap, -gap - bh * 2 - 6, gap + bh + 6, -gap - bh * 3 - 12, gap + bh * 2 + 12].map((dy) => ({ x, y: at.y + dy, w, h: bh }));
    const fits = (b) => b.y >= top && b.y + b.h <= bottom;
    const spot = tries.find((b) => fits(b) && !overlaps(b)) ?? (id === hover || id === ui.selected || !lay.size.compact ? tries.find(fits) : null);
    if (!spot) continue;
    placed.push(spot);
    const g = s("g", { class: `hv-bubble${old ? " old" : ""}`, "pointer-events": "none" });
    const tipY = spot.y > at.y ? spot.y : spot.y + spot.h;
    g.append(s("path", { d: `M${at.x},${at.y + (spot.y > at.y ? lay.size.r : -lay.size.r)} L${Math.max(spot.x + 8, Math.min(spot.x + w - 8, at.x))},${tipY}`, class: "hv-leader" }));
    g.append(s("rect", { x: spot.x, y: spot.y, width: w, height: bh, rx: 6 }));
    const head = `${line.seat}${old ? ` · round ${line.round}` : ""} · ${line.reason ?? ""} · saw ${line.saw ?? "?"}/${line.of ?? "?"}`;
    g.append(s("text", { x: spot.x + 8, y: spot.y + 14, class: "hv-bubble-head" }, clip(head, lay.size.compact ? 48 : 38)));
    text.forEach((t, i) => g.append(s("text", { x: spot.x + 8, y: spot.y + 29 + i * 14 }, t)));
    layer.append(g);
  }
}

function legend(structure, isMath) {
  const color = roleColor(structure);
  const item = (sw, label) => h("span", {}, sw, label);
  const dot = (style) => h("i", { class: "sw", style });
  return h("div", { class: "legend" },
    ...(structure.roles ?? []).map((r) => item(dot(`border:2px solid ${color(r.id)};border-radius:50%`), r.id)),
    item(h("i", { class: "hv-key solid" }), "topology"),
    item(h("i", { class: "hv-key dashed" }), "referral"),
    item(dot("background:var(--full);border-radius:50%"), "speaking (full)"),
    item(dot("background:var(--blind);border:1px dashed var(--accent);border-radius:50%"), "speaking (blind)"),
    isMath ? item(dot("background:var(--good);border-radius:50%"), "correct answer") : null,
    isMath ? item(dot("background:var(--bad);border-radius:50%"), "wrong answer") : null);
}

/** Draw the hive into `host` for `state`; `ui = {selected, onSelect(seatId)}`. */
export function renderGraph(host, structure, state, ui) {
  const width = Math.max(320, Math.floor(host.clientWidth || 720));
  const lay = layout(structure, width);
  const m = lay.size.compact ? MARGIN.compact : MARGIN.roomy;
  const height = lay.height + m * 2;
  const svg = svgRoot(width, height, `${structure.name ?? structure.id}: ${lay.seats.size} seats in ${lay.desks.size} desks`);
  svg.classList.add("hv-graph");
  svg.append(arrowDefs());
  const g = s("g", { transform: `translate(0,${m})` });
  const bubbles = s("g", { transform: `translate(0,${m})` });
  const onHover = (id) => drawBubbles(bubbles, lay, state, ui, id, -m + 2, lay.height + m - 2);
  drawDesks(g, structure, lay, state);
  drawEdges(g, structure, lay, state.crossings);
  drawSeats(g, structure, lay, state, ui, onHover);
  svg.append(g, bubbles);
  onHover(null);
  host.replaceChildren(svg, legend(structure, structure.sim?.kind === "math"));
}
