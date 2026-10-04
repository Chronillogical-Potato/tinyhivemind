#!/usr/bin/env python3
"""Summarise compare.sh jobs: per-task reward, tokens and wall time per arm."""
import json, sys, pathlib

jobs = pathlib.Path(sys.argv[1])
tag = sys.argv[2] if len(sys.argv) > 2 else "run"
rows = {}
for mode in ("single", "hive"):
    for trial in sorted((jobs / f"{tag}-{mode}").glob("*__*")):
        r = trial / "result.json"
        a = trial / "agent" / "result.json"
        if not r.exists():
            continue
        res = json.loads(r.read_text())
        reward = ((res.get("verifier_result") or {}).get("rewards") or {}).get("reward")
        ag = json.loads(a.read_text()) if a.exists() else {}
        rows.setdefault(trial.name.split("__")[0], {})[mode] = {
            "reward": reward, "tokens": ag.get("tokens_in", 0) + ag.get("tokens_out", 0),
            "wall_s": round(ag.get("wall_ms", 0) / 1000, 1), "turns": ag.get("turns")}
print(f"{'task':28} {'mode':7} reward  tokens  wall_s turns")
tot = {m: [0, 0, 0.0] for m in ("single", "hive")}
for task, modes in rows.items():
    for m, v in modes.items():
        print(f"{task:28} {m:7} {str(v['reward']):6} {v['tokens']:7} {v['wall_s']:7} {v['turns']}")
        t = tot[m]; t[0] += v["reward"] or 0; t[1] += v["tokens"]; t[2] += v["wall_s"]
for m, t in tot.items():
    print(f"TOTAL {m:7} passes={t[0]} tokens={t[1]} wall_s={round(t[2], 1)}")
