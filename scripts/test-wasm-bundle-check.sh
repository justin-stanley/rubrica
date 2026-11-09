#!/usr/bin/env bash
# Tests scripts/wasm-bundle-check.sh: a bundle that is whole passes, and each way a restored
# cache entry can be wrong fails with its exit code. Runs on scratch copies, so the real
# bundle never matters.
set -euo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.."
guard=scripts/wasm-bundle-check.sh
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { echo "test-wasm-bundle-check: $*" >&2; fail=1; }

locked=$(awk '$0 == "name = \"wasm-bindgen\"" { getline; gsub(/^version = "|"$/, ""); print; exit }' Cargo.lock)
# fresh NAME: a scratch bundle directory and its manifest, as a build then --write leaves them.
fresh() {
  d=$tmp/$1
  mkdir -p "$d"
  printf 'wasm bytes' >"$d/rubrica-search_bg.wasm"
  printf 'loader text' >"$d/rubrica-search.js"
  printf 'pub const WASM_BINDGEN_VERSION: &str = "%s";\n' "$locked" >"$d/meta.rs"
  "$guard" --write "$d" "$d.sha256" >/dev/null
}
# expect NAME WANT_RC: --verify on that scratch bundle exits WANT_RC.
expect() {
  local rc=0
  "$guard" --verify "$tmp/$1" "$tmp/$1.sha256" >/dev/null 2>&1 || rc=$?
  [ "$rc" -eq "$2" ] || bad "$1: exit $rc, want $2"
}

fresh whole;        expect whole 0
fresh flipped;      printf 'wasm bytez' >"$tmp/flipped/rubrica-search_bg.wasm"; expect flipped 1
fresh loader-edit;  printf 'loader TEXT' >"$tmp/loader-edit/rubrica-search.js"; expect loader-edit 1
fresh big-loader;   head -c 40000 /dev/zero | tr '\0' 'x' >"$tmp/big-loader/rubrica-search.js"
"$guard" --write "$tmp/big-loader" "$tmp/big-loader.sha256" >/dev/null; expect big-loader 1
fresh old-cli;      printf 'pub const WASM_BINDGEN_VERSION: &str = "0.0.1";\n' >"$tmp/old-cli/meta.rs"
"$guard" --write "$tmp/old-cli" "$tmp/old-cli.sha256" >/dev/null; expect old-cli 1
fresh unlisted;     grep -v 'meta.rs' "$tmp/unlisted.sha256" >"$tmp/unlisted.new"; mv "$tmp/unlisted.new" "$tmp/unlisted.sha256"; expect unlisted 1
fresh no-wasm;      rm "$tmp/no-wasm/rubrica-search_bg.wasm"; expect no-wasm 2
fresh no-manifest;  rm "$tmp/no-manifest.sha256"; expect no-manifest 2
fresh empty-wasm;   : >"$tmp/empty-wasm/rubrica-search_bg.wasm"
"$guard" --write "$tmp/empty-wasm" "$tmp/empty-wasm.sha256" >/dev/null; expect empty-wasm 1
rc=0; "$guard" >/dev/null 2>&1 || rc=$?; [ "$rc" -eq 2 ] || bad "no mode: exit $rc, want 2"

[ "$fail" -eq 0 ] || exit 1
echo "test-wasm-bundle-check: ok"
