// The recordings panels: the "Live now" strip at the top of the Hive tab
// (every run still being written, anywhere on this machine), the "Recorded
// runs" list on the gallery landing, and the toast that announces a run that
// just started. The listing arrives pushed from /api/recordings/events; the
// pure choices (live or finished, new, labels) are in activity.js.
import { elapsed, finished, liveNow, recordingLabel } from "./activity.js";
import { h } from "./dom.js";

const SHOWN = 12;

const verdict = (r) => (r.correct === true ? " · correct" : r.correct === false ? " · wrong" : "");

/**
 * The strip of live recordings into `host` (hidden when there are none).
 * `on = { follow(id), fromStart(id) }`; `current` is the open recording's id.
 */
export function renderLiveStrip(host, list, on, current, now = Date.now()) {
  const live = liveNow(list);
  host.hidden = !live.length;
  if (!live.length) return host.replaceChildren();
  host.replaceChildren(
    h("span", { class: "hv-live-title" }, "Live now"),
    ...live.map((r) => h("div", { class: `hv-live-item${r.id === current ? " open" : ""}`, "data-id": r.id },
      h("button", { type: "button", class: "hv-live-go", title: "Follow this run from its newest round", onclick: () => on.follow(r.id) },
        h("span", { class: `hv-badge-live${r.live ? "" : " quiet"}` }, r.live ? "LIVE" : "QUIET"),
        ` ${recordingLabel(r)} · ${elapsed(now - r.started)}`),
      h("button", { type: "button", class: "hv-live-rewind", title: "Replay this run from its first round, then follow it", onclick: () => on.fromStart(r.id) }, "⏮"))));
}

/** The gallery's list of recorded runs (live ones first); `onOpen(id)` replays one. */
export function renderRecordings(host, list, onOpen, showAll, onShowAll) {
  if (!list.length) {
    host.replaceChildren(h("h2", {}, "Recorded runs"),
      h("p", { class: "muted" }, "No recordings yet. Live runs, runs started here and hosts that write frames.ndjson under hive-runs/ show up on their own."));
    return;
  }
  const ordered = [...liveNow(list), ...finished(list)];
  const rows = showAll ? ordered : ordered.slice(0, SHOWN);
  host.replaceChildren(
    h("h2", {}, "Recorded runs"),
    h("ul", { class: "hv-recs" }, ...rows.map((r) => {
      const state = r.ended ? `${r.ending ?? "ended"}${verdict(r)}` : r.live ? "live" : r.active ? "quiet" : "stopped";
      return h("li", {},
        h("button", { type: "button", class: "hv-rec", "data-id": r.id, onclick: () => onOpen(r.id), title: r.path },
          r.ended ? null : h("span", { class: `hv-badge-live${r.live ? "" : " quiet"}` }, r.live ? "LIVE" : r.active ? "QUIET" : "STOPPED"),
          h("strong", {}, ` ${r.package}`),
          h("span", { class: "muted" }, ` ${recordingLabel(r).split(" · ").slice(1).join(" · ")} · ${state} · ${new Date(r.started).toLocaleString()}`)));
    })),
    ordered.length > rows.length ? h("button", { type: "button", onclick: onShowAll }, `Show all ${ordered.length}`) : null);
}

/** A toast for a run that just started; `onWatch()` follows it. Dismisses itself. */
export function toast(host, rec, onWatch) {
  const n = h("div", { class: "hv-toast", role: "status", "data-id": rec.id },
    h("span", { class: "hv-badge-live" }, "LIVE"),
    ` ${rec.package} run started${rec.problem?.id ? ` on ${rec.problem.id}` : ""} — `,
    h("button", { type: "button", class: "primary", onclick: () => { n.remove(); onWatch(); } }, "Watch"),
    h("button", { type: "button", "aria-label": "Dismiss", onclick: () => n.remove() }, "×"));
  host.append(n);
  setTimeout(() => n.remove(), 15000);
}
