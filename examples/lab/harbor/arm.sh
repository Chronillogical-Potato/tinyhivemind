#!/usr/bin/env bash
# One arm over a directory of Harbor tasks, for long-horizon runs where arms
# run side by side. Usage: arm.sh TASKS_DIR JOBS_DIR TAG ARM [extra harbor args]
#
# ARM is one of:
#   hive-briefing     hive, every activation starts from the briefing (--seat-session fresh)
#   hive-session      hive, persistent seat sessions, no memory
#   hive-session-mem  hive, persistent seat sessions, CortexDB memory
#   single            single agent, masking (same as single-mask)
#   single-mem        single agent, masking, CortexDB memory
#   hive              alias of hive-session (the default hive since sessions persist)
#   single-none | single-mask | single-summarize | single-mask+summarize
#
# Env: MAX_TURNS, TOKEN_CAP, TIMEOUT_MULT (agent timeout multiplier), ATTEMPTS.
# The memory arms need CORTEX_DB_URL and CORTEX_DB_KEY (docker/cortex/README.md);
# every trial gets its own run id, so trials never share memory.
set -euo pipefail
tasks=$1 jobs=$2 tag=$3 arm=$4; shift 4
here=$(cd "$(dirname "$0")" && pwd)
mode=hive ctx=mask session=persistent memory=none
case $arm in
  hive-briefing) session=fresh ;;
  hive|hive-session) ;;
  hive-session-mem) memory=cortex ;;
  single) mode=single ;;
  single-mem) mode=single memory=cortex ;;
  single-*) mode=single ctx=${arm#single-} ;;
  *) echo "unknown arm $arm" >&2; exit 2 ;;
esac
if [ "$memory" = cortex ] && { [ -z "${CORTEX_DB_URL:-}" ] || [ -z "${CORTEX_DB_KEY:-}" ]; }; then
  echo "arm $arm needs CORTEX_DB_URL and CORTEX_DB_KEY" >&2; exit 2
fi
PYTHONPATH=$here harbor run -p "$tasks" --agent hive_agent:HiveAgent \
  -m openai/gpt-oss-120b:nitro --ak mode=$mode --ak single_context=$ctx \
  --ak seat_session=$session --ak memory=$memory \
  --ak token_cap=${TOKEN_CAP:-4000000} --ak max_turns=${MAX_TURNS:-400} \
  --agent-timeout-multiplier ${TIMEOUT_MULT:-0.09} -k ${ATTEMPTS:-1} -n 1 \
  -o "$jobs" --job-name "$tag-$arm" -y "$@"
