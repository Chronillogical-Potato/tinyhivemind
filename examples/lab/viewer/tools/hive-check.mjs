// Checks the Hive and Compare tabs end to end through a running headless
// Chromium (see probe.mjs): opens each scenario, presses Run, waits for the
// ending banner (or the compare's "Done"), and saves a full-page screenshot.
// For math-swarm it also checks desk routing (a badge and an ending row per
// desk, accepted and refused referral rows), shoots the end of the run and
// one desk's transcript tab. The fixture recording (public/fixtures/recordings)
// opens through ?recording=<id> and must show its tool-call and memory badges,
// seat counters and a folded long turn. With FOLLOW_LIVE=1 it also waits for a
// live run started elsewhere (e.g. `hive_view ... --run --live`) to appear in
// the Live now strip, follows it, and shoots it mid-run and at its ending.
// Exits non-zero if a scenario times out or the page throws.
// Usage:
//   chrome --headless=new --remote-debugging-port=9333 --user-data-dir=<dir> &
//   node tools/hive-check.mjs http://localhost:8099/ [shots-dir] [timeoutMs]
import fs from "node:fs";
import path from "node:path";

const [base = "http://localhost:8099/", out = "shots", timeout = "180000"] = process.argv.slice(2);
const port = process.env.CDP_PORT ?? "9333";
const page = (await (await fetch(`http://127.0.0.1:${port}/json`)).json()).find((t) => t.type === "page");
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));

let id = 0;
let failures = 0;
const pending = new Map();
const send = (method, params = {}) =>
  new Promise((resolve) => {
    pending.set(++id, resolve);
    ws.send(JSON.stringify({ id, method, params }));
  });
ws.addEventListener("message", (m) => {
  const msg = JSON.parse(m.data);
  if (msg.id && pending.has(msg.id)) pending.get(msg.id)(msg.result);
  else if (msg.method === "Runtime.consoleAPICalled" && msg.params.type === "error") console.log("console error", msg.params.args.map((a) => a.value ?? a.description).join(" "));
  else if (msg.method === "Runtime.exceptionThrown") {
    failures++;
    console.log("EXCEPTION", msg.params.exceptionDetails.exception?.description ?? msg.params.exceptionDetails.text);
  }
});
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const evaluate = async (expression) => (await send("Runtime.evaluate", { returnByValue: true, awaitPromise: true, expression })).result.value;

async function waitFor(expression, ms = Number(timeout)) {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    if (await evaluate(expression)) return true;
    await sleep(250);
  }
  return false;
}

async function shot(file) {
  const { cssContentSize } = await send("Page.getLayoutMetrics");
  const img = await send("Page.captureScreenshot", {
    format: "png",
    clip: { x: 0, y: 0, width: cssContentSize.width, height: Math.min(cssContentSize.height, 6000), scale: 1 },
    captureBeyondViewport: true,
  });
  fs.mkdirSync(out, { recursive: true });
  fs.writeFileSync(path.join(out, file), Buffer.from(img.data, "base64"));
  console.log("saved", path.join(out, file));
}

async function open(query) {
  await send("Page.navigate", { url: new URL(query, base).href });
  await sleep(500);
}

async function hive(name, extra) {
  await open(`?hive=${name}`);
  if (!(await waitFor("!!document.querySelector('#hv-run') && !document.getElementById('hv-player').hidden", 30000))) {
    failures++;
    return console.log(`FAIL ${name}: player did not open`);
  }
  await evaluate("document.getElementById('hv-run').click()");
  await evaluate("(() => { const s = document.querySelector('[aria-label=Speed]'); s.value = '8'; s.dispatchEvent(new Event('change')); })()");
  const ok = await waitFor("!!document.getElementById('hv-ending')");
  const facts = await evaluate(`JSON.stringify({ ending: document.getElementById('hv-ending')?.textContent, seats: document.querySelectorAll('.hv-seat').length,
    desks: document.querySelectorAll('.hv-desk').length, lines: document.querySelectorAll('.hv-line').length, status: document.querySelector('#hv-player [role=status]')?.textContent })`);
  console.log(ok ? "ok" : "FAIL", name, facts);
  if (!ok) failures++;
  if (extra) await extra();
  await shot(`${name}.png`);
}

