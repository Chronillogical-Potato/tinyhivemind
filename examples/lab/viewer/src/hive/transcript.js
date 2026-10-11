// The rolling transcript. The current round's lines are marked; selecting a
// seat highlights its latest turn and the lines that turn saw (`saw` of `of`,
// see `sawBy` in simulation.js), so you can read what one seat was handed.
// A multi-desk package gets a tab per desk (All first); referral rows and
// desk endings sit between the turns, in the order they streamed.
import { h } from "./dom.js";
import { deskCounts, deskTabs, describeReferral, entryOnDesk } from "./referrals.js";
import { sawBy } from "./simulation.js";

const MAX_LINES = 400;

function tabs(structure, state, desk, onDesk) {
  const desks = deskTabs(structure, state);
  if (desks.length < 2) return null;
  const counts = deskCounts(state.entries, desks);
  const tab = (id, label, n) => h("button", { type: "button", role: "tab", class: "hv-tab", "aria-selected": String(desk === id),
    "data-desk": id ?? "", onclick: () => onDesk(id) }, label, h("span", { class: "muted" }, ` ${n}`));
  return h("div", { class: "hv-tabs", role: "tablist", "aria-label": "Transcript desk" },
    tab(null, "All", state.entries.length), ...desks.map((d) => tab(d, d, counts.get(d))));
}

function referralRow(ref, desk) {
  const dir = desk == null ? "" : ref.to_desk === desk ? "in" : "out";
  return h("li", { class: `hv-ref ${ref.accepted ? "accepted" : "refused"}`, title: ref.content ?? "" },
    h("span", { class: "hv-ref-tag" }, ref.accepted ? `${ref.kind ?? "referral"}${dir ? ` ${dir}` : ""}` : "refused"),
    ` ${describeReferral(ref)}${ref.seat ? ` · from ${ref.seat}` : ""}`,
    ref.content ? h("div", { class: "hv-content muted" }, ref.content) : null);
}

function deskEndRow(end, answerDesk) {
  const on = end.topic != null ? ` on “${end.topic}”` : "";
  const verdict = end.correct == null ? "" : end.correct ? " · correct" : " · wrong";
  return h("li", { class: `hv-desk-end ${end.ending}${end.desk === answerDesk ? " answer" : ""}` },
    `Desk ${end.desk} ${end.ending}${on} after ${end.rounds ?? "?"} rounds, ${end.turns ?? "?"} turns${verdict}${end.desk === answerDesk ? " · the hive's answer" : ""}`);
}

/**
 * Render the transcript for `state`; clicking a line selects its seat.
 * `opts = { structure, desk, onDesk(deskOrNull) }` drives the desk tabs.
 */
export function renderTranscript(host, state, selected, onSelect, opts = {}) {
  const desk = opts.desk ?? null;
  const view = selected ? sawBy(state, selected) : null;
  const note = view
    ? h("p", { class: "hv-saw" }, `${selected}'s round ${view.turn.round} turn saw ${view.turn.saw} of ${view.turn.of} lines (${view.turn.visibility}); the highlighted lines are what it saw.`)
    : selected
      ? h("p", { class: "muted" }, `${selected} has not spoken yet.`)
      : h("p", { class: "muted" }, "Click a seat or a line to see what that seat saw.");
  const entries = (state.entries ?? state.lines.map((line) => ({ kind: "turn", line }))).filter((e) => entryOnDesk(e, desk));
  const from = Math.max(0, entries.length - MAX_LINES);
  const list = h("ol", { class: "hv-lines" });
  let round = null;
  for (const e of entries.slice(from)) {
    if (e.kind === "referral") {
      list.append(referralRow(e.ref, desk));
      continue;
    }
    if (e.kind === "desk_end") {
      list.append(deskEndRow(e.end, state.ending?.desk ?? null));
      continue;
    }
    const l = e.line;
    if (l.round !== round) {
      round = l.round;
      const floor = l.floorTagged ? ` · ${l.floor}${l.deskRound != null ? ` #${l.deskRound}` : ""}` : "";
      list.append(h("li", { class: "hv-round" }, `Round ${l.round}${floor} · ${l.phase ?? ""} · ${l.visibility ?? ""}`));
    }
    const cls = [
      "hv-line",
      l.round === state.round?.round ? "now" : "",
      view?.seen.has(l.index) ? "seen" : "",
      view?.turn.index === l.index ? "own" : "",
    ].filter(Boolean).join(" ");
    const verdict = l.correct == null ? null : h("span", { class: l.correct ? "down" : "up" }, l.correct ? " correct" : " wrong");
    list.append(h("li", { class: cls },
      h("button", { type: "button", class: "hv-who", onclick: () => onSelect(l.seat), title: `${l.desk} · ${l.reason} · saw ${l.saw}/${l.of}` }, l.seat),
      h("span", { class: "muted" }, ` ${l.reason} · ${l.saw}/${l.of} `),
      h("span", { class: "hv-content" }, l.content ?? ""), verdict));
  }
  if (!entries.length) list.append(h("li", { class: "muted" }, desk ? `Nothing on ${desk} yet.` : "Nobody has spoken yet."));
  host.replaceChildren(...[tabs(opts.structure ?? {}, state, desk, opts.onDesk ?? (() => {})), note, list].filter(Boolean));
  // Scroll the panel (positioned, so it is the offset parent), never the page.
  const own = list.querySelector(".own");
  host.scrollTop = own ? own.offsetTop - 60 : host.scrollHeight;
}
