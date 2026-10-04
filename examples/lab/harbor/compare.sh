#!/usr/bin/env bash
# Run both arms over a directory of Harbor tasks, one trial at a time so wall
# clock is not confounded by concurrency. Usage: compare.sh TASKS_DIR JOBS_DIR [TAG]
# The model key comes from OPENROUTER_API_KEY in the environment.
set -euo pipefail
tasks=$1 jobs=$2 tag=${3:-run}
here=$(cd "$(dirname "$0")" && pwd)
(cd "$here/.." && cargo build --release --bin swe_hive)
for mode in single hive; do
  PYTHONPATH=$here harbor run -p "$tasks" --agent hive_agent:HiveAgent \
    -m openai/gpt-oss-120b:nitro --ak mode=$mode --ak token_cap=600000 --ak max_turns=60 \
    -n 1 -o "$jobs" --job-name "$tag-$mode" -y
done
