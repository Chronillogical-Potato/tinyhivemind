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
