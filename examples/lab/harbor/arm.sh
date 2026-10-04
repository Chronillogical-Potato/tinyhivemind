#!/usr/bin/env bash
# One arm over a directory of Harbor tasks, for long-horizon runs where arms
# run side by side. Usage: arm.sh TASKS_DIR JOBS_DIR TAG ARM [extra harbor args]
# ARM is hive, single-none, single-mask or single-summarize. Env: MAX_TURNS,
# TOKEN_CAP, TIMEOUT_MULT (agent timeout multiplier), ATTEMPTS.
set -euo pipefail
tasks=$1 jobs=$2 tag=$3 arm=$4; shift 4
here=$(cd "$(dirname "$0")" && pwd)
case $arm in
  hive) mode=hive ctx=mask ;;
  single-*) mode=single ctx=${arm#single-} ;;
  *) echo "unknown arm $arm" >&2; exit 2 ;;
esac
PYTHONPATH=$here harbor run -p "$tasks" --agent hive_agent:HiveAgent \
  -m openai/gpt-oss-120b:nitro --ak mode=$mode --ak single_context=$ctx \
  --ak token_cap=${TOKEN_CAP:-4000000} --ak max_turns=${MAX_TURNS:-400} \
  --agent-timeout-multiplier ${TIMEOUT_MULT:-0.09} -k ${ATTEMPTS:-1} -n 1 \
  -o "$jobs" --job-name "$tag-$arm" -y "$@"
