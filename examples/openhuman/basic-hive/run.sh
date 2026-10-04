#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$repo_root"

mode="${1:-offline}"
case "$mode" in
  offline)
    network_args=(--network none)
    environment_args=()
    program_args=()
    ;;
  live)
    : "${OPENROUTER_API_KEY:?set OPENROUTER_API_KEY before a live run}"
    network_args=(--network bridge)
    environment_args=(--env OPENROUTER_API_KEY)
    if [[ -n "${OPENROUTER_MODEL:-}" ]]; then
      environment_args+=(--env OPENROUTER_MODEL)
    fi
    program_args=(--live)
    ;;
  *)
    printf 'usage: %s [offline|live]\n' "$0" >&2
    exit 2
    ;;
esac

image="tinyhivemind-basic-hive:local"
docker build --file examples/openhuman/basic-hive/Dockerfile --tag "$image" .
docker run --rm --read-only --tmpfs /tmp:rw,nosuid,nodev,size=1g \
  --cap-drop ALL --security-opt no-new-privileges \
  --pids-limit 256 --memory 2g --cpus 2 \
  "${network_args[@]}" "${environment_args[@]}" \
  "$image" "${program_args[@]}"
