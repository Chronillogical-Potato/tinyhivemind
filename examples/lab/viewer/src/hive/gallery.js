// The landing page of the Hive tab: one card per package on disk, with what
// it is, how big it is, and a one-click Run.
import { h } from "./dom.js";

/** Render cards for `packages`; `onRun(name)` opens one, `onCompare()` the compare view. */
export function renderGallery(host, packages, onRun, onCompare) {
  if (!packages.length) {
    host.replaceChildren(h("p", { class: "muted" }, "No hive packages found. Set HIVE_PACKAGES, or add packages under crates/tinyhivemind-lang/examples/hives/."));
    return;
  }
  const math = packages.filter((p) => p.kind === "math").length;
  const cards = packages.map((p) =>
    h("article", { class: "hv-card", "data-name": p.name },
      h("h3", {}, p.title),
      h("p", { class: "muted hv-meta" }, `${p.name} · ${p.seats} seats · ${p.desks} desks${p.kind === "math" ? " · math" : ""}`),
      h("p", { class: "hv-summary", title: p.summary || null }, p.error ? h("span", { class: "up" }, p.error) : p.summary || "No summary."),
      h("button", { type: "button", class: "primary", onclick: () => onRun(p.name), disabled: p.error ? true : null }, "Run")));
  host.replaceChildren(
    h("div", { class: "bar hv-gallery-head" },
      h("span", { class: "muted" }, `${packages.length} packages. Run one to watch the room deliberate round by round.`),
      math >= 1 ? h("button", { type: "button", onclick: onCompare }, "Compare math packages") : null),
    h("div", { class: "hv-cards" }, ...cards));
}
