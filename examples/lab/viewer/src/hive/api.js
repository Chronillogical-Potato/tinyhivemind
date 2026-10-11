// The Hive tab's view of the dev server: listing, structure, and the two
// streams. See server/hive-api.js for the routes.
import { openStream } from "./stream.js";

async function json(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error((await res.text()) || `HTTP ${res.status}`);
  return res.json();
}

/**
 * `{ packages: [{name, id, title, seats, desks, summary, kind, error?}],
 * live: {available, model} }`: whether the server can run live (it holds an
 * OPENROUTER_API_KEY; the key itself never leaves it) and its default model.
 */
export async function listHives() {
  const body = await json("/api/hives");
  return { packages: body.packages, live: { available: body.live_available === true, model: body.live_model_default ?? null } };
}

/** The `--json` structure of one package. */
export const getHive = (name) => json(`/api/hives/${encodeURIComponent(name)}`);

const query = (o) => new URLSearchParams(Object.entries(o).filter(([, v]) => v != null && v !== "")).toString();

/** Stream one run (`knobs` may carry `live`, `model`, `max_calls`); `onEvent("frame"|"done"|"error"|"status", data)`. */
export const runHive = (name, knobs, onEvent, signal) =>
  openStream(`/api/hives/${encodeURIComponent(name)}/run?${query(knobs)}`, onEvent, signal);

/** Stream a compare; `onEvent("seed"|"summary"|"done"|"error"|"status", data)`. */
export const runCompare = (packages, seeds, onEvent, signal) =>
  openStream(`/api/compare?${query({ packages: packages.join(","), seeds })}`, onEvent, signal);

const recPath = (id) => `/api/recordings/${encodeURIComponent(id)}`;

/** Recorded runs on disk: `[{id, package, problem, seed, started, frames, round, desk, calls, live, active, ended, ending, ...}]`. */
export const listRecordings = async () => (await json("/api/recordings")).recordings;

/** A recording's `--json` structure (its `structure.json`, or its package's). */
export const getRecordingStructure = (id) => json(`${recPath(id)}/structure`);

/** Stream a recording's frames, then its appended ones; `onEvent("status"|"frame"|"done", data)`. */
export const openRecording = (id, onEvent, signal) => openStream(`${recPath(id)}/frames`, onEvent, signal);

/**
 * Call `onList(recordings)` with the listing now and whenever it changes,
 * reconnecting after a dropped stream until `signal` aborts.
 */
export async function watchRecordings(onList, signal) {
  while (!signal?.aborted) {
    try {
      await openStream("/api/recordings/events", (event, data) => { if (event === "recordings") onList(data.recordings ?? []); }, signal);
    } catch (err) {
      if (err.name === "AbortError") return;
    }
    await new Promise((r) => setTimeout(r, 3000));
  }
}
