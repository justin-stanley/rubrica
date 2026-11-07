#!/usr/bin/env bash
# The search page's bundle: crates/rubrica-wasm built for wasm32-unknown-unknown with the
# size profile (`--profile wasm`, never `--release`), then the loader written by the
# wasm-bindgen CLI, which must be the version Cargo.lock pins for the crate. Both outputs,
# <target>/wasm-bindgen/rubrica-search_bg.wasm and <target>/wasm-bindgen/rubrica-search.js,
# are held to the size budget. <target> is cargo's target directory: ./target, or
# CARGO_TARGET_DIR where scripts/wasm-reproducible.sh sets it for its second build. The
# loader has its own directory, not <target>/wasm/, which is cargo's for the wasm profile.
# No wasm-opt: its output is not reproducible yet. With --bundle, the two files and their
# provenance are also written into crates/rubrica-wasm-bundle/src/.
#
# Exit codes: 0 built and under budget; 1 over budget or a version mismatch; 2 a usage
# error, a missing tool or a failed build.
set -euo pipefail
export LC_ALL=C

WASM_BUDGET=1572864   # 1.5 MiB
LOADER_BUDGET=32768   # 32 KiB

bundle=0
case $#:${1:-} in
  0:) ;;
  1:--bundle) bundle=1 ;;
  *) echo "build-wasm: takes no arguments but --bundle" >&2; exit 2 ;;
esac
cd "$(dirname "$0")/.." || { echo "build-wasm: cannot enter the repository root" >&2; exit 2; }

if ! command -v wasm-bindgen >/dev/null; then
  echo "build-wasm: no wasm-bindgen on PATH (cargo install wasm-bindgen-cli --locked, at the version Cargo.lock pins)" >&2
  exit 2
fi
# The crate's version, from the lock file: the package line after `name = "wasm-bindgen"`.
rc=0
locked=$(awk '$0 == "name = \"wasm-bindgen\"" { getline; gsub(/^version = "|"$/, ""); print; exit }' Cargo.lock) || rc=$?
if [ "$rc" -ne 0 ] || [ -z "$locked" ]; then
  echo "build-wasm: no wasm-bindgen version in Cargo.lock" >&2
  exit 2
fi
cli=$(wasm-bindgen --version) || { echo "build-wasm: wasm-bindgen --version failed" >&2; exit 2; }
if [ "$cli" != "wasm-bindgen $locked" ]; then
  echo "build-wasm: the CLI is '$cli', the crate is $locked: they must be the same version" >&2
  exit 1
fi

target_dir=${CARGO_TARGET_DIR:-$PWD/target}
case $target_dir in /*) ;; *) target_dir=$PWD/$target_dir ;; esac
out=$target_dir/wasm-bindgen

# The paths baked into the .wasm are made neutral, so the bundle names no checkout and no
# target directory. The last matching remap wins: the target directory's comes after the
# checkout's, which may hold it.
RUSTFLAGS="--remap-path-prefix=$PWD=/rubrica --remap-path-prefix=$target_dir=/target --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo" \
  cargo build --profile wasm --locked -p rubrica-wasm --target wasm32-unknown-unknown ||
  { echo "build-wasm: cargo build failed" >&2; exit 2; }
rm -rf "$out"
wasm-bindgen --target web --no-typescript --out-dir "$out" --out-name rubrica-search \
  "$target_dir/wasm32-unknown-unknown/wasm/rubrica_wasm.wasm" ||
  { echo "build-wasm: wasm-bindgen failed" >&2; exit 2; }

size() { wc -c <"$1" | tr -d ' '; }
wasm=$(size "$out/rubrica-search_bg.wasm")
loader=$(size "$out/rubrica-search.js")
echo "wasm: $wasm bytes (budget $WASM_BUDGET)"
echo "loader: $loader bytes (budget $LOADER_BUDGET)"
over=0
if [ "$wasm" -gt "$WASM_BUDGET" ]; then echo "build-wasm: the .wasm is over budget" >&2; over=1; fi
if [ "$loader" -gt "$LOADER_BUDGET" ]; then echo "build-wasm: the loader is over budget" >&2; over=1; fi
[ "$over" -eq 0 ] || exit 1

# --bundle: the bundle crate's files, its .wasm and loader as built and src/meta.rs with
# their provenance (the CLI's version and the commit). All three are git-ignored: CI builds
# them, and the crate does not compile without them.
if [ "$bundle" -eq 1 ]; then
  dest=crates/rubrica-wasm-bundle/src
  commit=$(git rev-parse --verify HEAD) || { echo "build-wasm: no commit to record (git rev-parse HEAD)" >&2; exit 2; }
  cp "$out/rubrica-search_bg.wasm" "$out/rubrica-search.js" "$dest/" ||
    { echo "build-wasm: cannot copy the bundle into $dest" >&2; exit 2; }
  {
    echo "// Written by scripts/build-wasm.sh --bundle; never committed."
    echo "/// The \`wasm-bindgen\` CLI version that wrote [\`LOADER\`]."
    echo "pub const WASM_BINDGEN_VERSION: &str = \"$locked\";"
    echo "/// The commit the bundle was built from."
    echo "pub const SOURCE_COMMIT: &str = \"$commit\";"
  } >"$dest/meta.rs" || { echo "build-wasm: cannot write $dest/meta.rs" >&2; exit 2; }
  echo "bundle: $dest (wasm-bindgen $locked, commit $commit)"
fi
echo "build-wasm: ok"
