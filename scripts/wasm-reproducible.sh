#!/usr/bin/env bash
# The search bundle is reproducible: two builds of it (scripts/build-wasm.sh), one in the
# checkout's target/ and one in a fresh temporary target directory, give the same bytes,
# and those bytes are the files the bundle crate holds (crates/rubrica-wasm-bundle/src/), so
# a hand edit of the loader or a stale bundle fails. The second build sets CARGO_TARGET_DIR,
# here and for that one command only: nothing else in the repository sets it, and the gate
# refuses a shared target directory.
#
# Exit codes: 0 identical; 1 a difference; 2 a usage error, a missing bundle or a failed
# build.
set -euo pipefail
export LC_ALL=C

if [ "$#" -ne 0 ]; then
  echo "wasm-reproducible: takes no arguments" >&2
  exit 2
fi
cd "$(dirname "$0")/.." || { echo "wasm-reproducible: cannot enter the repository root" >&2; exit 2; }

bundle=crates/rubrica-wasm-bundle/src
files="rubrica-search_bg.wasm rubrica-search.js"
tmp=$(mktemp -d "${TMPDIR:-/tmp}/wasm-reproducible.XXXXXX") ||
  { echo "wasm-reproducible: mktemp failed" >&2; exit 2; }
trap 'rm -rf "$tmp"' EXIT

# same A B: 0 the same bytes, 1 different; a cmp error (a missing file) fails the script.
same() {
  local rc=0
  cmp -s "$1" "$2" || rc=$?
  case $rc in
    0 | 1) return "$rc" ;;
    *) echo "wasm-reproducible: cmp failed ($rc) on $1 and $2" >&2; exit 2 ;;
  esac
}

rc=0
env -u CARGO_TARGET_DIR scripts/build-wasm.sh >"$tmp/first.log" 2>&1 || rc=$?
if [ "$rc" -ne 0 ]; then
  cat "$tmp/first.log" >&2
  echo "wasm-reproducible: the first build failed ($rc)" >&2
  exit 2
fi
rc=0
CARGO_TARGET_DIR="$tmp/target" scripts/build-wasm.sh >"$tmp/second.log" 2>&1 || rc=$?
if [ "$rc" -ne 0 ]; then
  cat "$tmp/second.log" >&2
  echo "wasm-reproducible: the second build failed ($rc)" >&2
  exit 2
fi

differ=0
for f in $files; do
  same "target/wasm-bindgen/$f" "$tmp/target/wasm-bindgen/$f" ||
    { echo "wasm-reproducible: $f differ between two builds" >&2; differ=1; }
done
[ "$differ" -eq 0 ] || exit 1

for f in $files; do
  if [ ! -f "$bundle/$f" ]; then
    echo "wasm-reproducible: no $bundle/$f: run scripts/build-wasm.sh --bundle first" >&2
    exit 2
  fi
  same "target/wasm-bindgen/$f" "$bundle/$f" ||
    { echo "wasm-reproducible: $bundle/$f is not what the build writes" >&2; differ=1; }
done
[ "$differ" -eq 0 ] || exit 1

sha256_of() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}
for f in $files; do
  echo "wasm-reproducible: $(sha256_of "$bundle/$f")  $f"
done
echo "wasm-reproducible: identical"
