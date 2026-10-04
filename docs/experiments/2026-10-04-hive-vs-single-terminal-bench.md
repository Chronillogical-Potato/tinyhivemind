# Hive vs single agent on five Terminal-Bench tasks

One run per arm, `openai/gpt-oss-120b:nitro` through OpenRouter, agent phase
with `network_mode = "no-network"`, model calls from the host. Dataset
`terminal-bench-2-1` (there is no "4.0" in Harbor's registry). Tasks: the 50
with an agent timeout of 900s or less, five sampled with seed 20261004. Both arms:
`--max-turns 60 --token-cap 600000`, hive `round_width` default. Reproduce with
`examples/lab/harbor/compare.sh` and `aggregate.py`.

| task | single | hive |
| --- | --- | --- |
| cancel-async-tasks | fail, 61.7k tok, 14s | fail, 18.6k tok, 8s |
| fix-git | pass, 76.6k, 18s | pass, 159.6k, 43s |
| largest-eigenval | fail, 253k, 47s | fail, 192.8k, 51s (hit 60 turns) |
| multi-source-data-merger | fail, 53k, 14s | pass, 163.7k, 40s |
| vulnerable-secret | pass, 357k, 40s | fail, 169k, 47s (hit 60 turns) |
| **total** | **2/5, 801.8k tok, 132s** | **2/5, 703.9k tok, 188s** |

## Reading

- Passes tie at 2/5, and they are different tasks per arm. n=5 with one trial
  each is noise, not evidence: `cancel-async-tasks` passed in an earlier
  single-agent smoke run and failed here.
- Hive used 12% fewer tokens in total but ran 43% slower in wall time. The
  claim "better, cheaper and faster" is **not supported** by this run.
- The hive hit the 60-turn cap on two tasks, so the cap, not the model, may
  have decided those. The next run should raise it and use several trials.
- Round concurrency barely helped: seats ran one or two at a time. See the
  findings ledger (`2026-10-04-findings.md`) for the scheduling rows.

## Run 2: raised caps, three trials per task

Same tasks and model, after the F22/F13/F9 core fixes and an OpenHuman bump to
upstream `5514ca68`. `--max-turns 150 --token-cap 1500000`, 3 attempts per
task per arm (15 trials per arm). `harbor/aggregate.py target/tb/jobs r2`.

| task | single pass, mean tok, mean wall | hive pass, mean tok, mean wall |
| --- | --- | --- |
| cancel-async-tasks | 2/3, 117k, 18s | 1/3, 54k, 20s |
| fix-git | 3/3, 69k, 17s | 3/3, 95k, 29s |
| largest-eigenval | 0/3, 657k, 71s | 0/3, 605k, 142s |
| multi-source-data-merger | 3/3, 36k, 14s | 3/3, 151k, 35s |
| vulnerable-secret | 0/3, 135k, 29s | 0/3, 483k, 103s |
| **total** | **8/15, 3.04M tok, 448s** | **7/15, 4.16M tok, 984s** |

Tokens per pass: single 380k, hive 594k.

The hive did **not** beat one agent here: one fewer pass, 37% more tokens and
2.2x the wall time. Run 1's token saving and the `multi-source-data-merger`
win did not replicate: with three trials the single agent passed that task
3/3 too, so that win was variance. Raising the cap did not rescue the hive on
the two hard tasks (0/3 for both arms). The hive's cost shows up as
coordination overhead on easy tasks (`multi-source-data-merger` 4x tokens).
Next step is to read the traces for where the hive's tokens go, not to tune
the cap further.
