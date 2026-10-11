// Where everything sits in the hive graph. Pure geometry over the `--json`
// structure: desks are boxes in a wrapping grid, each seat sits in its first
// desk, and a hive with many seats switches to compact, unlabelled nodes so
// fifty of them still fit on a laptop screen.

const COMPACT_OVER = 16;
const PAD = 12, HEAD = 24, GAP = 46;

/** Seat sizing for a hive of `n` seats. */
export function nodeSize(n) {
  return n > COMPACT_OVER ? { compact: true, r: 9, cellW: 26, cellH: 26 } : { compact: false, r: 15, cellW: 92, cellH: 58 };
}

/** Each seat's home desk: its first listed desk that exists, else "(no desk)". */
export function homeDesks(structure) {
  const known = new Set((structure.desks ?? []).map((d) => d.id));
  const groups = new Map((structure.desks ?? []).map((d) => [d.id, []]));
  for (const seat of structure.seats ?? []) {
    const home = (seat.desks ?? []).find((d) => known.has(d)) ?? "(no desk)";
    if (!groups.has(home)) groups.set(home, []);
    groups.get(home).push(seat);
  }
  return groups;
}

/** The point where the segment from the centre of `box` toward (x, y) leaves it. */
export function exitPoint(box, x, y) {
  const cx = box.x + box.w / 2, cy = box.y + box.h / 2;
  const dx = x - cx, dy = y - cy;
  if (!dx && !dy) return { x: cx, y: cy };
  const t = Math.min(dx ? box.w / 2 / Math.abs(dx) : Infinity, dy ? box.h / 2 / Math.abs(dy) : Infinity);
  return { x: cx + dx * t, y: cy + dy * t };
}

/**
 * Lay out a hive at `width` pixels: `{ width, height, size, desks: Map<id,
 * {id, name, x, y, w, h}>, seats: Map<id, {seat, desk, x, y}> }`.
 */
export function layout(structure, width) {
  const groups = homeDesks(structure);
  const size = nodeSize((structure.seats ?? []).length);
  const names = new Map((structure.desks ?? []).map((d) => [d.id, d.name || d.id]));
  const ids = [...groups.keys()].filter((id) => groups.get(id).length || names.has(id));
  const largest = Math.max(1, ...ids.map((id) => groups.get(id).length));
  // Widest desk we would like: room for every seat of the largest desk on
  // `perRow` columns; then fit as many such columns as the width allows.
  const perRow = size.compact ? Math.min(largest, 8) : Math.min(largest, 4);
  const want = perRow * size.cellW + PAD * 2;
  const cols = Math.max(1, Math.min(ids.length, Math.floor((width + GAP) / (Math.min(want, width) + GAP))));
  const deskW = Math.floor((width - GAP * (cols - 1)) / cols);
  const inner = Math.max(1, Math.floor((deskW - PAD * 2) / size.cellW));
  const desks = new Map();
  const seats = new Map();
  let y = 0;
  for (let row = 0; row * cols < ids.length; row++) {
    const rowIds = ids.slice(row * cols, row * cols + cols);
    const rowH = Math.max(...rowIds.map((id) => Math.ceil(Math.max(1, groups.get(id).length) / inner))) * size.cellH + HEAD + PAD;
    rowIds.forEach((id, i) => {
      const x = i * (deskW + GAP);
      desks.set(id, { id, name: names.get(id) ?? id, x, y, w: deskW, h: rowH });
      const members = groups.get(id);
      const used = Math.min(members.length, inner) * size.cellW;
      const left = x + (deskW - used) / 2;
      members.forEach((seat, k) => {
        seats.set(seat.id, {
          seat,
          desk: id,
          x: left + (k % inner) * size.cellW + size.cellW / 2,
          y: y + HEAD + Math.floor(k / inner) * size.cellH + (size.compact ? size.cellH / 2 : size.r + 4),
        });
      });
    });
    y += rowH + GAP;
  }
  return { width, height: Math.max(0, y - GAP), size, desks, seats };
}

/** A compact node's glyph: trailing digits ("verify-03" → "03"), else two letters. */
export function glyph(label) {
  const digits = /(\d+)$/.exec(label);
  return digits ? digits[1].slice(-2) : label.slice(0, 2);
}
