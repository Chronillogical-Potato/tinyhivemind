// Which timeline lane a mark-like event belongs to, and how it is titled.
// Pure, so `npm test` can pin it without a browser.
//
// The SWE hive emits core's typed events: `session_resumed` when a seat
// resumes its persistent session, `recalled` and `remembered` for each memory
// call. A failed memory call also leaves a `mark` labelled `memory` with the
// reason. Older traces used `mark` labels `session` and `memory` for all of
// it; those still land in the same lanes.

/** Events drawn in the mark lanes. */
export const MARK_EVENTS = new Set(["mark", "checkpoint", "session_resumed", "recalled", "remembered"]);

/** Lanes, top to bottom: seat sessions, hive memory, everything else. */
export const MARK_LANES = [
  { key: "session", label: "session" },
  { key: "memory", label: "memory" },
  { key: "marks", label: "marks" },
];

/** The lane of one mark-like event. */
export function markLane(e) {
  if (e.event === "session_resumed") return "session";
  if (e.event === "recalled" || e.event === "remembered") return "memory";
  if (e.event === "mark" && e.label === "session") return "session";
  if (e.event === "mark" && e.label === "memory") return "memory";
  return "marks";
}

/** Whether a memory event reports a failed or timed-out call. */
export function memoryFailed(e) {
  return e.event === "mark" && e.label === "memory" && typeof e.detail === "string" && e.detail.includes("error=");
}

/** A one-line title for the tooltip. */
export function markTitle(e) {
  switch (e.event) {
    case "session_resumed":
      return `${e.seat} resumed its session: ${e.messages} messages, ${e.delta_rows} new desk rows`;
    case "recalled":
      return `${e.seat} recalled (${e.moment}): ${e.notes} notes, ${e.chars} chars, ${e.latency_ms} ms`;
    case "remembered":
      return `${e.seat} remembered ${e.entries} entries in ${e.latency_ms} ms`;
    default:
      return `${e.event}: ${e.label}${e.detail ? " " + e.detail : ""}`;
  }
}

/** Group events by lane key. */
export function byLane(events) {
  const out = Object.fromEntries(MARK_LANES.map((l) => [l.key, []]));
  for (const e of events) out[markLane(e)].push(e);
  return out;
}
