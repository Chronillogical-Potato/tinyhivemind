# euler-hive

A live, problem-agnostic hive for hard mathematical problems: fourteen persona
seats on five desks run as a pipeline — research, theory, compute, verify,
synthesis — joined by referrals, with web search only on research seats,
Python only on compute and verify seats, and a CortexDB memory bound to every
seat. The committed `context/problem.md` is a placeholder; a host replaces it
with the statement it is handed, so the package names no problem and carries
none of a problem's data.

`examples/openhuman --bin euler_hive` is the host that runs it live
(OpenRouter models, TinyHumans SDK search, a Docker sandbox); see
[its README](../../../../../examples/openhuman/src/bin/euler_hive/README.md).
`tests/euler_hive_package.rs` pins the package's shape and gating.

| Desk | Seats | Tools (besides the hive's own) | Job |
| --- | --- | --- | --- |
| `research` | librarian, provenance skeptic, algorithmic scout | `web_search` | Find the theory and literature; report sources, never a copied answer |
| `theory` | combinatorial-design geometer, linear-algebra and coding theorist, counting auditor | none | Identify the structure and hand compute a checkable plan |
| `compute` | exhaustive-search engineer, GF(2) programmer, brute-force minimalist | `run_python` | Exact programs, run in the sandbox, outputs verbatim |
| `verify` | constraint auditor, independent re-deriver, adversarial uniqueness tester | `run_python` | Re-derive every candidate, reproduce every sample value the statement gives, check each constraint |
| `synthesis` | evidence-weighing judge, dissent recorder | none | Commit the answer once verify signs off |

Every seat also holds the hive's own tools: `hive_broadcast` (a desk
colleague), `hive_refer` (a linked desk), `hive_complete` (finish with a
report, an optional `candidate` and a `verdict`), `hive_remember` and
`hive_recall`. Every seat denies `shell`, file access, `web_fetch`, `browser`,
`http`, `glob`, `grep` and `git`, and every seat off the research desk denies
`web_search` too.

**Topology** is the pipeline `research -> theory -> compute -> verify ->
synthesis`. **Referrals** add the way back: theory to research, compute to
theory, verify to compute and theory, synthesis to verify. Referral is enabled
(`reach: desks`, `returns: true`, `max_hops: 6`), rounds are at most three
seats wide, and the constitution caps a run at 80 turns.

**Memory.** The root is `team:euler-hive`. Each seat binds its own agent
identity at `team:euler-hive/agent:<seat>` and reads it and the shared
`team:euler-hive/project:verified` pool, recalls at session start, rejoin and
compaction within 2,500 characters, and may remember observations, failed
attempts, outcomes and notes. Every identity has `lifecycle: run`, so a host
scopes a run's memory to that run.

| Path | Purpose |
| --- | --- |
| `hive.json` | Five desks, fourteen seats with per-seat tool access, goals, missions, topology, referrals, policies |
| `constitution.json` | `max_turns` 80, `max_width` 3, the two hard constraints on web and file access |
| `memory.json` | The root, one agent identity per seat, the verified pool, and fourteen bindings |
| `roles/` | One charter per role |
| `seats/` | Fourteen prompts, each opening with a distinct `You are a "<persona>".` |
| `context/problem.md` | The placeholder a host replaces with the problem statement |
