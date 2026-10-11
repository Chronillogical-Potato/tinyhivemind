// The player's controls: package and knobs for a fresh run, then Play/Pause,
// Step, speed and a scrubber that replay the buffered frames.
import { h } from "./dom.js";

const SPEEDS = [0.5, 1, 2, 4, 8];

function num(id, label, min, max, placeholder) {
  return h("label", {}, `${label} `, h("input", { id, type: "number", min, max, step: 1, placeholder, inputmode: "numeric", class: "hv-num" }));
}

/**
 * Build the controls. `on` = { pick(name), run(knobs), toggle(), step(by),
 * seek(n), speed(x) }. Returns { el, knobs(), setPackages(list, current),
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
  const where = h("span", { class: "muted hv-where", "aria-live": "polite" }, "");
  const msg = h("p", { class: "muted", role: "status" }, "");
  const el = h("section", { class: "card", "aria-label": "Hive controls" },
    h("div", { class: "bar" }, h("label", {}, "Package ", pkg), num("hv-seed", "Seed", 0, 4294967296, "1"),
      num("hv-width", "Round width", 1, 64, "package"), num("hv-budget", "Turn budget", 1, 2000, "package"), problemLabel, run),
    h("div", { class: "bar" }, back, play, fwd, h("label", {}, "Speed ", speed), scrub, where),
    msg);
  const val = (id) => el.querySelector(`#${id}`).value.trim();

  function knobs() {
    return { seed: val("hv-seed"), round_width: val("hv-width"), turn_budget: val("hv-budget"), problem: problemLabel.hidden ? "" : problem.value };
  }

  return {
    el,
    knobs,
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
    update(state) {
      play.textContent = state.playing ? "Pause" : "Play";
      play.setAttribute("aria-pressed", String(state.playing));
      scrub.max = String(state.total);
      scrub.value = String(state.step);
      const tail = state.ended ? "" : " (streaming…)";
      where.textContent = `Round ${state.step} / ${state.total}${tail}`;
    },
    status(text, err) {
      msg.textContent = text;
      msg.className = err ? "err" : "muted";
    },
  };
}
