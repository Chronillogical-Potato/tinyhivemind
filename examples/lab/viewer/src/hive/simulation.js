// The pure half of the player: frames from `hive_view --run --frames` folded
// into the state of the room after N rounds. No DOM, so it is unit-tested.
// Step 0 is the room before anyone speaks; step `rounds.length` is the last
// round, and the ending shows once the `end` frame has arrived and the player
// stands on the last step.

/** Group a frame list into `{ start, rounds, end }`, ignoring unknown types. */
export function timeline(frames) {
  const tl = { start: null, rounds: [], end: null };
  for (const f of frames) {
    if (f?.type === "start") tl.start = f;
    else if (f?.type === "round") tl.rounds.push(f);
    else if (f?.type === "end") tl.end = f;
  }
  return tl;
}

/** The step index nearest `n` that exists. */
export function clampStep(tl, n) {
  const v = Math.round(Number.isFinite(n) ? n : 0);
  return Math.max(0, Math.min(tl.rounds.length, v));
}

/** Move `by` steps from `step`; never past either end. */
export const stepBy = (tl, step, by) => clampStep(tl, step + by);

/** True when the player is on the last step and the episode has ended. */
export const atEnd = (tl, step) => tl.end != null && step >= tl.rounds.length;

/** Desks whose seats spoke in a round. */
const desksOf = (round) => new Set((round?.turns ?? []).map((t) => t.desk).filter(Boolean));

/**
 * Desk pairs the conversation moved across between two consecutive rounds:
 * every (desk that spoke before, desk that speaks now) pair of different desks.
 */
export function crossings(prev, round) {
  const before = desksOf(prev);
  const now = desksOf(round);
  const out = [];
  for (const from of before) for (const to of now) if (from !== to && !before.has(to)) out.push({ from, to });
  return out;
}

/** The room after `n` rounds. */
export function stateAt(tl, n) {
  const step = clampStep(tl, n);
  const lines = [];
  const answers = new Map();
  const spoke = new Map();
  let standings = [];
  for (let i = 0; i < step; i++) {
    const r = tl.rounds[i];
    for (const t of r.turns ?? []) {
      lines.push({ ...t, index: lines.length, round: r.round ?? i + 1, visibility: r.visibility, phase: r.phase });
      spoke.set(t.seat, (spoke.get(t.seat) ?? 0) + 1);
      if (t.answer != null) answers.set(t.seat, { answer: t.answer, correct: t.correct ?? null });
    }
    if (Array.isArray(r.standings) && r.standings.length) standings = r.standings;
  }
  const round = step ? tl.rounds[step - 1] : null;
  const active = new Map((round?.turns ?? []).map((t) => [t.seat, t]));
  return {
    step,
    total: tl.rounds.length,
    start: tl.start,
    problem: tl.start?.problem ?? null,
    round,
    active,
    lines,
    answers,
    spoke,
    standings,
    crossings: step ? crossings(step > 1 ? tl.rounds[step - 2] : null, round) : [],
    ending: atEnd(tl, step) ? tl.end : null,
  };
}

/**
 * The transcript lines a seat's most recent turn saw: of the `of` lines in
 * front of it, the `saw` most recent. A blind round hides its own peers, so
 * only lines from earlier rounds count as in front of a blind turn.
 * Returns `{ turn, seen: Set<line index> }`, or null if the seat has not spoken.
 */
export function sawBy(state, seat) {
  let mine = null;
  for (const l of state.lines) if (l.seat === seat) mine = l;
  if (!mine) return null;
  const before = state.lines.filter((l) =>
    l.index < mine.index && (l.round < mine.round || (mine.visibility !== "blind" && l.round === mine.round)));
  const n = Math.max(0, Math.min(mine.saw ?? 0, before.length));
  return { turn: mine, seen: new Set(before.slice(before.length - n).map((l) => l.index)) };
}
