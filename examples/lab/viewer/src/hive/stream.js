// Reads a server-sent-event response with fetch rather than EventSource, so a
// 400 shows its message and a finished stream is not reconnected. The parser
// is pure and unit-tested; `openStream` is the browser half.

/** Incremental SSE parser: `push(text)` returns the complete events `{event, data}`. */
export function sseParser() {
  let buf = "";
  return {
    push(text) {
      buf += text.replace(/\r\n/g, "\n");
      const out = [];
      let cut;
      while ((cut = buf.indexOf("\n\n")) >= 0) {
        const block = buf.slice(0, cut);
        buf = buf.slice(cut + 2);
        let event = "message";
        const data = [];
        for (const line of block.split("\n")) {
          if (line.startsWith("event:")) event = line.slice(6).trim();
          else if (line.startsWith("data:")) data.push(line.slice(5).replace(/^ /, ""));
        }
        if (data.length) out.push({ event, data: data.join("\n") });
      }
      return out;
    },
  };
}

/**
 * GET `url` and call `onEvent(event, parsedData)` per event. Resolves when the
 * stream closes; rejects with the body text on a non-2xx. `signal` aborts.
 */
export async function openStream(url, onEvent, signal) {
  const res = await fetch(url, { signal });
  if (!res.ok) throw new Error((await res.text()) || `HTTP ${res.status}`);
  const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
  const parser = sseParser();
  for (;;) {
    const { value, done } = await reader.read();
    if (done) return;
    for (const { event, data } of parser.push(value)) {
      let parsed = data;
      try {
        parsed = JSON.parse(data);
      } catch {
        // keep the raw text
      }
      onEvent(event, parsed);
    }
  }
}
