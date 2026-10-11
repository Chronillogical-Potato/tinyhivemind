// The pure half of the player: frames from `hive_view --run --frames` folded
// into the state of the room after N rounds. No DOM, so it is unit-tested.
// Step 0 is the room before anyone speaks; step `rounds.length` is the last
// round, and the ending shows once the `end` frame has arrived and the player
// stands on the last step. A multi-desk package runs one floor per desk, so a
// round names its `desk`; `referral` and `desk_end` frames land on the step
// that was current when they arrived (see referrals.js).

/**
 * Group a frame list into `{ start, rounds, end, referrals, deskEnds, usage }`,
 * ignoring unknown types. Each referral and desk ending is stamped with
 * `step`, the number of rounds received before it, and referrals with `id`,
 * their arrival order.
 */
export function timeline(frames) {
  const tl = { start: null, rounds: [], end: null, referrals: [], deskEnds: [], usage: null };
  for (const f of frames) {
    if (f?.type === "start") tl.start = f;
    else if (f?.type === "round") tl.rounds.push(f);
    else if (f?.type === "end") tl.end = f;
    else if (f?.type === "referral") tl.referrals.push({ ...f, step: tl.rounds.length, id: tl.referrals.length });
    else if (f?.type === "desk_end") tl.deskEnds.push({ ...f, step: tl.rounds.length });
    else if (f?.type === "usage") tl.usage = f;
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

/** True when rounds name their floor: messages then cross desks only as referrals. */
const perDesk = (tl) => tl.rounds.some((r) => r.desk != null);

/** The room after `n` rounds. */
export function stateAt(tl, n) {
  const step = clampStep(tl, n);
  const lines = [];
  const entries = []; // transcript order: turns, referrals and desk endings
  const answers = new Map();
  const spoke = new Map();
  const deskEnds = new Map();
  let standings = [];
  let standingsDesk = null;
  const shown = (list, i) => list.filter((x) => x.step === i);
  const note = (i) => {
    for (const ref of shown(tl.referrals, i)) entries.push({ kind: "referral", ref });
    for (const end of shown(tl.deskEnds, i)) {
      entries.push({ kind: "desk_end", end });
      deskEnds.set(end.desk, end);
    }
  };
  note(0);
  for (let i = 0; i < step; i++) {
    const r = tl.rounds[i];
    for (const t of r.turns ?? []) {
      const line = { ...t, index: lines.length, round: r.round ?? i + 1, visibility: r.visibility, phase: r.phase,
        floor: r.desk ?? t.desk ?? null, deskRound: r.desk_round ?? null, floorTagged: r.desk != null };
      lines.push(line);
      entries.push({ kind: "turn", line });
      spoke.set(t.seat, (spoke.get(t.seat) ?? 0) + 1);
      if (t.answer != null) answers.set(t.seat, { answer: t.answer, correct: t.correct ?? null });
    }
    // Standings are cumulative within a desk, so an empty report keeps them;
    // on another desk the carried candidates are not that desk's, so it clears them.
    if (Array.isArray(r.standings)) {
      const desk = r.desk ?? null;
      if (r.standings.length) standings = r.standings;
      else if (desk !== standingsDesk) standings = [];
      standingsDesk = desk;
    }
    note(i + 1);
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
    crossings: step && !perDesk(tl) ? crossings(step > 1 ? tl.rounds[step - 2] : null, round) : [],
    entries,
    referrals: tl.referrals.filter((r) => r.step <= step),
    fresh: tl.referrals.filter((r) => r.step === step),
    deskEnds,
    usage: tl.usage,
    ending: atEnd(tl, step) ? tl.end : null,
  };
}

/**
 * The transcript lines a seat's most recent turn saw: of the `of` lines in
 * front of it, the `saw` most recent. A blind round hides its own peers, so
 * only lines from earlier rounds count as in front of a blind turn, and on a
 * per-desk floor only lines of the same floor do.
 * Returns `{ turn, seen: Set<line index> }`, or null if the seat has not spoken.
 */
export function sawBy(state, seat) {
  let mine = null;
  for (const l of state.lines) if (l.seat === seat) mine = l;
  if (!mine) return null;
  const before = state.lines.filter((l) =>
    l.index < mine.index && (!mine.floorTagged || l.floor === mine.floor) &&
    (l.round < mine.round || (mine.visibility !== "blind" && l.round === mine.round)));
  const n = Math.max(0, Math.min(mine.saw ?? 0, before.length));
  return { turn: mine, seen: new Set(before.slice(before.length - n).map((l) => l.index)) };
}
