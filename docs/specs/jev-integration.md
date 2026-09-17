# Typed System One decisions

- **Status:** Implemented
- **Owners:** `tinyhivemind-core`, `tinyhivemind`, `tinyhivemind-hive`, and the benchmark host

## Behavior

Responder selection consumes a complete fixed-point probability distribution,
not generated text. The selection must cover every candidate exactly once, sum
to one million parts, select a maximum-probability candidate, and meet the
request's configured confidence threshold. Failure keeps the existing
deterministic desk fallback.

Hive consensus has a typed evaluated path. Each worker output is bound to its
author and source sequence and carries a Choice distribution across topics plus
abstention, a normalized evidence-quality Score, and a Noul policy-violation
probability. Missing or rejected evaluations contribute nothing. Malformed or
stale evaluations stop the fold. An admitted member contributes at most once,
using its latest in-window evaluation:

```text
topic contribution = stance probability × evidence quality
```

All arithmetic is integer parts per million. Cross-inhibition removes the
targeted member's contribution and refutation caps remain structural. A topic
carries at `quorum.threshold × 1_000_000` expected support.

The approval fold remains the authority for side effects. Semantic evaluation
may raise a host-declared effect to mutating or unclassified; it can never turn
a mutating, denied, refused, unapprovable, or unclassified request into an
allow. Evaluator absence and threshold failure are unclassified and therefore
deny.

## Provider boundary

No production library crate depends on a transport or Jev client. The benchmark
takes `tinyjevclient` as an example-only dev-dependency and converts provider
answers into provider-neutral fixed-point payloads. The hive and core crates
remain pure folds.

## Evaluation

`bench --decision-eval --episodes N` sends byte-identical state and equivalent
Choice, Score, and Noul questions to Jev and to `openai/gpt-5-mini` through
OpenRouter strict JSON Schema. Request order alternates per case. The report
includes p50/p90/p99 latency, throughput, input/output tokens, estimated cost,
primitive accuracy, Choice/Noul Brier scores, Score MAE, provider/schema
failure rate, and diagnostics. Prices are explicit run constants rather than
claims about future billing.

Credentials come only from `TYPESAFE_API_KEY` and `OPENROUTER_API_KEY`; neither
is placed in process arguments or output.
