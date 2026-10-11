// The Hive and Compare tabs: tab switching, URL state, and the wiring from the
// dev server's streams into the player and its panels. Everything it draws
// lives in the sibling modules; this file only decides what to draw when.
import "./hive.css";
import { getHive, listHives, runCompare, runHive } from "./api.js";
import { deskEndings, endingBanner, usageLine } from "./banner.js";
import { renderCompare } from "./compare.js";
import { createControls } from "./controls.js";
import { h } from "./dom.js";
import { renderGallery } from "./gallery.js";
import { renderGraph } from "./graph.js";
import { createPlayer } from "./player.js";
import { renderStandings } from "./standings.js";
import { renderTranscript } from "./transcript.js";

const $ = (id) => document.getElementById(id);
const VIEWS = ["runs", "hive", "compare"];
const KNOB_KEYS = ["seed", "round_width", "turn_budget", "problem"];
const DEFAULT_COMPARE = ["math-panel", "math-swarm"];

let packages = null; // listing, once fetched
let listing = null; // the one in-flight or settled listing request
const structures = new Map();
let current = null; // { name, structure }
let selected = null;
let deskFilter = null; // transcript tab: null = All, else a desk id
let runAbort = null;
let compareAbort = null;
let compareState = { seeds: [], summary: null, requested: DEFAULT_COMPARE, done: false };

// ---------- views and URL ----------
function setView(view, write = true) {
  document.body.dataset.view = view;
  for (const v of VIEWS) {
    $(`view-${v}`).hidden = v !== view;
    $(`tab-${v}`).setAttribute("aria-selected", String(v === view));
  }
  window.dispatchEvent(new CustomEvent("viewchange", { detail: view }));
  if (view !== "runs") ensureListing().then(() => (view === "hive" ? showHive() : showCompareForm()));
  if (write && view !== "runs") writeUrl();
}

function writeUrl() {
  const q = new URLSearchParams();
  const view = document.body.dataset.view;
  if (view === "hive" && current) {
    q.set("hive", current.name);
    for (const [k, v] of Object.entries(controls.knobs())) if (v) q.set(k, v);
  } else if (view === "hive") q.set("view", "hive");
  else if (view === "compare") {
    q.set("compare", compareState.requested.join(","));
    q.set("seeds", $("hv-seeds").value || "20");
  }
  history.replaceState(null, "", `?${q}`);
}

function ensureListing() {
  listing ??= listHives().then(
    ({ packages: list, live }) => {
      controls.setLive(live);
      return (packages = list);
    },
    (err) => {
      packages = [];
      $("hv-gallery").replaceChildren(h("p", { class: "err" }, `No hive API here (${err.message}). Run the viewer with npm run dev.`));
      return packages;
    },
  );
  return listing;
}

// ---------- hive player ----------
const player = createPlayer(draw);
const controls = createControls({
  pick: (name) => openHive(name, {}, false),
  run: () => run(),
  toggle: () => player.toggle(),
  step: (by) => player.step(by),
  seek: (n) => { player.pause(); player.seek(n); },
  speed: (x) => player.setSpeed(x),
  latest: () => player.latest(),
});

function showHive() {
  const on = !!current;
  $("hv-gallery").hidden = on;
  $("hv-player").hidden = !on;
  if (!on && packages.length) renderGallery($("hv-gallery"), packages, (name) => openHive(name, {}, true), () => setView("compare"));
}

async function openHive(name, knobs, autorun) {
  try {
    if (!structures.has(name)) structures.set(name, await getHive(name));
  } catch (err) {
    current = null;
    showHive();
    $("hv-gallery").prepend(h("p", { class: "err" }, `Could not open ${name}: ${err.message}`));
    return;
  }
  runAbort?.abort();
  current = { name, structure: structures.get(name) };
  selected = null;
  deskFilter = null;
  const st = current.structure;
  controls.setPackages(packages ?? [], name);
  controls.setStructure(st);
  controls.setKnobs({ seed: "", round_width: "", turn_budget: "", ...knobs });
  $("hv-title").textContent = st.name ?? name;
  $("hv-summary").textContent = st.summary ?? "";
  $("hv-summary").title = st.summary ?? "";
  player.reset(false);
  controls.status("Press Run to start an episode.");
  if (document.body.dataset.view !== "hive") setView("hive", false);
  showHive();
  writeUrl();
  if (autorun) run();
}

async function run() {
  if (!current) return;
  runAbort?.abort();
  const ctl = new AbortController();
  runAbort = ctl;
  selected = null;
  writeUrl();
  // Live knobs never go in the URL, so a reload cannot start a billed run.
  const live = controls.liveKnobs();
  const isLive = live.live === "1";
  player.reset(true, isLive);
  controls.status(isLive
    ? `Starting a live run on ${live.model}… each round waits on real model calls, so this takes minutes; rounds appear as they finish.`
    : "Starting… the first run builds hive_view, which can take a minute.");
  const k = { ...controls.knobs(), ...live };
  try {
    await runHive(current.name, k, (event, data) => {
      if (event === "frame") {
        player.push(data);
        if (isLive && data?.type === "round") controls.status(`Live: round ${data.round}${data.desk ? ` on ${data.desk}` : ""} arrived.`);
      } else if (event === "error") controls.status(`Run failed: ${data.message}${data.stderr ? `\n${data.stderr}` : ""}`, true);
      else if (event === "done") controls.status(`Episode streamed: ${player.state().received} rounds.`);
      else if (event === "status") controls.status(isLive ? "Running live… waiting on the first round." : "Running…");
    }, ctl.signal);
  } catch (err) {
    if (err.name !== "AbortError") controls.status(`Run failed: ${err.message}`, true);
  } finally {
    if (runAbort === ctl) player.finish();
  }
}

