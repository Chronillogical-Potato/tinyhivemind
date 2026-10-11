// The rolling transcript. The current round's lines are marked; selecting a
// seat highlights its latest turn and the lines that turn saw (`saw` of `of`,
// see `sawBy` in simulation.js), so you can read what one seat was handed.
import { h } from "./dom.js";
import { sawBy } from "./simulation.js";

const MAX_LINES = 400;

/** Render the transcript for `state`; clicking a line selects its seat. */
export function renderTranscript(host, state, selected, onSelect) {
  const view = selected ? sawBy(state, selected) : null;
  const note = view
    ? h("p", { class: "hv-saw" }, `${selected}'s round ${view.turn.round} turn saw ${view.turn.saw} of ${view.turn.of} lines (${view.turn.visibility}); the highlighted lines are what it saw.`)
    : selected
      ? h("p", { class: "muted" }, `${selected} has not spoken yet.`)
      : h("p", { class: "muted" }, "Click a seat or a line to see what that seat saw.");
  const from = Math.max(0, state.lines.length - MAX_LINES);
  const list = h("ol", { class: "hv-lines", start: String(from + 1) });
  let round = null;
  for (const l of state.lines.slice(from)) {
    if (l.round !== round) {
      round = l.round;
      list.append(h("li", { class: "hv-round" }, `Round ${l.round} · ${l.phase ?? ""} · ${l.visibility ?? ""}`));
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
  if (!state.lines.length) list.append(h("li", { class: "muted" }, "Nobody has spoken yet."));
  host.replaceChildren(note, list);
  // Scroll the panel (positioned, so it is the offset parent), never the page.
  const own = list.querySelector(".own");
  host.scrollTop = own ? own.offsetTop - 60 : host.scrollHeight;
}
