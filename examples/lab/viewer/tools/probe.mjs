// Drives a running headless Chromium over the DevTools protocol: loads a URL,
// waits, prints console messages and page exceptions, and optionally saves a
// full-page screenshot. Usage:
//   chromium --headless --remote-debugging-port=9333 &
//   node tools/probe.mjs http://localhost:8099/ [shot.png] [waitMs]
import fs from "node:fs";

const [url, shot, wait = "6000"] = process.argv.slice(2);
const port = process.env.CDP_PORT ?? "9333";
const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = targets.find((t) => t.type === "page");
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));

let id = 0;
const pending = new Map();
const send = (method, params = {}) =>
  new Promise((resolve) => {
    pending.set(++id, resolve);
    ws.send(JSON.stringify({ id, method, params }));
  });
ws.addEventListener("message", (m) => {
  const msg = JSON.parse(m.data);
  if (msg.id && pending.has(msg.id)) pending.get(msg.id)(msg.result);
  else if (msg.method === "Runtime.consoleAPICalled") console.log("console", msg.params.type, msg.params.args.map((a) => a.value ?? a.description).join(" "));
  else if (msg.method === "Runtime.exceptionThrown") console.log("EXCEPTION", msg.params.exceptionDetails.exception?.description ?? msg.params.exceptionDetails.text);
});

await send("Runtime.enable");
await send("Page.enable");
await send("Emulation.setDeviceMetricsOverride", { width: 1280, height: 900, deviceScaleFactor: 1, mobile: false });
await send("Page.navigate", { url });
await new Promise((r) => setTimeout(r, Number(wait)));
const probe = await send("Runtime.evaluate", {
  returnByValue: true,
  expression: `JSON.stringify({status: document.getElementById('status')?.textContent, pressed: [...document.querySelectorAll('[aria-pressed=true]')].length, runs: [...document.querySelectorAll('#runs h2')].map(h=>h.textContent), url: location.href})`,
});
console.log(probe.result.value);
if (shot) {
  const metrics = await send("Page.getLayoutMetrics");
  const { width, height } = metrics.cssContentSize;
  const img = await send("Page.captureScreenshot", { format: "png", clip: { x: 0, y: 0, width, height: Math.min(height, 6000), scale: 1 } });
  fs.writeFileSync(shot, Buffer.from(img.data, "base64"));
}
ws.close();
