# Jev decision evaluation

- **Date:** 2026-09-17
- **Status:** Recorded
- **Code:** `cargo run --release -p tinyhivemind-hive --example bench -- --decision-eval --episodes 1000 --jobs 32`
- **Spec:** [`../specs/jev-integration.md`](../specs/jev-integration.md)
- **Decision:** keep Jev optional and thresholded; do not replace higher-accuracy evidence scoring without a domain-specific calibration pass

## Setup

One thousand paired cases were sent to `openai/gpt-5-mini` through OpenRouter
strict JSON Schema and to TypeSafe Jev through `tinyjevclient`. Every pair used
identical structured state and equivalent questions:

- Choice for responder routing;
- Choice for the worker output's consensus stance, including abstention;
- Score for evidence quality;
- Noul for an explicit safety or approval violation.

The six labeled cases repeat deterministically across the sample. Request order
alternated A/B then B/A. The outer benchmark width was 32; Jev was capped at
four in flight after a preliminary width-32 probe returned authentication
failures. Prices were snapshotted at $0.25/M input and $2/M output for the
baseline, and $0.04/M input and $0/M output for Jev.

## Results

| Metric Name | LLM Baseline | Jev-Hybrid | Δ Speedup / Savings |
| --- | ---: | ---: | ---: |
| decision latency p50 | 17582.47 ms | 357.65 ms | 49.16x |
| decision latency p90 | 23786.19 ms | 431.80 ms | 55.09x |
| decision latency p99 | 34377.72 ms | 802.98 ms | 42.81x |
| peak bounded ops/sec | 1.72 | 10.62 | 6.17x |
| input tokens/case | 556.9440 | 526.9510 | 5.4% |
| output tokens/case | 1248.8920 | 121.4820 | 90.3% |
| attempts/case | 0.9990 | 0.9990 | 0.0% |
| estimated USD/case | $0.00263702 | $0.00002108 | 99.2% |
| primitive accuracy | 95.22% | 83.77% | -11.45 pp |
| Choice Brier | 0.0461 | 0.0450 | 2.3% |
| Noul Brier | 0.0068 | 0.0190 | -180.1% |
| Score MAE | 0.2090 | 0.4957 | -137.2% |
| schema/provider failure rate | 0.10% | 0.20% | +0.10 pp |

The baseline had one schema failure; Jev had two. TypeSafe reports output-token
usage even though the configured output-token price is zero, so tokens and cost
remain separate rows.

## Diagnostic analysis

Jev's main error was evidence strength. It repeatedly placed direct evidence
near the middle Score level and weak evidence near the unsupported level. That
explains most of the 11.45-point accuracy gap and the larger Score MAE. It also
occasionally abstained from a stance on the deliberately unsafe output while
still identifying its policy violation. Routing itself was substantially more
stable after routing and stance were separated into independent Choice
questions.

The baseline's errors were mostly evidence-level misses, occasional routing of
an `other` case to the planner, and one false-positive violation. Jev's Choice
Brier score was slightly better despite lower exact accuracy, while its Noul
Brier score was worse; the model was generally directionally right but less
well calibrated on the violation probability in this small repeated corpus.

The initial uncapped concurrency-32 probe is excluded. Sequential and width-4
Jev runs had no provider failures, while width 32 returned authentication
failures for the production account. The shipped harness therefore caps Jev at
four and reports throughput at that bound rather than treating overload zeros
as model measurements.

## Limits

This is a repeated six-case decision corpus, not 1,000 independent tasks. It
supports latency, cost, wire reliability, and behavior comparisons for these
questions; it does not establish general model quality. It measures the control
plane decisions, not complete live hive episodes with generated worker turns.
The latter remains a separate live-scenario experiment before the draft
integration PR can be marked ready.
