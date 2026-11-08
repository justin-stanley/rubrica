#!/usr/bin/env bash
# Builds one repository with two `rubrica` binaries and compares the two sites byte for byte.
#
#   tools/render-diff.sh <bin-a> <bin-b> <repo-dir> [--subtree DIR] [--config FILE]
#
# Each binary runs `build <repo-dir> <scratch>/<a|b> [--subtree DIR] [--config FILE]` into its
# own scratch directory. The outputs are compared with `diff -rq`, leaving out `_health`
# (it names the binary's version and commit). The differing files are listed, then the
# summary line `render-diff: N files differ`.
#
# Exit codes: 0 the sites are identical; 1 they differ; 2 a usage error, an unusable
# argument, or a build or comparison that did not run. Fails closed: every status is read.
set -euo pipefail
export LC_ALL=C

usage() {
  echo "usage: render-diff.sh <bin-a> <bin-b> <repo-dir> [--subtree DIR] [--config FILE]" >&2
  exit 2
}
fail() {
  echo "render-diff: $1" >&2
  exit 2
}

pos=()
subtree=""
config=""
have_subtree=0
have_config=0
while [ "$#" -gt 0 ]; do
  case $1 in
    --subtree | --config)
      opt=$1
      [ "$#" -ge 2 ] || fail "$opt needs a value"
      if [ "$opt" = --subtree ]; then
        [ "$have_subtree" -eq 0 ] || fail "--subtree given twice"
        have_subtree=1
        subtree=$2
      else
        [ "$have_config" -eq 0 ] || fail "--config given twice"
        have_config=1
        config=$2
      fi
      shift 2
      ;;
    -*) fail "unknown option $1" ;;
    *)
      pos+=("$1")
      shift
      ;;
  esac
done
[ "${#pos[@]}" -eq 3 ] || usage
bin_a=${pos[0]}
bin_b=${pos[1]}
repo=${pos[2]}

for b in "$bin_a" "$bin_b"; do
  if [ ! -f "$b" ] || [ ! -x "$b" ]; then
    fail "not an executable file: $b"
  fi
done
[ -d "$repo" ] || fail "not a directory: $repo"
if [ "$have_subtree" -eq 1 ] && [ -z "$subtree" ]; then fail "--subtree needs a value"; fi
if [ "$have_config" -eq 1 ] && [ ! -f "$config" ]; then fail "--config is not a file: $config"; fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/render-diff.XXXXXX") || fail "cannot make a scratch directory"
cleanup() { rm -rf "$tmp"; }
trap cleanup EXIT
# The real path: `rubrica build` refuses a symbolic link on the way (macOS's /var).
tmp=$(cd "$tmp" && pwd -P) || fail "cannot resolve the scratch directory"

opts=()
[ "$have_subtree" -eq 0 ] || opts+=(--subtree "$subtree")
[ "$have_config" -eq 0 ] || opts+=(--config "$config")

build_one() { # <bin> <out>
  local rc=0
  "$1" build "$repo" "$2" ${opts[@]+"${opts[@]}"} >"$tmp/build.log" 2>&1 || rc=$?
  if [ "$rc" -ne 0 ]; then
    cat "$tmp/build.log" >&2
    fail "build failed ($1 exited $rc)"
  fi
}
build_one "$bin_a" "$tmp/a"
build_one "$bin_b" "$tmp/b"

rc=0
listing=$(diff -rq -x _health "$tmp/a" "$tmp/b") || rc=$?
case $rc in
  0) count=0 ;;
  1) count=$(printf '%s\n' "$listing" | wc -l | tr -d ' ') ;;
  *) fail "diff failed ($rc)" ;;
esac
if [ "$count" -gt 0 ]; then
  printf '%s\n' "$listing"
fi
echo "render-diff: $count files differ"
[ "$count" -eq 0 ] || exit 1