function filterDesk(desk) {
  deskFilter = desk;
  draw(player.state());
}

function select(seat) {
  selected = selected === seat ? null : seat;
  draw(player.state());
}

function draw(state) {
  if (!current) return;
  const st = current.structure;
  controls.update(state);
  $("hv-banner").replaceChildren(...[endingBanner(state), deskEndings(state), usageLine(state.usage)].filter(Boolean));
  const p = state.problem;
  $("hv-problem-text").textContent = p ? `Problem ${p.id}: ${p.statement}` : "";
  renderGraph($("hv-graph"), st, state, { selected, onSelect: select });
  renderStandings($("hv-standings"), state.standings, st.episode?.quorum_threshold ?? 0, p?.answer ?? null);
  renderTranscript($("hv-transcript"), state, selected, select, { structure: st, desk: deskFilter, onDesk: filterDesk });
}

// ---------- compare ----------
function showCompareForm() {
  const host = $("hv-compare-pick");
  const math = packages.filter((p) => p.kind === "math" || compareState.requested.includes(p.name));
  const list = math.length ? math : packages;
  host.replaceChildren(...list.map((p) =>
    h("label", { class: "hv-check" }, h("input", { type: "checkbox", value: p.name, checked: compareState.requested.includes(p.name) || null }), ` ${p.title}`)));
}

async function compare() {
  compareAbort?.abort();
  const ctl = new AbortController();
  compareAbort = ctl;
  const requested = [...$("hv-compare-pick").querySelectorAll("input:checked")].map((i) => i.value);
  if (!requested.length) return compareStatus("Pick at least one package.", true);
  compareState = { seeds: [], summary: null, requested, done: false };
  writeUrl();
  const openable = new Set((packages ?? []).map((p) => p.name));
  const redraw = () => renderCompare($("hv-compare-out"), compareState, (pkg, seed, problem) => openHive(pkg, { seed, problem }, true), openable);
  redraw();
  compareStatus("Running… the first run builds hive_view, which can take a minute.");
  try {
    await runCompare(requested, $("hv-seeds").value || "20", (event, data) => {
      if (event === "seed") compareState.seeds.push(data);
      else if (event === "summary") compareState.summary = data.rows;
      else if (event === "error") compareStatus(`Compare failed: ${data.message}${data.stderr ? `\n${data.stderr}` : ""}`, true);
      else if (event === "done") { compareState.done = true; compareStatus(`Done: ${compareState.seeds.length} seeds.`); }
      if (event === "seed" || event === "summary" || event === "done") redraw();
    }, ctl.signal);
  } catch (err) {
    if (err.name !== "AbortError") compareStatus(`Compare failed: ${err.message}`, true);
  }
}

function compareStatus(text, err) {
  const n = $("hv-compare-status");
  n.textContent = text;
  n.className = err ? "err" : "muted";
}

// ---------- boot ----------
$("hv-summary").after(controls.el);
$("hv-back").addEventListener("click", () => {
  runAbort?.abort();
  player.pause();
  current = null;
  writeUrl();
  showHive();
});
$("hv-compare-run").addEventListener("click", compare);
for (const v of VIEWS) $(`tab-${v}`).addEventListener("click", () => setView(v));
document.addEventListener("keydown", (e) => {
  if (document.body.dataset.view !== "hive" || !current || /INPUT|SELECT|TEXTAREA/.test(e.target.tagName)) return;
  if (e.key === "ArrowRight") player.step(1);
  else if (e.key === "ArrowLeft") player.step(-1);
});
let resizeTimer;
window.addEventListener("resize", () => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(() => current && document.body.dataset.view === "hive" && draw(player.state()), 150);
});

const q = new URLSearchParams(location.search);
if (q.get("compare") != null) {
  compareState.requested = q.get("compare").split(",").filter(Boolean);
  if (q.get("seeds")) $("hv-seeds").value = q.get("seeds");
  setView("compare", false);
  ensureListing().then(() => { showCompareForm(); compare(); });
} else if (q.get("hive")) {
  const knobs = Object.fromEntries(KNOB_KEYS.filter((k) => q.get(k)).map((k) => [k, q.get(k)]));
  setView("hive", false);
  ensureListing().then(() => openHive(q.get("hive"), knobs, true));
} else if (q.get("view") === "hive" || q.get("view") === "compare") {
  setView(q.get("view"), false);
} else {
  setView("runs", false);
}
