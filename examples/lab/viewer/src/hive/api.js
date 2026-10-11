// The Hive tab's view of the dev server: listing, structure, and the two
// streams. See server/hive-api.js for the routes.
import { openStream } from "./stream.js";

async function json(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error((await res.text()) || `HTTP ${res.status}`);
  return res.json();
}

/** `[{name, id, title, seats, desks, summary, kind, error?}]`. */
export const listHives = async () => (await json("/api/hives")).packages;

/** The `--json` structure of one package. */
export const getHive = (name) => json(`/api/hives/${encodeURIComponent(name)}`);

const query = (o) => new URLSearchParams(Object.entries(o).filter(([, v]) => v != null && v !== "")).toString();

/** Stream one run; `onEvent("frame"|"done"|"error"|"status", data)`. */
export const runHive = (name, knobs, onEvent, signal) =>
  openStream(`/api/hives/${encodeURIComponent(name)}/run?${query(knobs)}`, onEvent, signal);

/** Stream a compare; `onEvent("seed"|"summary"|"done"|"error"|"status", data)`. */
export const runCompare = (packages, seeds, onEvent, signal) =>
  openStream(`/api/compare?${query({ packages: packages.join(","), seeds })}`, onEvent, signal);
