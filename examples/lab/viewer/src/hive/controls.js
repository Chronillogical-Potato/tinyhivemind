// The player's controls: package and knobs for a fresh run, the live
// (OpenRouter) toggle with its model and call cap, then Play/Pause, Step,
// speed, a scrubber that replays the buffered frames, and Latest, which
// resumes following a stream after scrubbing back. A recording swaps the run
// knobs for a recording picker and a LIVE badge while its tail is followed.
import { recordingLabel } from "./activity.js";
import { h } from "./dom.js";

const SPEEDS = [0.5, 1, 2, 4, 8];
/** Shown until the server reports its own default. */
export const LIVE_MODEL_FALLBACK = "openai/gpt-oss-120b:nitro";
export const LIVE_MAX_CALLS = 300;

function num(id, label, min, max, placeholder) {
  return h("label", {}, `${label} `, h("input", { id, type: "number", min, max, step: 1, placeholder, inputmode: "numeric", class: "hv-num" }));
}

/**
 * Build the controls. `on` = { pick(name), run(knobs), toggle(), step(by),
 * seek(n), speed(x), latest() }. Returns { el, knobs(), liveKnobs(),
 * setLive({available, model}), setPackages(list, current),
 * setStructure(structure), setKnobs(knobs), update(state), status(msg, err) }.
 */
