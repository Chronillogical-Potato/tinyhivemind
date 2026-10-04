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

check() { # label mode [extra swe_hive flags...]
  local label="$1" mode="$2"
  shift 2
  docker exec "$name" rm -f /tmp/hello.txt
  env -u OPENROUTER_API_KEY "$bin" --mode "$mode" --task "$task" --container "$name" \
    --api-base "$api" --trace "$work/$label.jsonl" --result "$work/$label.json" \
    --max-turns 30 --token-cap 100000 "$@" 2>"$work/$label.err"
  [ "$(docker exec "$name" cat /tmp/hello.txt)" = "hi" ] || { echo "$label: file missing"; exit 1; }
  python3 - "$work/$label.json" "$work/$label.jsonl" "$mode" "$label" <<'PY'
import json, sys
result = json.load(open(sys.argv[1]))
events = [json.loads(line) for line in open(sys.argv[2])]
mode, label = sys.argv[3], sys.argv[4]
kinds = {e["event"] for e in events}
assert result["completed"] is True, result
assert result["mode"] == mode, result
assert result["tokens_in"] > 0 and result["tokens_out"] > 0 and result["turns"] > 0, result
need = {"turn_started", "turn_finished", "tool_call", "mark"}
if mode == "hive":
    need |= {"round", "converged"}
assert need <= kinds, (need - kinds)
marks = [e for e in events if e["event"] == "mark"]
resumed = [e for e in events if e["event"] == "session_resumed"]
recalled = [e for e in events if e["event"] == "recalled"]
remembered = [e for e in events if e["event"] == "remembered"]
failures = [m["detail"] for m in marks if m.get("label") == "memory" and "error=" in m["detail"]]
if label.startswith("hive-persistent"):
    assert result["seat_session"] == "persistent", result
    lead = [e for e in resumed if e["seat"] == "lead"]
    assert lead and min(e["messages"] for e in lead) > 2, ("a woken lead resumes its session", resumed)
if label == "hive-fresh":
    assert result["seat_session"] == "fresh" and result["context_policy"] == "mask", result
    assert not resumed, ("fresh sessions never resume", resumed)
if label.endswith("-mem"):
    assert result["memory"] == "cortex", result
    assert any(e["moment"] == "session_start" for e in recalled), recalled
    assert remembered and all(e["entries"] > 0 for e in remembered), remembered
    assert not failures, failures
    if mode == "hive":
        rejoin = [e for e in recalled if e["moment"] == "rejoin"]
        assert rejoin and all(e["notes"] > 0 for e in rejoin), ("a teammate's turn is indexed in time", rejoin)
else:
    assert result["memory"] == "none" and not recalled and not remembered, result["memory"]
sessions = resumed
memory = recalled + remembered
print(f'{label}: ok  turns={result["turns"]} in={result["tokens_in"]} '
      f'out={result["tokens_out"]} wall_ms={result["wall_ms"]} '
      f'sessions={len(sessions)} memory_marks={len(memory)}')
PY
}

check single single
check hive-persistent hive --seat-session persistent --memory none
check hive-fresh hive --seat-session fresh

# Memory against a live CortexDB, only when one is configured (see
# docker/cortex/README.md); each run writes under its own team:<run-id>.
if [ -n "${CORTEX_DB_URL:-}" ] && [ -n "${CORTEX_DB_KEY:-}" ]; then
  check hive-persistent-mem hive --seat-session persistent --memory cortex --run-id "offline-hive-$$"
  check single-mem single --memory cortex --run-id "offline-single-$$"
  cp "$work/hive-persistent-mem.jsonl" "${SWE_OFFLINE_KEEP_TRACE:-/dev/null}" 2>/dev/null || true
else
  echo "memory: skipped (set CORTEX_DB_URL and CORTEX_DB_KEY to exercise --memory cortex)"
fi

# A long single-arm session (24 commands, ~2.5 KB each) under each context
# policy and a small budget: mask and summarize must keep the largest prompt
# well under the unbounded run's, and say so in result.json and the trace.
long_run() { # policy
  local policy="$1"
  docker exec "$name" rm -f /tmp/hello.txt
  env -u OPENROUTER_API_KEY "$bin" --mode single --task "LONGSESSION $task" --container "$name" \
    --api-base "$api" --trace "$work/long-$policy.jsonl" --result "$work/long-$policy.json" \
    --max-turns 80 --token-cap 5000000 --single-context "$policy" --context-budget 2000 \
    --context-keep 4 2>"$work/long-$policy.err"
  [ "$(docker exec "$name" cat /tmp/hello.txt)" = "hi" ] || { echo "long $policy: file missing"; exit 1; }
}
long_run none
long_run mask
long_run summarize
python3 - "$work" <<'PY'
import json, sys
w = sys.argv[1]
r = {p: json.load(open(f"{w}/long-{p}.json")) for p in ("none", "mask", "summarize")}
marks = {p: [json.loads(l) for l in open(f"{w}/long-{p}.jsonl")] for p in r}
for p, x in r.items():
    assert x["completed"] is True and x["context_policy"] == p, (p, x)
assert r["none"]["context_events"] == 0 and r["none"]["max_prompt_tokens"] > 10000, r["none"]
for p in ("mask", "summarize"):
    assert r[p]["context_events"] > 0, (p, r[p])
    assert r[p]["max_prompt_tokens"] < r["none"]["max_prompt_tokens"] // 2, (p, r[p], r["none"])
    assert any(e["event"] == "mark" and e.get("label") == "context" for e in marks[p]), p
assert r["mask"]["turns"] == r["none"]["turns"], "mask must cost no extra model calls"
assert r["summarize"]["turns"] > r["none"]["turns"], "summarize pays extra calls"
print("context: " + "  ".join(
    f'{p}: max_prompt={x["max_prompt_tokens"]} events={x["context_events"]} turns={x["turns"]} in={x["tokens_in"]}'
    for p, x in r.items()))
PY
echo "offline run passed"