await send("Runtime.enable");
await send("Page.enable");
await send("Emulation.setDeviceMetricsOverride", { width: 1280, height: 900, deviceScaleFactor: 1, mobile: false });
// SCHEME=light|dark forces the colour scheme; unset follows the browser.
if (process.env.SCHEME) await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: process.env.SCHEME }] });

await open("?view=hive");
const cards = await waitFor("document.querySelectorAll('.hv-card').length > 0", 60000);
console.log(cards ? "ok" : "FAIL", "gallery", await evaluate("document.querySelectorAll('.hv-card').length"), "cards");
if (!cards) failures++;
const recs = await waitFor("document.querySelectorAll('#hv-recordings .hv-rec').length > 0", 15000);
console.log(recs ? "ok" : "FAIL", "recorded runs", await evaluate("document.querySelectorAll('#hv-recordings .hv-rec').length"), "listed");
if (!recs) failures++;
await shot("gallery.png");

await hive("review-board");
await hive("math-swarm", async () => {
  // Desks route: the stream must have carried referrals (accepted and refused) and one ending per desk.
  const routed = JSON.parse(await evaluate(`JSON.stringify({ badges: document.querySelectorAll('.hv-desk-badge').length,
    desks: document.querySelectorAll('.hv-desk').length, accepted: document.querySelectorAll('.hv-ref.accepted').length,
    refused: document.querySelectorAll('.hv-ref.refused').length, deskEnds: document.querySelectorAll('.hv-desk-end').length,
    answer: document.querySelectorAll('.hv-desk.answer').length, standings: document.getElementById('h-standings')?.textContent })`));
  const ok = routed.badges === routed.desks && routed.desks === 5 && routed.deskEnds === 5 && routed.accepted >= 1 && routed.refused >= 1 && routed.answer === 1;
  console.log(ok ? "ok" : "FAIL", "math-swarm routing", JSON.stringify(routed));
  if (!ok) failures++;
  await shot("math-swarm-end.png");
  // One desk's transcript tab: only that desk's turns, referrals and ending.
  const desk = await evaluate("(() => { const t = document.querySelector('.hv-tab[data-desk=verify]') ?? document.querySelectorAll('.hv-tab')[1]; t?.click(); return t?.dataset.desk ?? null; })()");
  await sleep(200);
  const tab = JSON.parse(await evaluate(`JSON.stringify({ selected: document.querySelector('.hv-tab[aria-selected=true]')?.dataset.desk,
    rows: document.querySelectorAll('#hv-transcript li.hv-line, #hv-transcript li.hv-ref').length,
    others: [...document.querySelectorAll('#hv-transcript li.hv-round')].filter((r) => !r.textContent.includes(${JSON.stringify(` · ${desk}`)})).length })`));
  const tabOk = desk && tab.selected === desk && tab.rows > 0 && tab.others === 0;
  console.log(tabOk ? "ok" : "FAIL", `math-swarm ${desk} tab`, JSON.stringify(tab));
  if (!tabOk) failures++;
  await shot(`math-swarm-${desk ?? "desk"}-tab.png`);
  await evaluate("document.querySelector('.hv-tab[data-desk=\"\"]')?.click()");
  // Step back to the middle and select a seat, so the shot shows a bubble and the transcript highlight.
  await evaluate("(() => { const r = document.querySelector('.hv-scrub'); r.value = String(Math.ceil(Number(r.max) / 2)); r.dispatchEvent(new Event('input')); })()");
  await sleep(200);
  await evaluate("document.querySelectorAll('.hv-seat')[23]?.dispatchEvent(new MouseEvent('click', { bubbles: true }))");
  await sleep(300);
});

