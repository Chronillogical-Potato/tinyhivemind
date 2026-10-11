// Tiny DOM and SVG builders shared by the Hive tab, in the style of main.js.
const SVGNS = "http://www.w3.org/2000/svg";

/** An HTML element: `on*` attributes become listeners, null children are skipped. */
export function h(tag, attrs = {}, ...kids) {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v == null || v === false) continue;
    if (k === "class") n.className = v;
    else if (k.startsWith("on")) n.addEventListener(k.slice(2), v);
    else n.setAttribute(k, v === true ? "" : v);
  }
  for (const c of kids.flat()) if (c != null) n.append(c);
  return n;
}

/** An SVG element with optional text content. */
export function s(tag, attrs = {}, text) {
  const n = document.createElementNS(SVGNS, tag);
  for (const [k, v] of Object.entries(attrs)) if (v != null) n.setAttribute(k, v);
  if (text != null) n.textContent = text;
  return n;
}

/** A sized, labelled SVG root. */
export function svgRoot(width, height, label) {
  return s("svg", { width, height, viewBox: `0 0 ${width} ${height}`, role: "img", "aria-label": label });
}

/** Truncate to `n` characters with an ellipsis. */
export const clip = (text, n) => (text.length > n ? `${text.slice(0, n - 1)}…` : text);

/** Greedy word wrap into at most `lines` lines of `width` characters. */
export function wrap(text, width, lines) {
  const out = [];
  let cur = "";
  for (const word of String(text).split(/\s+/).filter(Boolean)) {
    if (!cur) cur = word;
    else if (cur.length + 1 + word.length <= width) cur += ` ${word}`;
    else {
      out.push(cur);
      cur = word;
    }
    if (out.length === lines) break;
  }
  if (cur && out.length < lines) out.push(cur);
  if (out.length === lines && out.join(" ").length < String(text).trim().length) out[lines - 1] = clip(`${out[lines - 1]}…`, width);
  return out.map((l) => clip(l, width));
}
