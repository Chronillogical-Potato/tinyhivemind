// Checks the Hive and Compare tabs end to end through a running headless
// Chromium (see probe.mjs): opens each scenario, presses Run, waits for the
// ending banner (or the compare's "Done"), and saves a full-page screenshot.
// For math-swarm it also checks desk routing (a badge and an ending row per
// desk, accepted and refused referral rows), shoots the end of the run and
// one desk's transcript tab.
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