// A recording: the fixture, found by package in the pushed listing.
async function recording() {
  const id = await evaluate("fetch('/api/recordings').then((r) => r.json()).then((d) => d.recordings.find((r) => r.package === 'coin-panel')?.id ?? null)");
  if (!id) {
    failures++;
    return console.log("FAIL recording: the coin-panel fixture is not listed");
  }
  await open(`?recording=${encodeURIComponent(id)}`);
  await waitFor("!document.getElementById('hv-player').hidden", 15000);
  await evaluate("(() => { const s = document.querySelector('[aria-label=Speed]'); s.value = '8'; s.dispatchEvent(new Event('change')); })()");
  const ok = await waitFor("!!document.getElementById('hv-ending')", 30000);
  const facts = JSON.parse(await evaluate(`JSON.stringify({ url: location.search, tools: document.querySelectorAll('.hv-tool').length,
    failed: document.querySelectorAll('.hv-tool.fail').length, memory: document.querySelectorAll('.hv-mem').length,
    folds: document.querySelectorAll('.hv-fold').length, counters: document.querySelectorAll('.hv-act').length,
    picker: !document.querySelector('.hv-rec-bar').hidden, ending: document.getElementById('hv-ending')?.textContent })`));
  const good = ok && facts.url.includes("recording=") && facts.tools >= 5 && facts.failed >= 1 && facts.memory >= 5 && facts.folds >= 1 && facts.counters >= 4 && facts.picker;
  console.log(good ? "ok" : "FAIL", "recording", JSON.stringify(facts));
  if (!good) failures++;
  await evaluate("document.querySelector('.hv-fold')?.click()");
  await sleep(150);
  const open_ = await evaluate("document.querySelector('.hv-fold')?.getAttribute('aria-expanded')");
  console.log(open_ === "true" ? "ok" : "FAIL", "recording fold expands");
  if (open_ !== "true") failures++;
  await shot("recording.png");
}
await recording();

// Follow a live run started outside this page.
if (process.env.FOLLOW_LIVE) {
  await open("?view=hive");
  const seen = await waitFor("!!document.querySelector('#hv-live-strip .hv-live-go')", Number(process.env.FOLLOW_WAIT_MS ?? 120000));
  console.log(seen ? "ok" : "FAIL", "live run in the strip", await evaluate("document.getElementById('hv-live-strip').textContent"));
  if (!seen) failures++;
  else {
    await shot("live-strip.png");
    // Nothing was open, so the page may already follow it; click to be sure.
    await evaluate("document.querySelector('#hv-live-strip .hv-live-go').click()");
    await waitFor("!document.getElementById('hv-player').hidden", 15000);
    const first = await waitFor("document.querySelectorAll('.hv-line').length > 0", 600000);
    const r1 = await evaluate("document.querySelector('.hv-where').textContent");
    await waitFor(`document.querySelector('.hv-where').textContent !== ${JSON.stringify(r1)}`, 600000);
    const r2 = JSON.parse(await evaluate("JSON.stringify({ where: document.querySelector('.hv-where').textContent, live: !document.getElementById('hv-live-badge').hidden, status: document.querySelector('#hv-player [role=status]').textContent })"));
    console.log(first && r2.where !== r1 ? "ok" : "FAIL", "live follow advances", r1, "->", JSON.stringify(r2));
    if (!first || r2.where === r1) failures++;
    await shot("live-mid.png");
    const ended = await waitFor("!!document.getElementById('hv-ending')", Number(process.env.FOLLOW_END_MS ?? 1800000));
    console.log(ended ? "ok" : "FAIL", "live run ended", await evaluate("document.getElementById('hv-ending')?.textContent ?? ''"));
    if (!ended) failures++;
    await shot("live-end.png");
  }
}

await open("?compare=math-panel,math-swarm&seeds=10");
const done = await waitFor("/^Done/.test(document.getElementById('hv-compare-status')?.textContent ?? '')");
console.log(done ? "ok" : "FAIL", "compare", await evaluate("JSON.stringify({ status: document.getElementById('hv-compare-status').textContent, cells: document.querySelectorAll('.hv-cell').length })"));
if (!done) failures++;
await shot("compare.png");

// A per-seed cell opens that seed in the player and runs it.
await evaluate("document.querySelector('.hv-cell.open:last-of-type')?.dispatchEvent(new MouseEvent('click', { bubbles: true }))");
const replay = await waitFor("!!document.getElementById('hv-ending') && !document.getElementById('view-hive').hidden");
console.log(replay ? "ok" : "FAIL", "replay from compare", await evaluate("location.search"));
if (!replay) failures++;

ws.close();
process.exit(failures ? 1 : 0);
