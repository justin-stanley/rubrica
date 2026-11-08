#!/usr/bin/env bash
# The workspace's tests, as the gate (scripts/ci.sh) and the MSRV job run them.
#
# With cargo-nextest on PATH the unit and integration tests run under it, on the `ci`
# profile of .config/nextest.toml (no retries, no fail-fast: a flaky test stays visible and
# every failure is listed), and the doctests run on their own, because nextest does not run
# them. Without it everything runs under `cargo test`, as before, and one line says which.
#
# RUBRICA_REQUIRE_NEXTEST=1 (set in CI's workflows) makes a missing cargo-nextest an error
# instead of a fallback, so a broken install cannot quietly turn the fast path off.
set -euo pipefail
cd "$(dirname "$0")/.."

if cargo nextest --version >/dev/null 2>&1; then
  echo "run-tests: cargo nextest ($(cargo nextest --version | head -1)), then the doctests under cargo test --doc"
  cargo nextest run --workspace --all-features --locked --profile ci
  cargo test --doc --workspace --all-features --locked
elif [ "${RUBRICA_REQUIRE_NEXTEST:-}" = 1 ]; then
  echo "run-tests: RUBRICA_REQUIRE_NEXTEST=1 but cargo-nextest is not installed"
  exit 1
else
  echo "run-tests: cargo test (no cargo-nextest on PATH)"
  cargo test --all-features --locked
fi
