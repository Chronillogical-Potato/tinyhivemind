// Pure: what a turn did besides speak, from the optional fields a recording
// host adds to `turns[]` — `tool_calls: [{tool, summary, ok}]` and
// `memory: {recalled, remembered}` (counts, or lists of entries) — and the
// same summed per seat for the graph. Also the pure half of the recordings
// panels: which runs are live, which finished, and which just started.

const ICONS = { run_python: "🐍", python: "🐍", web_search: "🔎", web_fetch: "🔎", search: "🔎" };

/** The icon for a tool name: 🐍 for python, 🔎 for search, 🔧 for anything else. */
export const toolIcon = (tool) => ICONS[tool] ?? (/python/.test(tool ?? "") ? "🐍" : /search|fetch/.test(tool ?? "") ? "🔎" : "🔧");

/** A memory count from a number or a list; 0 for anything else. */
const count = (v) => (Array.isArray(v) ? v.length : Number.isFinite(v) ? Math.max(0, v) : 0);

/** `{ tools: [{tool, summary, ok, icon}], recalled, remembered }` for one turn. */
export function turnActivity(turn) {
  const tools = (Array.isArray(turn?.tool_calls) ? turn.tool_calls : []).map((c) => ({
    tool: String(c?.tool ?? "tool"),
    summary: c?.summary == null ? "" : String(c.summary),
    ok: c?.ok == null ? null : !!c.ok,
    icon: toolIcon(c?.tool),
  }));
  return { tools, recalled: count(turn?.memory?.recalled), remembered: count(turn?.memory?.remembered) };
}

/** True when a turn carries any tool call or memory activity. */
export const hasActivity = (a) => a.tools.length > 0 || a.recalled > 0 || a.remembered > 0;

/** Per seat, summed over `lines`: `Map<seat, {tools, failed, recalled, remembered}>` (only seats with any). */
export function seatActivity(lines) {
  const out = new Map();
  for (const l of lines) {
    const a = turnActivity(l);
    if (!hasActivity(a)) continue;
    const s = out.get(l.seat) ?? { tools: 0, failed: 0, recalled: 0, remembered: 0 };
    s.tools += a.tools.length;
    s.failed += a.tools.filter((t) => t.ok === false).length;
    s.recalled += a.recalled;
    s.remembered += a.remembered;
    out.set(l.seat, s);
  }
  return out;
}

/**
 * Recordings still being written (newest first): changed in the last 30 s,
 * or quiet for a while (a live round can take minutes) with frames already
 * in it. An empty folder that went quiet is not a run in progress.
 */
const inProgress = (r) => !r.ended && (r.live || (r.active && r.frames > 0));
export const liveNow = (list) => list.filter(inProgress);

/** Every other recording: ended, or stopped without an end frame. */
export const finished = (list) => list.filter((r) => !inProgress(r));

/** Live recordings in `next` that were not in `prev` (null on the first listing: nothing is new then). */
export function newlyLive(prev, next) {
  if (!prev) return [];
  const seen = new Set(prev.map((r) => r.id));
  return liveNow(next).filter((r) => !seen.has(r.id));
}

/** Elapsed time in words: "45 s", "3 min", "2 h 05 min". */
export function elapsed(ms) {
  if (!Number.isFinite(ms) || ms < 0) return "?";
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min`;
  return `${Math.floor(m / 60)} h ${String(m % 60).padStart(2, "0")} min`;
}

/** One line about a recording: package, problem or seed, round and desk, turns. */
export function recordingLabel(r) {
  const what = r.problem?.id ? `problem ${r.problem.id}` : r.seed != null ? `seed ${r.seed}` : "";
  const where = r.round != null ? `round ${r.round}${r.desk ? ` on ${r.desk}` : ""}` : "not started";
  return [r.package, what, where, `${r.calls ?? r.turns ?? 0} calls`].filter(Boolean).join(" · ");
}
