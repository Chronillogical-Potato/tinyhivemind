// The runs panel: lists the traces the dev server finds on disk, loads the ones
// you pick, compares arms of one task, and follows runs that are still being
// written. Falls back to nothing on a static build, where there is no /api.

const MODE_ORDER = ["single", "hive"];

function el(tag, attrs = {}, ...kids) {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") n.className = v;
    else if (k.startsWith("on")) n.addEventListener(k.slice(2), v);
    else n.setAttribute(k, v);
  }
  n.append(...kids);
  return n;
}

const kTok = (t) => (t == null ? "" : t >= 1000 ? `${(t / 1000).toFixed(1)}k tok` : `${t} tok`);
const secs = (ms) => (ms == null ? "" : `${(ms / 1000).toFixed(1)}s`);
const label = (r) => (r.arm ?? r.mode ? `${r.task} · ${r.arm ?? r.mode}` : r.task);

function verdict(r) {
  if (r.reward == null) return { text: "–", cls: "muted" };
  return r.reward >= 1 ? { text: "pass", cls: "down" } : { text: "fail", cls: "up" };
}

/** Group the catalog into rows: one per (tag, task) with a cell per mode. */
export function rows(catalog) {
  const byKey = new Map();
  for (const r of catalog) {
    const key = `${r.tag}\u0000${r.task}`;
    if (!byKey.has(key)) byKey.set(key, { tag: r.tag, task: r.task, cells: new Map(), mtime: 0 });
    const row = byKey.get(key);
    row.cells.set(r.arm ?? r.mode ?? "run", r);
    row.mtime = Math.max(row.mtime, r.mtime);
  }
  return [...byKey.values()].sort((a, b) => b.mtime - a.mtime || a.task.localeCompare(b.task));
}

/** Modes of a row in comparison order: single first, then hive, then the rest. */
export function orderedModes(row) {
  const modes = [...row.cells.keys()];
  return [...MODE_ORDER.filter((m) => modes.includes(m)), ...modes.filter((m) => !MODE_ORDER.includes(m)).sort()];
}

export function initPanel(api) {
  const host = document.getElementById("runs-panel");
  const loaded = new Map(); // id -> {mtime, size} as last fetched
  let catalog = [];
  let timer;

  const byId = (id) => catalog.find((r) => r.id === id);

  function syncUrl() {
    const q = new URLSearchParams();
    for (const id of loaded.keys()) q.append("run", id);
    history.replaceState(null, "", q.toString() ? `?${q}` : location.pathname);
  }

  async function fetchRun(r) {
    const res = await fetch(`/api/runs/${encodeURIComponent(r.id)}`);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return { name: r.path.split("/").pop(), text: await res.text(), label: label(r), source: r.id };
  }

  async function load(r) {
    try {
      await api.ingest([await fetchRun(r)]);
      loaded.set(r.id, { mtime: r.mtime, size: r.size });
    } catch (err) {
      api.status(`Could not load ${r.path}: ${err.message}`, true);
    }
  }

  function unload(id) {
    api.remove(id);
    loaded.delete(id);
  }

  async function toggle(r) {
    if (loaded.has(r.id)) unload(r.id);
    else await load(r);
    syncUrl();
    render();
  }

  async function compare(row) {
    for (const id of [...loaded.keys()]) unload(id);
    const modes = orderedModes(row);
    for (const m of modes) await load(row.cells.get(m));
    const names = modes.map((m) => label(row.cells.get(m)));
    api.select(names[0], names[1]);
    syncUrl();
    render();
  }

  function cell(row, mode) {
    const r = row.cells.get(mode);
    if (!r) return el("td", { class: "l muted" }, "");
    const v = verdict(r);
    const on = loaded.has(r.id);
    const bits = [kTok(r.tokens), secs(r.wall_ms), r.turns != null ? `${r.turns} turns` : ""].filter(Boolean).join(" · ");
    const btn = el(
      "button",
      { type: "button", "aria-pressed": String(on), title: r.path, onclick: () => toggle(r) },
      on ? "● " : "○ ",
      el("span", { class: v.cls }, v.text),
      bits ? ` ${bits}` : "",
    );
    return el("td", { class: "l" }, btn);
  }

  function render() {
    if (!catalog.length) {
      host.replaceChildren(
        el("h2", {}, "Runs"),
        el("p", { class: "muted" }, "No traces found. Start the server with HIVE_RUNS=/path/to/jobs (colon-separated), or drop files below."),
      );
      return;
    }
    const all = rows(catalog);
    const modes = [...new Set(all.flatMap(orderedModes))];
    const head = el("tr", {}, el("th", { scope: "col", class: "l" }, "Task"), ...modes.map((m) => el("th", { scope: "col", class: "l" }, m)), el("th", { scope: "col" }, ""));
    const body = el("tbody");
    let tag = null;
    for (const row of all) {
      if (row.tag !== tag) {
        tag = row.tag;
        body.append(el("tr", {}, el("th", { scope: "rowgroup", colspan: String(modes.length + 2), class: "l muted" }, tag)));
      }
      const can = row.cells.size >= 2;
      body.append(
        el("tr", {}, el("td", { class: "l" }, row.task), ...modes.map((m) => cell(row, m)),
          el("td", {}, can ? el("button", { type: "button", onclick: () => compare(row) }, "Compare") : "")),
      );
    }
    host.replaceChildren(
      el("h2", {}, "Runs"),
      el("p", { class: "muted" }, `${catalog.length} traces on disk. Click a cell to load it, or Compare to load every arm of a task. New runs appear as they are written.`),
      el("div", { class: "scroll", tabindex: "0", role: "region", "aria-label": "Runs on disk" }, el("table", {}, el("thead", {}, head), body)),
    );
  }

  // Re-read any loaded trace that grew since it was fetched (a live run).
  async function followLoaded() {
    for (const [id, seen] of [...loaded]) {
      const r = byId(id);
      if (!r || (r.size === seen.size && r.mtime === seen.mtime)) continue;
      try {
        api.replace(id, await fetchRun(r));
        loaded.set(id, { mtime: r.mtime, size: r.size });
      } catch {
        // The file may be mid-write; the next change event retries.
      }
    }
  }

  async function refreshCatalog() {
    const res = await fetch("/api/runs");
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    catalog = (await res.json()).runs;
    await followLoaded();
    render();
  }

  async function restore() {
    const q = new URLSearchParams(location.search);
    const ids = q.getAll("run");
    if (ids.length) {
      for (const id of ids) {
        const r = byId(id);
        if (r) await load(r);
      }
    } else if (!q.getAll("file").length && catalog.length) {
      const first = rows(catalog).find((r) => r.cells.size >= 2) ?? rows(catalog)[0];
      if (first) return first.cells.size >= 2 ? compare(first) : toggle(first.cells.values().next().value);
    }
    for (const url of q.getAll("file")) {
      try {
        const res = await fetch(url);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        await api.ingest([{ name: url.split("/").pop() || url, text: await res.text() }]);
      } catch (err) {
        api.status(`Could not fetch ${url}: ${err.message}`, true);
      }
    }
    syncUrl();
    render();
  }

  refreshCatalog()
    .then(restore)
    .then(() => {
      const events = new EventSource("/api/events");
      events.addEventListener("runs", () => {
        clearTimeout(timer);
        timer = setTimeout(() => refreshCatalog().catch(() => {}), 250);
      });
    })
    .catch(() => {
      host.hidden = true; // static build: no /api, drag and drop only
    });

  return {
    cleared() {
      loaded.clear();
      syncUrl();
      render();
    },
  };
}
