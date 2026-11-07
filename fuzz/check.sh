#!/usr/bin/env bash
# Build every fuzz target on the pinned nightly and run each for FUZZ_SECONDS (default 10)
# on the shared corpus fuzz/corpus/spec. `--build-only` stops after the build.
set -euo pipefail
cd "$(dirname "$0")"
NIGHTLY=nightly-2026-10-30
TARGETS="parse offsets autolinks differential front_matter"

cargo +"$NIGHTLY" fuzz build
if [ "${1:-}" = "--build-only" ]; then
  echo "fuzz: all targets built"
  exit 0
fi
for t in $TARGETS; do
  echo "fuzz: running $t for ${FUZZ_SECONDS:-10}s"
  # The corpus directory is given explicitly so cargo-fuzz's per-target default
  # (corpus/<t>) is never created.
  cargo +"$NIGHTLY" fuzz run "$t" corpus/spec -- -max_total_time="${FUZZ_SECONDS:-10}" -runs=-1
done
echo "fuzz: all targets ran"
