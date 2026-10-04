// Which timeline lane a `mark` or `checkpoint` belongs to. Pure, so `npm test`
// can pin it without a browser.

/** Lanes, top to bottom: seat sessions, hive memory, everything else. */
export const MARK_LANES = [
  { key: "session", label: "session" },
  { key: "memory", label: "memory" },
  { key: "marks", label: "marks" },
];

/** The lane of one mark or checkpoint event. */
export function markLane(e) {
  if (e.event === "mark" && e.label === "session") return "session";
  if (e.event === "mark" && e.label === "memory") return "memory";
  return "marks";
}

/** Whether a memory mark reports a failed or timed-out call. */
export function memoryFailed(e) {
  return markLane(e) === "memory" && typeof e.detail === "string" && e.detail.includes(" error=");
}

/** Group events by lane key. */
export function byLane(events) {
  const out = Object.fromEntries(MARK_LANES.map((l) => [l.key, []]));
  for (const e of events) out[markLane(e)].push(e);
  return out;
}
