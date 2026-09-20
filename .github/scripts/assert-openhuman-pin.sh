#!/bin/sh
# Assert the `rev` of the git `openhuman-embed` dependency in Cargo.toml equals
# the commit recorded for the `vendor/openhuman` submodule. The `[patch]` in
# Cargo.toml builds this workspace against the submodule; a consumer without
# that patch builds against the rev. If they drift, the two build different
# OpenHuman trees and nothing else would say so.
set -eu
cd "$(dirname "$0")/../.."
rev=$(sed -n 's/^openhuman-embed = { git = "https:\/\/github.com\/tinyhumansai\/openhuman", rev = "\([0-9a-f]*\)".*/\1/p' Cargo.toml)
sub=$(git ls-tree HEAD vendor/openhuman | awk '{print $3}')
if [ -z "$rev" ] || [ -z "$sub" ]; then
  echo "assert-openhuman-pin: could not read the rev ($rev) or the submodule pointer ($sub)" >&2
  exit 1
fi
if [ "$rev" != "$sub" ]; then
  echo "assert-openhuman-pin: Cargo.toml pins openhuman-embed at $rev but vendor/openhuman records $sub" >&2
  exit 1
fi
echo "assert-openhuman-pin: $rev"
