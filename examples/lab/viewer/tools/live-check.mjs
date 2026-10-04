// Checks the live path end to end through a running headless Chromium (see
// probe.mjs): open the viewer, write a new trace into the runs dir, and report
// whether the runs panel picked it up without a reload.
// Usage: node tools/live-check.mjs http://localhost:8099/ <runs-dir>
import fs from "node:fs";
import path from "node:path";

const [url, runs] = process.argv.slice(2);
const port = process.env.CDP_PORT ?? "9333";
const page = (await (await fetch(`http://127.0.0.1:${port}/json`)).json()).find((t) => t.type === "page");
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));
let id = 0;
const pending = new Map();
ws.addEventListener("message", (m) => {
  const msg = JSON.parse(m.data);
  if (msg.id && pending.has(msg.id)) pending.get(msg.id)(msg.result);
});
const send = (method, params = {}) => new Promise((resolve) => { pending.set(++id, resolve); ws.send(JSON.stringify({ id, method, params })); });
const text = async () => (await send("Runtime.evaluate", { returnByValue: true, expression: "document.querySelector('#runs-panel p')?.textContent" })).result.value;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

await send("Page.navigate", { url });
await sleep(4000);
const before = await text();
const dir = path.join(runs, "live-check");
fs.mkdirSync(dir, { recursive: true });
fs.writeFileSync(path.join(dir, "new.jsonl"), '{"run":"live","seq":0,"at_ms":0,"event":"idle"}\n');
await sleep(3000);
const after = await text();
fs.rmSync(dir, { recursive: true });
console.log("before:", before);
console.log("after: ", after);
ws.close();
