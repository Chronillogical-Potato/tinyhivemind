# DeepSWE OpenHuman upstream-main rerun

**Date:** 2026-09-20

**Status:** Recorded

**Code:** `deepswe_hive` with OpenHuman
`b8eeec0b8d1254a516a9f06477823b74ee7ed62c`, default model
`openai/gpt-oss-120b:nitro`, 24 committed-turn cap

**Spec:** [issue 57](https://github.com/tinyhumansai/tinyhivemind/issues/57)

## Question

Does updating the OpenHuman dependency to its 2026-09-20 `upstream/main`
revision improve the three-task Docker-isolated smoke, and what fraction of
provider input tokens are served from cache?

## Method

This reran the same three tasks, base commits, model, public baselines, task
images, and hidden verifiers as the 2026-09-18 smoke. Each checkout was freshly
extracted from its task image and verified clean at its exact base commit.
Focused public baseline tests passed inside the derived agent image before the
run.

Agent actions ran as the checkout owner in ephemeral Docker containers with
network mode `none`; provider HTTP remained in the host process. Agents had no
browser, search, host shell, credentials, reference solution, dataset, or
hidden-verifier mount. Canonical grading ran later in separate networkless
containers using the unaltered official task images.

OpenHuman `b8eeec0` requires an embedded agent's explicit definition to also
be present in its config-backed catalogue during hosted turn preparation. The
runner now registers that equivalent catalogue entry. The same revision also
sanitizes hosted provider failures, so an error without a retained retryability
classification fails closed rather than being guessed retryable. All 45 runner
tests passed before the paid rerun.

## Results

| task | wall time | runner termination | patch | F2P | P2P | partial diagnostic | reward |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| `abs-module-cache-flags` | 10:44.98 | implementer emitted zero actions after 3 attempts | 0 B | 0/20 | 3/3 | 0.1304 | **0** |
| `abs-stepped-slices` | 1:14.47 | tester emitted zero actions after 3 attempts | 0 B | 0/6 | 6/6 | 0.5000 | **0** |
| `actionlint-action-pinning-lint` | 0:59.22 | episode exceeded 24 turns | 216 B | 0/55 | 0/145 | 0.0000 | **0** |

**Smoke score: 0/3 (0%).** The partial column is grader diagnostics, not an
alternative DeepSWE score. The third patch added only a three-line placeholder
file, and the hidden suite did not build; the other two runs produced no patch.

Provider-reported usage was:

| task | usage records | input tokens | cached input subset | cache hit rate | output tokens | cost |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `abs-module-cache-flags` | 318 | 3,059,862 | 1,814,784 | 59.31% | 66,524 | $2.638765 |
| `abs-stepped-slices` | 188 | 2,147,908 | 882,176 | 41.07% | 31,752 | $2.269064 |
| `actionlint-action-pinning-lint` | 280 | 3,269,594 | 1,596,672 | 48.83% | 52,342 | $3.141449 |
| **total** | **786** | **8,477,364** | **4,293,632** | **50.65%** | **150,618** | **$8.049278** |

Every usage-record id was unique. Cache hit rate is
`cached_input_tokens / input_tokens`; cached input is a subset of input, not
additional usage. The aggregate rate was 50.6482%, about 0.42 percentage points
higher than the original smoke's 50.23%.

## Observation

The dependency update did not improve canonical correctness. Agents spent more
input tokens than in the first smoke, but two rounds failed the native-action
protocol and the only captured patch was a placeholder. One Task 1 action also
ran a recursive grep outside `/workspace` within the container image until the
existing 540-second command timeout stopped it. The container still had no
network or host/verifier mount and was removed by cleanup.
