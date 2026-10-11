// The pure half of desk-to-desk traffic: which declared link a referral frame
// travelled, how to word it, and which transcript entries belong to one desk.
// No DOM, so it is unit-tested under node; graph.js and transcript.js draw it.

const REASONS = {
  hop_limit_reached: "hop limit reached",
  unlinked: "no link between these desks",
  desk_closed: "target desk already closed",
  disabled: "referrals are disabled",
};

const KINDS = { report: "report", forward: "forward", return: "return" };

/** A refusal reason in words; unknown reasons are de-snake-cased. */
export const reasonText = (reason) => REASONS[reason] ?? String(reason ?? "refused").replace(/_/g, " ");

/**
 * The declared link a referral from `from` to `to` travelled:
 * `{from, to, kind: "referral"|"topology", reversed}`, or null when the
 * package declares no link between the two desks (an unlinked attempt).
 * A referral link is preferred over a topology link, and a link in the
 * travelled direction over its reverse (a `return` usually runs backwards).
 */
export function edgeFor(structure, from, to) {
  const links = [
    ...(structure.referrals ?? []).map((e) => ({ ...e, kind: "referral" })),
    ...(structure.topology ?? []).map((e) => ({ ...e, kind: "topology" })),
  ];
  const ahead = links.find((e) => e.from === from && e.to === to);
  if (ahead) return { from: ahead.from, to: ahead.to, kind: ahead.kind, reversed: false };
  const back = links.find((e) => e.from === to && e.to === from);
  return back ? { from: back.from, to: back.to, kind: back.kind, reversed: true } : null;
}

/** One line describing a referral, for tooltips and the transcript. */
export function describeReferral(ref) {
  const kind = KINDS[ref.kind] ?? ref.kind ?? "referral";
  const route = `${ref.from_desk} → ${ref.to_desk}${ref.target ? ` (${ref.target})` : ""}`;
  const hops = ref.hops != null ? ` · ${ref.hops} hop${ref.hops === 1 ? "" : "s"}` : "";
  return ref.accepted
    ? `${kind} ${route}${hops}`
    : `${kind} ${route} refused: ${reasonText(ref.reason)}${hops}`;
}

/** True when a transcript entry belongs to `desk` (null keeps everything). */
export function entryOnDesk(entry, desk) {
  if (desk == null) return true;
  if (entry.kind === "turn") return entry.line.floor === desk;
  if (entry.kind === "referral") return entry.ref.from_desk === desk || entry.ref.to_desk === desk;
  if (entry.kind === "desk_end") return entry.end.desk === desk;
  return false;
}

/** The desks a transcript can be filtered to: declared desks, then any floor the stream named. */
export function deskTabs(structure, state) {
  const ids = (structure.desks ?? []).map((d) => d.id);
  for (const e of state.entries ?? []) {
    const d = e.kind === "turn" ? e.line.floor : e.kind === "desk_end" ? e.end.desk : null;
    if (d && !ids.includes(d)) ids.push(d);
  }
  return ids;
}

/** How many transcript entries each desk has, for the tab labels. */
export function deskCounts(entries, desks) {
  return new Map(desks.map((d) => [d, entries.filter((e) => entryOnDesk(e, d)).length]));
}
