# lab/src/bin

One binary per area of core. A directory binary (`name/main.rs`) is
auto-discovered, so adding one needs no manifest edit. Each keeps its files
under 700 lines by splitting along the topic it prints.

| Path | What it is |
| --- | --- |
| `hello_trace.rs` | the smallest traced run: a few events as JSONL on stdout |
| `swe_hive.rs` | the SWE hive runner, see [`../swe/README.md`](../swe/README.md) |
| `memory_hive/` | digest, pins, sharing, briefing, elsewhere, threads, the log contract |
| `context_tools/` | the tool surface, asides, projection, masking, mentions |
| `swarm_knobs/` | `EpisodePolicy` sweeps and the mechanisms under the episode |
| `driver_knobs/` | the completion driver, the conductor, routing and `JevRouter` |
| `relay_hive/` | a bug relayed across desks by referral |
| `gate_knobs/` | identity, dispatch, responder and approval gates |

The coverage matrix mapping every knob to its example is in the
[lab README](../../README.md#coverage-matrix).
