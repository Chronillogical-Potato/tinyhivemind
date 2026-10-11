# A hive-language hive on two live Euler problems

**Date:** 2026-10-11. **Status:** Recorded.
**Code:** `cargo run --manifest-path examples/openhuman/Cargo.toml --bin euler_hive -- --problem <statement.html> --problem-id <id> --runs hive-runs [--max-turns 80]`
(see [`euler_hive/README.md`](../../examples/openhuman/src/bin/euler_hive/README.md)).
**Package:** [`euler-hive`](../../crates/tinyhivemind-lang/examples/hives/euler-hive/README.md).
**Spec:** [hive language](../specs/hive-language.md), [hive memory](../specs/hive-memory.md),
[cross-desk referral](../specs/cross-desk-referral.md).

This record is redacted, as Project Euler's rules require. It gives no
answer, no number or data from either statement, and no value the hive
proposed. Run directories, the problem statements and both oracles stay
under the git-ignored `hive-runs/`.

## The question

Can a hive that is defined entirely in the hive language — desks, personas,
tool access, topology, referral policy and memory bindings — be lowered into
live OpenHuman agents and attempt a hard problem end to end? We counted it a
success only if the hive committed the answer that a hidden oracle computes
independently.

The setup had these pieces:

- **Package.** Fourteen persona seats on five desks run as a pipeline:
  research, theory, compute, verify, synthesis.
- **Models.** Every model call went through OpenRouter, using
  `google/gemini-3.8-flash`.
- **Web search.** Research seats only, through the TinyHumans SDK
  (Parallel, and Gemini with Google Search grounding).
- **Python.** Compute and verify seats only, in a fresh Docker container
  per program, with no network and nothing mounted.
- **Memory.** Every seat was bound to CortexDB behind the TinyHumans backend.
- **Routing.** Each desk ran on its own core `CompletionDriver` floor. Desks
  reached each other only through core's `referral` fold.

The two problems were PE1014 (a combinatorial design) and PE1012 (an
equilibrium sum the user rated harder).

## The oracles

Neither oracle is in any prompt, context or memory, and the sandbox can mount
nothing, so the hive could not reach them.

- **PE1014.** The oracle rebuilt the whole design from the given blocks and
  checked three things: there was exactly one completion, it had 759 blocks,
  and every 5-set was covered exactly once. The answer block satisfied every
  constraint in the statement.
- **PE1012.** The oracle solved each `n` exactly in rationals up to 2,000,
  checking every equilibrium condition. It then solved up to the target in
  double precision with iterative refinement. A 40-digit fallback took over
  where the support changes (one such `n`). Five widely spaced `n` checked
  at 40 digits differed from the float values by at most 4e-16. The float sum
  matched the exact sum at 2,000 to 4e-15, and it reproduces every sample
  value in the statement. We treat it as trustworthy to the requested
  precision.

## Runs

| Run | Problem | Ending | Correct | Turns | Model calls | Tokens in / out | OpenRouter | Search | Wall | Sandbox | Searches |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| smoke 1 (`deepseek/deepseek-v4-pro`) | 1014 | turn cap (8) | — | 8 | 32 | 154k / 57k | $0.08 | $0.02 | 15 min | 1 | 2 |
| smoke 2 | 1014 | turn cap (16) | — | 15 | 46 | 207k / 19k | $0.15 | $0.07 | 17 min | 6 | 3 |
| full 1 | 1014 | converged | **yes** | 18 | 67 | 351k / 33k | $0.27 | $0.12 | 14 min | 13 | 10 |
| full 1 | 1012 | idle (host defect) | — | 17 | 78 | 875k / 62k | $0.62 | $0.12 | 26 min | 25 | 12 |
| full 2 | 1012 | converged | no | 21 | 99 | 1.04M / 64k | $0.65 | $0.07 | 23 min | 40 | 5 |
| full 3 | 1012 | converged | no | 52 | 211 | 3.10M / 172k | $1.80 | $0.09 | 51 min | 60 | 4 |

The OpenRouter column is the key's own account delta for the run. OpenHuman
reported a cost of zero for this model, so the host meters spend from the
account instead. Wall time includes runtime boot. In smoke 1, before the
keychain fix described below, boot took about five minutes.

## PE1014: what happened

The run was correct on the first full attempt: 18 turns, about $0.39
including search, 14 minutes.

- Research named the structure, but two seats also searched for the
  problem's own data strings. No search result contained the answer; that
  behaviour is why the query guard below exists.
- One research seat asked theory, and theory asked compute. Both answers came
  home by core's `Return`.
- The candidate first appeared in the printed output of a compute program.
  Four more compute programs and nine verify programs reproduced it.
- All three verify seats published to the verified pool, and synthesis
  committed.

