# tinyhivemind-tools

This crate renders the core episode vocabulary as native `tinytools::ToolSpec`
definitions and records calls through `EpisodeTools`. A host registers an open
turn, gives its seat the returned definitions, calls `EpisodeTools::call` when a
tool is invoked, and drains accepted events or refusals after the turn.

The record opens no socket and awaits nothing. It depends on
[`tinyhivemind-core`](../tinyhivemind-core/README.md) for the speech vocabulary
and on `tinytools` for the host-facing specification type. OpenHuman wraps
these definitions in executable tools; other hosts may bind them themselves.

| Path | Purpose |
| --- | --- |
| `src/render/` | Native specs, schema rendering, and argument parsing. |
| `src/tools/` | Per-seat turn gate and accepted or refused call records. |
