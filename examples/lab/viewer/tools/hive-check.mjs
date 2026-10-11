// Checks the Hive and Compare tabs end to end through a running headless
// Chromium (see probe.mjs): opens each scenario, presses Run, waits for the
// ending banner (or the compare's "Done"), and saves a full-page screenshot.
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

await hive("review-board");
await hive("math-swarm", async () => {
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

ws.close();
process.exit(failures ? 1 : 0);