All eight referrals were accepted: two asks, two returns, and four pipeline
reports at hops 1 to 4.

## PE1012: what happened

Every PE1012 run reproduced the statement's sample values and the structure
of the support. Its boundaries matched the oracle's. No run committed the
correct sum.

- **Run 1** went idle after 17 turns. Two research seats asked theory at the
  same time, and each theory answer asked compute. The host's single
  resume slot per desk then dropped one answer's way home, so research waited
  forever. This was a host defect and is fixed below.
- **Run 2** converged on a value the oracle rejects. Compute spent all 40
  sandbox runs, so the verify desk ran nothing and still signed off.
- **Run 3** ran with the fixes. Verify reserved and spent 20 programs, and
  the support gate refused one ungrounded support. No verify seat supported a
  candidate. Synthesis committed a value anyway, against its own charter, and
  the oracle rejects it. The host now refuses that; see below.

## Memory, read back from CortexDB

These counts are read from the store after each run, per seat namespace
`team:euler-hive/ws:<run>/agent:<seat>`. They are not host counters.

| Run | Conversation items (OpenHuman's own turn log) | Learnings (host `Remember` and `hive_remember`) | Verified-pool writes | Notes recalled during the run | Notes a post-run recall finds |
| --- | --- | --- | --- | --- | --- |
| 1014 full 1 | 36 | 21 | 3 | 0 | not measured |
| 1012 full 2 | 46 | 27 | 0 | 0 | 27 |
| 1012 full 3 | 113 | 56 | 0 | 55 | 48 |

Every seat holds both kinds. Early in-run recall was empty by design: a
binding reads only the seat's own namespace and the verified pool, and a
rejoin recalls only what *other* seats wrote since. Recall grows as the pool
and the sessions fill. That is the 55 notes recalled in PE1012 run 3.

## What the runs changed in the host

Each fix below has a regression test in `euler_hive/test.rs`.

- **Overlapping questions lost an answer.** Work on a desk is now a *chain*
  with its own id. A question owes its chain one answer, and the answer
  reopens the chain on the asking seat when it runs. A chain reports only
  once nothing it asked is still owed.
  (`overlapping_questions_with_nested_asks_all_come_home_and_the_pipeline_finishes`)
- **Compute starved verify of sandbox runs.** A third of the sandbox budget
  is now held for the verify desk, reserved atomically.
- **Ungrounded support.** A verify seat's `support` is refused unless it ran
  a `run_python` that executed during that turn.
- **Committing an unverified answer.** Synthesis's candidate becomes the
  hive's answer only if a verify seat supported it. Otherwise the run ends
  `no_decision`.
- **Research looked the problem up.** A query that quotes the statement's
  own data (long numbers, upper-case data words) or looks up the problem's
  source is refused before it is sent. In PE1012 run 1 a search had already
  surfaced a published derivation of the problem's closed form; it carried
  no final value.
- **Boot stall.** OpenHuman's secret store waited minutes on the OS
  keychain. Upstream now keeps secrets in memory for an embed host, and the
  vendored OpenHuman was moved to that release.
- **Exa and TinyFish refuse API keys.** The TinyHumans backend answers an
  API-key caller with 403 on Exa and 404 on TinyFish, so seats are offered
  Parallel and Gemini. `--probe` records both refusals.

## In the seats' own words

Excerpts are trimmed, and every answer and data string is redacted:

> **research-librarian** (PE1014): "The main problem describes the Witt design
> S(5, 8, 24), the unique Steiner system with parameters v = 24, k = 8, t = 5."

> **theory-counter** (PE1014): "Auditing the combinatorial counting … Total
> number of blocks: b = C(24, 5) / C(8, 5) = 42504 / 56 = 759."

> **verify-rederiver** (PE1014): "Sample validation: … Filtering all
> 3-subsets … uniquely yields [the sample's block], matching the problem
> statement."

> **theory-counter** (PE1012): "Brilliant breakthrough on the mechanism of
> support intervals! … The lower bound cannot advance further once the
> element immediately below gets positive expected payoff."

## What this does and does not show

It shows that a hive-language package can be lowered into live, tool-gated,
memory-bound OpenHuman agents on desk floors joined only by referral. A
14-seat hive solved a design problem correctly for about $0.40. On the harder
problem it reproduced every sample and the right structure, but still
committed wrong answers twice. Its own verification desk was the weak point:
first it was starved of runs, then it was ignored. Both are now enforced by
the host instead of asked of the model.

These are single runs of one model, not a benchmark. Nothing here measures
the hive against a single agent at matched budget. Across the runs above
plus the runs listed in the
[addendum](2026-10-11-euler-hive-live-addendum.md), the total spend was about
$5.
