// The ending banner: how the episode finished, on what, and (for a math
// package) whether the room's answer was right.
import { h } from "./dom.js";

const WORDS = {
  converged: "Converged",
  deadlocked: "Deadlocked",
  exhausted: "Exhausted the turn budget",
  idle: "Went idle",
};

/** The banner for `state`, or null while the episode is still running. */
export function endingBanner(state) {
  const end = state.ending;
  if (!end) return null;
  const on = end.topic != null ? ` on “${end.topic}”` : "";
  const verdict = end.correct == null ? null : h("strong", { class: end.correct ? "down" : "up" }, end.correct ? " Correct." : ` Wrong (answer ${state.problem?.answer ?? "?"}).`);
  return h("div", { class: `hv-banner ${end.ending}`, role: "status", id: "hv-ending" },
    h("strong", {}, `${WORDS[end.ending] ?? end.ending}${on}`),
    ` after ${end.rounds} rounds and ${end.turns} turns.`, verdict);
}
