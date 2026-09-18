# DeepSWE OpenHuman smoke

**Date:** 2026-09-18

**Status:** Recorded

**Code:** `deepswe_hive` on `issue-57-deepswe-eval`, default model
`openai/gpt-oss-120b:nitro`, 24 committed-turn cap

**Spec:** [issue 57](https://github.com/tinyhumansai/tinyhivemind/issues/57)

**Decision:** Do not treat the local acceptance fixture as benchmark evidence;
fix bounded-run finalization before expanding the DeepSWE slice.

## Question

Can the OpenHuman-backed four-seat hive produce canonically correct patches on
a small real DeepSWE v1.1 slice while every agent action remains inside an
offline Docker workspace?

## Corpus and slice

The gated official dataset could not be fetched without Hugging Face
credentials. This run used the public third-party materialization
`luolc/deep-swe-1-1-materialized`, which identifies its source as
`datacurve-ai/deep-swe` commit
`435ee89ec2f2e2289f33b0da4f992f0b7b7266b9`. The materialization contains 113
tasks. Its parquet SHA-256 was verified as
`954184ffb6fd88c798171dfc8577793b29631bff35c8d02f6fa4bbf88a44abd0`.

This was a three-task smoke, not a score over the full 113-task benchmark. The
slice was the first three task IDs in lexical order, spanning two upstream
repositories:

| task | repository | base commit |
| --- | --- | --- |
| `abs-module-cache-flags` | `abs-lang/abs` | `cb1b3b671d0ee9fa9da9f7b02f86967953ffd10a` |
| `abs-stepped-slices` | `abs-lang/abs` | `cb1b3b671d0ee9fa9da9f7b02f86967953ffd10a` |
| `actionlint-action-pinning-lint` | `rhysd/actionlint` | `0bdc95715fa58f64e3fd6e63b0f89be8733cbbab` |

Each checkout was extracted from its task image and verified at the stated
commit with no tracked, untracked, ignored, or gitlink entries. Focused public
baseline tests passed before the run.

## Isolation

Agent actions ran as the checkout owner in ephemeral containers with Docker
network mode `none`, a cleared environment, all capabilities dropped,
`no-new-privileges`, bounded memory/CPU/PIDs, and `.git` masked. Agents received
only repository read/write/edit/shell/test MCP tools plus the two hive actions.
They had no browser, web search, fetch, host shell, credentials, dataset,
reference solution, or hidden-verifier mount. Provider HTTP ran in the host
process, outside the agent containers.

The task images stored their preloaded Go modules below `/root/go`. Derived
agent images changed only directory/module-cache read permissions so the
unprivileged checkout owner could run tests offline. Canonical grading used the
unaltered task images in fresh, networkless containers.

## Results

The official task reward is binary. All three captured patches were empty, and
all three canonical graders returned zero:

| task | wall time | runner termination | patch | F2P | P2P | partial diagnostic | reward |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| `abs-module-cache-flags` | 2:21.79 | next round of 4 exceeded cap | 0 B | 0/20 | 3/3 | 0.1304 | **0** |
| `abs-stepped-slices` | 8:40.23 | next round of 3 exceeded cap | 0 B | 0/6 | 6/6 | 0.5000 | **0** |
| `actionlint-action-pinning-lint` | 1:05.79 | next round of 3 exceeded cap | 0 B | 0/55 | 145/145 | 0.7250 | **0** |

**Smoke score: 0/3 (0%).** The partial column is grader diagnostics, not the
DeepSWE reward and not an alternative score.

The runner rejected a pending round when it would cross the 24-turn cap. It
exited before serializing `result.json` or the committed-turn counter, so an
exact per-task committed-turn count is unavailable. Raw evidence establishes
only that each run stopped below or at 24 turns before its next three- or
four-seat round. Archived OpenHuman session snapshots numbered 29, 35, and 29;
these include protocol retries and must not be reported as committed turns.

The local one-task acceptance fixture passed 1/1 during runner validation. It
is a synthetic, visible-test fixture and is not benchmark evidence; the real
smoke result above supersedes it for any claim about DeepSWE performance.

## What happened

The seats repeatedly announced completion without making source changes. The
reviewer and tester sometimes reported success despite finding no implementation
or seeing network-blocked dependency attempts. The completion protocol did not
reach quorum before the cap, and the strict round-width check then terminated
the run. This is a useful failure: the harness prevented unbounded calls and
the canonical hidden tests rejected the empty work.

One Task 2 shell command recursively searched the task image until its
600-second timeout. Because the Docker action timeout and model-turn timeout
were identical, the MCP process could be stopped before cleanup and left one
exited container. It was removed, and the runner now uses nested 540-second
command and 570-second action deadlines inside the unchanged 600-second turn
deadline. The final Docker audit found no `deepswe-*` containers.

## Consequences

Before a larger slice, the runner should write a terminal result at budget
exhaustion, including committed turns, retry counts, and the captured patch,
rather than exiting before serialization. The agent protocol also needs a
grounded-progress check: completion claims without a non-empty diff or relevant
test evidence should not advance the episode. Neither change alters this
recorded score.
