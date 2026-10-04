#!/usr/bin/env bash
# End-to-end run of both arms with no network and no API key: a scripted mock
# model on localhost drives swe_hive against a throwaway container started with
# `--network none`. Run from anywhere; needs docker, python3, curl, cargo.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
lab="$(dirname "$here")"
image="${SWE_OFFLINE_IMAGE:-python:3.12-slim}"
work="$(mktemp -d)"
name="swe-offline-$$"
mock_pid=""

cleanup() {
  [ -n "$mock_pid" ] && kill "$mock_pid" 2>/dev/null || true
  docker rm -f "$name" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

(cd "$lab" && cargo build --quiet --bin swe_hive)
bin="$lab/target/debug/swe_hive"

python3 "$here/mock_llm.py" --port-file "$work/port" 2>"$work/mock.log" &
mock_pid=$!
for _ in $(seq 50); do [ -s "$work/port" ] && break; sleep 0.1; done
api="http://127.0.0.1:$(cat "$work/port")/v1"

docker run -d --rm --network none --name "$name" "$image" sleep 600 >/dev/null
task='Write the word hi into /tmp/hello.txt and check it.'

check() { # mode
  local mode="$1"
  docker exec "$name" rm -f /tmp/hello.txt
  env -u OPENROUTER_API_KEY "$bin" --mode "$mode" --task "$task" --container "$name" \
    --api-base "$api" --trace "$work/$mode.jsonl" --result "$work/$mode.json" \
    --max-turns 30 --token-cap 100000 2>"$work/$mode.err"
  [ "$(docker exec "$name" cat /tmp/hello.txt)" = "hi" ] || { echo "$mode: file missing"; exit 1; }
  python3 - "$work/$mode.json" "$work/$mode.jsonl" "$mode" <<'PY'
import json, sys
result = json.load(open(sys.argv[1]))
events = [json.loads(line) for line in open(sys.argv[2])]
kinds = {e["event"] for e in events}
assert result["completed"] is True, result
assert result["mode"] == sys.argv[3], result
assert result["tokens_in"] > 0 and result["tokens_out"] > 0 and result["turns"] > 0, result
need = {"turn_started", "turn_finished", "tool_call", "mark"}
if sys.argv[3] == "hive":
    need |= {"round", "converged"}
assert need <= kinds, (need - kinds)
print(f'{result["mode"]}: ok  turns={result["turns"]} in={result["tokens_in"]} '
      f'out={result["tokens_out"]} wall_ms={result["wall_ms"]}')
PY
}

check single
check hive
echo "offline run passed"
