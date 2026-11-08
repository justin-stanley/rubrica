#!/usr/bin/env bash
# The checks on the search bundle that need no build: what CI's `wasm` job runs on a bundle it
# restored from its cache instead of building (a miss builds it, and scripts/wasm-reproducible.sh
# proves the build).
#
#   wasm-bundle-check.sh --write [DIR [MANIFEST]]   after a build that passed every check:
#                                                    record the sha256 of the three files
#   wasm-bundle-check.sh --verify [DIR [MANIFEST]]   on restored files: all three exist, the
#                                                    .wasm and the loader are within the size
#                                                    budget of scripts/build-wasm.sh (read from
#                                                    it, not copied), their sha256 are the ones
#                                                    recorded, and meta.rs names the
#                                                    wasm-bindgen version Cargo.lock pins
#
# DIR is crates/rubrica-wasm-bundle/src and MANIFEST target/wasm-bundle.sha256 unless given.
# The manifest travels in the cache beside the files, so a damaged or half-restored entry fails
# here rather than reaching the jobs that embed the bundle.
#
# Exit codes: 0 ok; 1 a check failed; 2 a usage error or a missing file.
set -euo pipefail
export LC_ALL=C

mode=${1:-}
case $mode in --write | --verify) ;; *) echo "wasm-bundle-check: usage: $0 --write|--verify [DIR [MANIFEST]]" >&2; exit 2 ;; esac
[ "$#" -le 3 ] || { echo "wasm-bundle-check: too many arguments" >&2; exit 2; }
cd "$(dirname "$0")/.." || { echo "wasm-bundle-check: cannot enter the repository root" >&2; exit 2; }
dir=${2:-crates/rubrica-wasm-bundle/src}
manifest=${3:-target/wasm-bundle.sha256}
files="rubrica-search_bg.wasm rubrica-search.js meta.rs"

sha256_of() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

for f in $files; do
  [ -f "$dir/$f" ] || { echo "wasm-bundle-check: no $dir/$f" >&2; exit 2; }
done

if [ "$mode" = --write ]; then
  mkdir -p "$(dirname "$manifest")"
  for f in $files; do echo "$(sha256_of "$dir/$f")  $f"; done >"$manifest"
  echo "wasm-bundle-check: recorded in $manifest"
  exit 0
fi

fail=0
bad() { echo "wasm-bundle-check: FAIL: $*" >&2; fail=1; }

# The budgets are scripts/build-wasm.sh's own.
budget() {
  local v
  v=$(sed -n "s/^$1=\([0-9][0-9]*\) .*/\1/p" scripts/build-wasm.sh)
  [ -n "$v" ] || { echo "wasm-bundle-check: no $1 in scripts/build-wasm.sh" >&2; exit 2; }
  echo "$v"
}
wasm_budget=$(budget WASM_BUDGET)
loader_budget=$(budget LOADER_BUDGET)
wasm=$(wc -c <"$dir/rubrica-search_bg.wasm" | tr -d ' ')
loader=$(wc -c <"$dir/rubrica-search.js" | tr -d ' ')
echo "wasm: $wasm bytes (budget $wasm_budget)"
echo "loader: $loader bytes (budget $loader_budget)"
[ "$wasm" -le "$wasm_budget" ] || bad "the .wasm is over budget"
[ "$loader" -le "$loader_budget" ] || bad "the loader is over budget"
[ "$wasm" -gt 0 ] && [ "$loader" -gt 0 ] || bad "an empty bundle file"

[ -f "$manifest" ] || { echo "wasm-bundle-check: no manifest $manifest" >&2; exit 2; }
for f in $files; do
  want=$(sed -n "s/^\([0-9a-f]\{64\}\)  $f\$/\1/p" "$manifest")
  [ -n "$want" ] || { bad "$f is not in $manifest"; continue; }
  got=$(sha256_of "$dir/$f")
  [ "$got" = "$want" ] || bad "$f is $got, the cache recorded $want"
done

locked=$(awk '$0 == "name = \"wasm-bindgen\"" { getline; gsub(/^version = "|"$/, ""); print; exit }' Cargo.lock)
[ -n "$locked" ] || { echo "wasm-bundle-check: no wasm-bindgen version in Cargo.lock" >&2; exit 2; }
recorded=$(sed -n 's/^pub const WASM_BINDGEN_VERSION: &str = "\(.*\)";$/\1/p' "$dir/meta.rs")
[ "$recorded" = "$locked" ] || bad "meta.rs says wasm-bindgen '$recorded', Cargo.lock pins '$locked'"

[ "$fail" -eq 0 ] || exit 1
echo "wasm-bundle-check: ok"
