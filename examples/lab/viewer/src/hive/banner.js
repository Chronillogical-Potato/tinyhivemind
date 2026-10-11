// The ending banner: how the episode finished, on what, and (for a math
// package) whether the room's answer was right. A multi-desk run also names
// the desk whose ending is the hive's answer and lists every desk's own
// ending; a live run reports its model usage once the binary sends it.
import { h } from "./dom.js";

const WORDS = {
  converged: "Converged",
  deadlocked: "Deadlocked",
  exhausted: "Exhausted the turn budget",
  idle: "Went idle",
  unopened: "Never opened",
};

/** The banner for `state`, or null while the episode is still running. */
export function endingBanner(state) {
  const end = state.ending;
  if (!end) return null;
  const on = end.topic != null ? ` on “${end.topic}”` : "";
  const verdict = end.correct == null ? null : h("strong", { class: end.correct ? "down" : "up" }, end.correct ? " Correct." : ` Wrong (answer ${state.problem?.answer ?? "?"}).`);
  const desk = end.desk ? h("span", {}, " The answer is desk ", h("strong", {}, end.desk), "’s ending.") : null;
  return h("div", { class: `hv-banner ${end.ending}`, role: "status", id: "hv-ending" },
    h("strong", {}, `${WORDS[end.ending] ?? end.ending}${on}`),
    ` after ${end.rounds} rounds and ${end.turns} turns.`, verdict, desk);
}

/** One chip per desk that has ended so far, or null when none has. */
export function deskEndings(state) {
  const ends = [...(state.deskEnds?.values() ?? [])];
  if (!ends.length) return null;
  const answer = state.ending?.desk ?? null;
  return h("div", { class: "hv-desk-ends", id: "hv-desk-ends", "aria-label": "Desk endings" },
    h("span", { class: "muted" }, "Desks: "),
    ...ends.map((e) => {
      const tone = e.correct === true ? " right" : e.correct === false ? " wrong" : "";
      const text = `${e.desk}: ${(WORDS[e.ending] ?? e.ending).toLowerCase()}${e.topic != null ? ` · ${e.topic}` : ""}`;
      return h("span", { class: `hv-desk-chip ${e.ending}${tone}${e.desk === answer ? " answer" : ""}`, "data-desk": e.desk,
        title: `${e.rounds ?? "?"} rounds, ${e.turns ?? "?"} turns${e.desk === answer ? " · the hive's answer" : ""}` }, text);
    }));
}

const fmt = (n) => (Number.isFinite(n) ? n.toLocaleString("en-US") : "?");

/** Wall time in words: "850 ms", "12.3 s", "4 min 05 s". */
export function wallTime(ms) {
  if (!Number.isFinite(ms)) return "?";
  if (ms < 1000) return `${Math.round(ms)} ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)} s`;
  const s = Math.round(ms / 1000);
  return `${Math.floor(s / 60)} min ${String(s % 60).padStart(2, "0")} s`;
}

/** The live run's usage line, or null before the usage frame arrives. */
export function usageLine(usage) {
  if (!usage) return null;
  const cost = Number.isFinite(usage.cost_usd) ? `$${usage.cost_usd.toFixed(4)}` : "cost unknown";
  return h("p", { class: `hv-usage${usage.exhausted_calls ? " exhausted" : ""}`, id: "hv-usage", role: "status" },
    h("strong", {}, "Model usage: "),
    `${fmt(usage.calls)} calls · ${fmt(usage.prompt_tokens)} prompt + ${fmt(usage.completion_tokens)} completion = ${fmt(usage.total_tokens)} tokens · ${cost} · ${wallTime(usage.wall_ms)}`,
    usage.exhausted_calls ? h("strong", { class: "up" }, " · stopped at the call cap") : null);
}