export function createControls(on) {
  const pkg = h("select", { id: "hv-pkg", "aria-label": "Package", onchange: (e) => on.pick(e.target.value) });
  const problem = h("select", { id: "hv-problem", "aria-label": "Problem" });
  const problemLabel = h("label", { hidden: true }, "Problem ", problem);
  const run = h("button", { type: "button", id: "hv-run", class: "primary", onclick: () => on.run(knobs()) }, "Run");
  const play = h("button", { type: "button", id: "hv-play", onclick: () => on.toggle() }, "Play");
  const back = h("button", { type: "button", "aria-label": "Step back", onclick: () => on.step(-1) }, "◀ Step");
  const fwd = h("button", { type: "button", "aria-label": "Step forward", onclick: () => on.step(1) }, "Step ▶");
  const speed = h("select", { "aria-label": "Speed", onchange: (e) => on.speed(Number(e.target.value)) },
    ...SPEEDS.map((x) => h("option", { value: x, selected: x === 1 }, `${x}×`)));
  const scrub = h("input", { type: "range", min: 0, max: 0, value: 0, step: 1, "aria-label": "Round", class: "hv-scrub", oninput: (e) => on.seek(Number(e.target.value)) });
  const latest = h("button", { type: "button", id: "hv-latest", hidden: true, title: "Jump to the newest round and keep following the stream", onclick: () => on.latest() }, "Latest ⇥");
  const where = h("span", { class: "muted hv-where", "aria-live": "polite" }, "");
  const msg = h("p", { class: "muted", role: "status" }, "");
  const live = h("input", { type: "checkbox", id: "hv-live", onchange: () => syncLive() });
  const model = h("input", { type: "text", id: "hv-model", class: "hv-model", spellcheck: "false", autocomplete: "off", maxlength: 131,
    placeholder: LIVE_MODEL_FALLBACK, "aria-label": "Model" });
  const calls = h("input", { id: "hv-max-calls", type: "number", min: 1, max: LIVE_MAX_CALLS, step: 1, value: LIVE_MAX_CALLS, inputmode: "numeric", class: "hv-num", "aria-label": "Max calls" });
  const liveNote = h("span", { class: "muted hv-live-note" }, "");
  const liveBar = h("div", { class: "bar hv-live" },
    h("label", { class: "hv-check", title: "Seats are real model calls through OpenRouter: slow (minutes) and billed" }, live, " Live (OpenRouter)"),
    h("label", {}, "Model ", model), h("label", {}, "Max calls ", calls), liveNote);
  const recPick = h("select", { id: "hv-rec-pick", "aria-label": "Recording", onchange: (e) => on.pickRecording?.(e.target.value) });
  const liveBadge = h("span", { class: "hv-badge-live", id: "hv-live-badge", hidden: true }, "LIVE");
  const rewind = h("button", { type: "button", id: "hv-rec-rewind", title: "Replay from the first round, then follow", onclick: () => on.rewind?.() }, "⏮ From start");
  const recBar = h("div", { class: "bar hv-rec-bar", hidden: true }, h("label", {}, "Recording ", recPick), liveBadge, rewind);
  const runBar = h("div", { class: "bar" }, h("label", {}, "Package ", pkg), num("hv-seed", "Seed", 0, 4294967296, "1"),
    num("hv-width", "Round width", 1, 64, "package"), num("hv-budget", "Turn budget", 1, 2000, "package"), problemLabel, run);
  const el = h("section", { class: "card", "aria-label": "Hive controls" },
    recBar, runBar,
    liveBar,
    h("div", { class: "bar" }, back, play, fwd, h("label", {}, "Speed ", speed), scrub, where, latest),
    msg);
  let available = false;

  function syncLive() {
    live.disabled = !available;
    if (!available) live.checked = false;
    model.disabled = calls.disabled = !live.checked;
    liveNote.textContent = available
      ? live.checked ? `Real model calls, billed to the server's key; capped at ${calls.value || LIVE_MAX_CALLS} calls. Rounds stream as they finish.` : ""
      : "Live runs are unavailable: the dev server has no OPENROUTER_API_KEY in its environment.";
  }
  calls.addEventListener("input", syncLive);
  syncLive();
  const val = (id) => el.querySelector(`#${id}`).value.trim();

  function knobs() {
    return { seed: val("hv-seed"), round_width: val("hv-width"), turn_budget: val("hv-budget"), problem: problemLabel.hidden ? "" : problem.value };
  }

  /** The live knobs for a run request; empty (offline) unless the toggle is on. */
  function liveKnobs() {
    if (!available || !live.checked) return {};
    return { live: "1", model: model.value.trim() || model.placeholder, max_calls: calls.value.trim() };
  }

  return {
    el,
    knobs,
    liveKnobs,
    setLive({ available: on, model: def }) {
      available = !!on;
      model.placeholder = def || LIVE_MODEL_FALLBACK;
      syncLive();
    },
    setPackages(list, current) {
      pkg.replaceChildren(...list.map((p) => h("option", { value: p.name, selected: p.name === current }, `${p.title} (${p.seats} seats)`)));
    },
    setStructure(structure) {
      const problems = structure?.sim?.kind === "math" ? structure.sim.problems ?? [] : [];
      problemLabel.hidden = !problems.length;
      problem.replaceChildren(...problems.map((p) => h("option", { value: p.id, title: p.statement }, p.id)));
      el.querySelector("#hv-width").placeholder = String(structure?.episode?.round_width ?? "");
      el.querySelector("#hv-budget").placeholder = String(structure?.episode?.turn_budget ?? "");
    },
    setKnobs(k) {
      if (k.seed != null) el.querySelector("#hv-seed").value = k.seed;
      if (k.round_width != null) el.querySelector("#hv-width").value = k.round_width;
      if (k.turn_budget != null) el.querySelector("#hv-budget").value = k.turn_budget;
      if (k.problem) problem.value = k.problem;
    },
    /** `"run"` shows the package knobs; `"recording"` the recording picker. */
    setMode(mode) {
      const rec = mode === "recording";
      recBar.hidden = !rec;
      runBar.hidden = liveBar.hidden = rec;
    },
    setRecordings(list, current) {
      const known = list.some((r) => r.id === current);
      const rows = known ? list : [{ id: current, package: current }, ...list];
      recPick.replaceChildren(...rows.map((r) => h("option", { value: r.id, selected: r.id === current },
        `${r.ended ? "" : r.live ? "● " : "○ "}${recordingLabel(r)}${r.ended ? ` · ${r.ending ?? "ended"}` : ""}`)));
    },
    /** The LIVE badge: on while a recording's tail is open and being followed or buffered. */
    setLive(on) {
      liveBadge.hidden = !on;
    },
    update(state) {
      play.textContent = state.playing ? "Pause" : "Play";
      play.setAttribute("aria-pressed", String(state.playing));
      scrub.max = String(state.total);
      scrub.value = String(state.step);
      const tail = state.ended ? "" : state.following ? " (following the stream…)" : " (streaming…)";
      where.textContent = `Round ${state.step} / ${state.total}${tail}`;
      latest.hidden = !state.streaming || state.following;
    },
    status(text, err) {
      msg.textContent = text;
      msg.className = err ? "err" : "muted";
    },
  };
}
