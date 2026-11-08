#!/usr/bin/env bash
# The gate every change passes, locally and in CI.
set -euo pipefail
cd "$(dirname "$0")/.."

# Never share a target dir with another checkout: a shared one once ran another
# worktree's stale test binary as "this" checkout's tests.
target=$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
# Both spellings of the checkout count: a symlinked one is the same checkout.
case "$target" in "$PWD"/target|"$(pwd -P)"/target) ;; *) echo "ci: target dir is '$target', not $PWD/target: unset CARGO_TARGET_DIR"; exit 1;; esac

# The search page's wasm bundle. Its files are never committed and rubrica-site embeds them
# by default (the `search-wasm` feature), so they come before any build:
# * with the wasm32-unknown-unknown target on the pinned toolchain and a wasm-bindgen CLI on
#   PATH, scripts/build-wasm.sh --bundle writes them (held to the size budget), and
#   scripts/wasm-reproducible.sh proves a second build from another target directory gives
#   the same bytes and that the files are the build's, byte for byte;
# * without them, a bundle already in the crate is used as it is, and the gate says so (CI's
#   jobs that download the one the wasm job built and checked);
# * with neither, rubrica-site is built --no-default-features only, and the gate fails.
bundle_dir=crates/rubrica-wasm-bundle/src
wasm_targets=$(rustup target list --installed) || { echo "ci: rustup target list failed"; exit 1; }
if grep -qxF wasm32-unknown-unknown <<<"$wasm_targets" && command -v wasm-bindgen >/dev/null; then
  scripts/build-wasm.sh --bundle
  scripts/wasm-reproducible.sh
elif [ -f "$bundle_dir/rubrica-search_bg.wasm" ] && [ -f "$bundle_dir/rubrica-search.js" ] && [ -f "$bundle_dir/meta.rs" ]; then
  echo "ci: no wasm32-unknown-unknown target or no wasm-bindgen; the bundle in $bundle_dir is used as it is, not rebuilt or checked"
else
  cargo build --locked -p rubrica-site --no-default-features
  echo "ci: no wasm32-unknown-unknown target or no wasm-bindgen, and no bundle in $bundle_dir: rubrica-site built --no-default-features only"
  echo "ci: the rest of the gate needs the bundle: rustup target add wasm32-unknown-unknown, cargo install wasm-bindgen-cli@<the version Cargo.lock pins> --locked, then scripts/build-wasm.sh --bundle"
  exit 1
fi
# The feature switch works both ways: rubrica-site builds without the bundle too.
cargo build --locked -p rubrica-site --no-default-features
# The bundle crate ships exactly its PACKAGE-FILES, under crates.io's size limit, and the
# unpacked crate builds on its own. (The other crates' lists are crates/rubrica/tests/package.rs;
# CI's `package` job packs and builds all five.)
scripts/package-check.sh rubrica-wasm-bundle
echo "ci: the bundle crate ships PACKAGE-FILES exactly"

cargo fmt --all --check
# Strictness about positions is test-only: no cargo feature carries it, since
# `--all-features` (below) would unify one into the `rubrica` binary.
if cargo tree -e features --all-features --locked --workspace -i rubrica-render | grep -i strict; then
  echo "ci: a strict feature reaches the build under --all-features; strictness is test-only"; exit 1
fi
cargo build --all-targets --all-features --locked
# The tests clean up after themselves: run with TMPDIR a fresh,
# empty directory, they leave nothing in it. A test's scratch directory is a
# guard that removes it when dropped; before, one passing run left 114
# directories (351 MB) in TMPDIR, on every run, which would fill the CI
# runner's disk. Nothing is allowed: cargo writes nothing there, and rustc's
# own temporary directories are gone when it exits.
test_tmp="$PWD/target/ci-test-tmp"
rm -rf "$test_tmp" && mkdir -p "$test_tmp"
TMPDIR="$test_tmp" cargo test --all-features --locked
left=$(find "$test_tmp" -mindepth 1 -maxdepth 1 | sort) || left="(find failed)"
[ -z "$left" ] || {
  echo "ci: the tests left $(printf '%s\n' "$left" | wc -l | tr -d ' ') entries in TMPDIR ($test_tmp):"
  printf '%s\n' "$left" | head -20
  echo "ci: a test's scratch directory must be removed when the test ends (a guard that removes it on drop)"
  exit 1; }
rmdir "$test_tmp"
echo "ci: the tests left nothing in TMPDIR"
cargo clippy --all-targets --all-features --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features --locked
# CI runs these two as their own jobs (RUBRICA_CI_SPLIT=1); locally the gate runs everything.
if [ "${RUBRICA_CI_SPLIT:-}" != 1 ]; then
  cargo deny check bans licenses sources
  # bincode 1.x via syntect's dump-load: the syntax dump is a build-time embedded blob, never input
  cargo audit -D warnings --ignore RUSTSEC-2025-0141
fi
scripts/no-js.sh
# The release-tag verifier refuses every tag that is not an annotated tag with a listed signature.
scripts/test-verify-release-tag.sh
# The fuzz targets must keep building. They need the pinned nightly; the fuzz.yml job
# always has it, a laptop may not.
if rustup toolchain list | grep -q nightly-2026-10-30; then
  fuzz/check.sh --build-only
else
  echo "ci: no pinned nightly; fuzz targets not built"
fi
# No crate reads a file outside itself or one it does not ship, and the guard that says so
# is itself tested (planted bad cases must fail, each with its exit code and message).
scripts/test-crate-paths.sh
scripts/crate-paths.sh
echo "ci: all green"
