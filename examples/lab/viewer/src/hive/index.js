// The Hive and Compare tabs: tab switching, URL state, and the wiring from the
// dev server's streams into the player and its panels. Everything it draws
// lives in the sibling modules; this file only decides what to draw when.
// Recordings (runs written to hive-runs/ by any host) are pushed from the
// server as they appear and grow; one opens in the same player, following
// its tail while the run is still going.
import "./hive.css";
import { newlyLive } from "./activity.js";
import { getHive, getRecordingStructure, listHives, openRecording, runCompare, runHive, watchRecordings } from "./api.js";
import { deskEndings, endingBanner, usageLine } from "./banner.js";
import { renderCompare } from "./compare.js";
import { createControls } from "./controls.js";
import { h } from "./dom.js";
import { renderGallery } from "./gallery.js";
import { renderGraph } from "./graph.js";
import { createPlayer } from "./player.js";
import { renderLiveStrip, renderRecordings, toast } from "./recordings.js";
import { deskPolicy } from "./referrals.js";
import { renderStandings } from "./standings.js";
import { renderTranscript } from "./transcript.js";

const $ = (id) => document.getElementById(id);
const VIEWS = ["runs", "hive", "compare"];
const KNOB_KEYS = ["seed", "round_width", "turn_budget", "problem"];
const DEFAULT_COMPARE = ["math-panel", "math-swarm"];

let packages = null; // listing, once fetched
let listing = null; // the one in-flight or settled listing request
const structures = new Map();
let current = null; // { name, structure, recording: id | null }
let recordings = null; // the pushed listing, once it arrives
let showAllRecordings = false;
let recAbort = null;
let runStartedAt = 0;
let opening = null; // a package being opened (its structure is loading): no auto-follow then
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
  if (view === "hive" && current?.recording) q.set("recording", current.recording);
  else if (view === "hive" && current) {
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
  pickRecording: (id) => openRecordingView(id),
  rewind: () => current?.recording && openRecordingView(current.recording, { fromStart: true }),
});

function showHive() {
  const on = !!current;
  $("hv-gallery").hidden = on;
  $("hv-recordings").hidden = on;
  $("hv-player").hidden = !on;
  if (!on) drawRecordings();
  if (!on && packages.length) renderGallery($("hv-gallery"), packages, (name) => openHive(name, {}, true), () => setView("compare"));
}

async function openHive(name, knobs, autorun) {
  opening = name;
  recAbort?.abort();
  try {
    if (!structures.has(name)) structures.set(name, await getHive(name));
  } catch (err) {
    opening = null;
    current = null;
    showHive();
    $("hv-gallery").prepend(h("p", { class: "err" }, `Could not open ${name}: ${err.message}`));
    return;
  }
  if (opening !== name) return; // something else was opened meanwhile
  opening = null;
  runAbort?.abort();
  recAbort?.abort();
  current = { name, structure: structures.get(name), recording: null };
  controls.setMode("run");
  controls.setRecordingLive(false);
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
  runStartedAt = Date.now();
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
    if (runAbort === ctl) {
      player.finish();
      runAbort = null;
    }
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
  // Standings are the current round's floor; on a per-desk run that desk's quorum applies.
  const policy = deskPolicy(st, (st.desks ?? []).length > 1 ? state.round?.desk ?? null : null);
  $("h-standings").textContent = policy.name ? `Quorum standings · ${policy.name}` : "Quorum standings";
  renderStandings($("hv-standings"), state.standings, policy.episode.quorum_threshold ?? 0, p?.answer ?? null);
  renderTranscript($("hv-transcript"), state, selected, select, { structure: st, desk: deskFilter, onDesk: filterDesk });
}

// ---------- recordings ----------
const followRecording = (id) => openRecordingView(id);
const replayRecording = (id) => openRecordingView(id, { fromStart: true });

function drawRecordings() {
  if (recordings == null) return;
  renderRecordings($("hv-recordings"), recordings, replayRecording, showAllRecordings, () => { showAllRecordings = true; drawRecordings(); });
}

function drawLiveStrip() {
  if (recordings) renderLiveStrip($("hv-live-strip"), recordings, { follow: followRecording, fromStart: replayRecording }, current?.recording ?? null);
}

/** A run this page started itself shows up as a recording too; it needs no toast. */
const ownRun = (rec) => !!runAbort && current && !current.recording && rec.package === current.name && rec.started >= runStartedAt - 5000;

