#!/usr/bin/env python3
"""Summarise compare.sh jobs: per-task pass rate, mean tokens and wall time per arm."""
import json, pathlib, statistics, sys

jobs = pathlib.Path(sys.argv[1])
tag = sys.argv[2] if len(sys.argv) > 2 else "run"
runs: dict = {}
for mode in ("single", "hive"):
    for trial in sorted((jobs / f"{tag}-{mode}").glob("*__*")):
        r, a = trial / "result.json", trial / "agent" / "result.json"
        if not r.exists():
            continue
        res = json.loads(r.read_text())
        reward = ((res.get("verifier_result") or {}).get("rewards") or {}).get("reward") or 0
        ag = json.loads(a.read_text()) if a.exists() else {}
        runs.setdefault(trial.name.split("__")[0], {}).setdefault(mode, []).append({
            "reward": reward, "tokens": ag.get("tokens_in", 0) + ag.get("tokens_out", 0),
            "wall_s": ag.get("wall_ms", 0) / 1000, "capped": not ag.get("completed", False)})

print(f"{'task':26} {'mode':7} pass  mean_tok  mean_wall_s  capped")
tot = {m: dict(n=0, p=0.0, tok=0, wall=0.0) for m in ("single", "hive")}
for task, modes in runs.items():
    for m, v in modes.items():
        p = sum(x["reward"] for x in v)
        print(f"{task:26} {m:7} {p:g}/{len(v)}  {statistics.mean(x['tokens'] for x in v):8.0f}  "
              f"{statistics.mean(x['wall_s'] for x in v):11.1f}  {sum(x['capped'] for x in v)}")
        t = tot[m]; t["n"] += len(v); t["p"] += p
        t["tok"] += sum(x["tokens"] for x in v); t["wall"] += sum(x["wall_s"] for x in v)
for m, t in tot.items():
    if t["n"]:
        print(f"TOTAL {m:7} passes={t['p']:g}/{t['n']} tokens={t['tok']} wall_s={t['wall']:.0f}"
              f" tokens/pass={t['tok'] / t['p'] if t['p'] else float('inf'):.0f}")
