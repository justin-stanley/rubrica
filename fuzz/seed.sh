#!/usr/bin/env bash
# Write every CommonMark and GFM spec example's markdown to fuzz/corpus/spec/ (the seed
# corpus; one shared directory for all targets). Idempotent. Needs jq.
set -euo pipefail
cd "$(dirname "$0")/.."
out=fuzz/corpus/spec
mkdir -p "$out"
for set in commonmark-0.31.2:cm gfm-0.29:gfm; do
  json=crates/rubrica-markdown/tests/spec/${set%%:*}.json
  n=$(jq length "$json")
  for ((i = 0; i < n; i++)); do
    jq -j ".[$i].markdown" "$json" > "$out/${set##*:}-$(printf '%04d' $((i + 1)))"
  done
done
echo "seed: $(ls "$out" | wc -l | tr -d ' ') files in $out"