function onRecordings(list) {
  const fresh = newlyLive(recordings, list).filter((r) => !ownRun(r));
  recordings = list;
  drawLiveStrip();
  if (!$("hv-recordings").hidden) drawRecordings();
  if (current?.recording) controls.setRecordings(list, current.recording);
  // Nothing open on the Hive tab: follow the newest run that just started,
  // and announce the others (or all of them, when something is open).
  const follow = fresh.length && document.body.dataset.view === "hive" && !current && !opening ? fresh[0] : null;
  for (const rec of fresh) if (rec !== follow) toast($("hv-toasts"), rec, () => followRecording(rec.id));
  if (follow) followRecording(follow.id);
}

/**
 * Open recording `id` in the player. A run still being written is followed
 * at its newest round unless `fromStart`, which replays it from round one
 * (Latest then resumes following); a finished one replays from the start.
 */
async function openRecordingView(id, { fromStart = false } = {}) {
  runAbort?.abort();
  recAbort?.abort();
  opening = null;
  const ctl = new AbortController();
  recAbort = ctl;
  let structure;
  try {
    structure = await getRecordingStructure(id);
  } catch (err) {
    if (ctl.signal.aborted) return;
    current = null;
    showHive();
    $("hv-recordings").prepend(h("p", { class: "err" }, `Could not open recording ${id}: ${err.message}`));
    return;
  }
  if (ctl.signal.aborted) return;
  const rec = recordings?.find((r) => r.id === id);
  current = { name: rec?.package ?? structure.id ?? id, structure, recording: id };
  selected = null;
  deskFilter = null;
  controls.setMode("recording");
  controls.setRecordings(recordings ?? [], id);
  controls.setRecordingLive(false);
  $("hv-title").textContent = `${structure.name ?? current.name} · recorded run`;
  $("hv-summary").textContent = structure.summary ?? "";
  $("hv-summary").title = structure.summary ?? "";
  player.reset(false);
  controls.status("Loading the recording…");
  if (document.body.dataset.view !== "hive") setView("hive", false);
  showHive();
  drawLiveStrip();
  writeUrl();
  // Frames are folded once per animation frame, so a recording of thousands
  // of frames loads in a few redraws rather than one per frame.
  let buffer = [];
  let pending = false;
  let following = false;
  const flush = () => {
    pending = false;
    if (ctl.signal.aborted || !buffer.length) return;
    const batch = buffer;
    buffer = [];
    player.pushMany(batch);
    if (following) {
      const st = player.state();
      controls.status(`Following live: round ${st.received}${st.round?.desk ? ` on ${st.round.desk}` : ""}. New rounds appear as the run writes them.`);
    }
  };
  try {
    await openRecording(id, (event, data) => {
      if (ctl.signal.aborted) return;
      if (event === "status") {
        const open = !!data.active && !data.ended;
        following = open && !fromStart;
        player.reset(true, following);
        controls.setRecordingLive(open);
        controls.status(open ? (following ? "Following live…" : "Replaying from the start; Latest jumps to the newest round.") : "Replaying the recording.");
      } else if (event === "frame" && data && typeof data === "object") {
        buffer.push(data);
        if (!pending) {
          pending = true;
          requestAnimationFrame(flush);
        }
      } else if (event === "done") {
        flush();
        controls.status(data?.ended ? `Recording complete: ${player.state().received} rounds.` : `The run stopped without an ending after ${player.state().received} rounds.`);
      }
    }, ctl.signal);
  } catch (err) {
    if (err.name !== "AbortError" && !ctl.signal.aborted) controls.status(`Recording failed: ${err.message}`, true);
  } finally {
    if (!ctl.signal.aborted) {
      flush();
      player.finish();
      controls.setRecordingLive(false);
    }
  }
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
  recAbort?.abort();
  controls.setMode("run");
  controls.setRecordingLive(false);
  player.pause();
  current = null;
  writeUrl();
  showHive();
  drawLiveStrip();
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

watchRecordings(onRecordings);
setInterval(drawLiveStrip, 5000); // elapsed times tick between listings

const q = new URLSearchParams(location.search);
if (q.get("recording")) {
  setView("hive", false);
  openRecordingView(q.get("recording"));
} else if (q.get("compare") != null) {
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
