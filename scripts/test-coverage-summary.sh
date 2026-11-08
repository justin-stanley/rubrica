#!/usr/bin/env bash
# shellcheck disable=SC2016,SC2015  # fixtures hold backticks in single quotes on purpose
# Check for scripts/coverage-summary.sh: it must turn the fixture reports in
# scripts/fixtures/coverage/ into the summary, hold each crate to its floor, and refuse every
# bad input with its exit code and message. Exit codes of the script under test: 0 fine; 1 a
# crate is below its floor (or has none); 2 bad input.
set -euo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.."
guard=$PWD/scripts/coverage-summary.sh
fx=$PWD/scripts/fixtures/coverage
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0

bad() { echo "test-coverage-summary: $*" >&2; fail=1; }

# run_ ARGS...: sets $out (stdout and stderr together) and $rc.
run_() {
  rc=0
  out=$("$guard" "$@" 2>&1) || rc=$?
}
# case_ NAME WANT_RC WANT_SUBSTRING ARGS...
case_() {
  local name=$1 want=$2 msg=$3
  shift 3
  run_ "$@"
  if [ "$rc" != "$want" ]; then bad "$name: exit $rc, want $want: $out"; return; fi
  case $out in *"$msg"*) echo "test-coverage-summary: $name ok" ;; *) bad "$name: output lacks '$msg': $out" ;; esac
}
# has_ NAME SUBSTRING: the last run's output holds the text.
has_() {
  case $out in *"$2"*) echo "test-coverage-summary: $1 ok" ;; *) bad "$1: output lacks '$2': $out" ;; esac
}
# before_ NAME FIRST SECOND: in the last run's output, FIRST's line comes before SECOND's.
before_() {
  local a b
  a=$(printf '%s\n' "$out" | grep -nF -m1 -- "$2" | cut -d: -f1 || true)
  b=$(printf '%s\n' "$out" | grep -nF -m1 -- "$3" | cut -d: -f1 || true)
  if [ -n "$a" ] && [ -n "$b" ] && [ "$a" -lt "$b" ]; then echo "test-coverage-summary: $1 ok"; else bad "$1: '$2' (line ${a:-none}) is not before '$3' (line ${b:-none})"; fi
}

# The fixture's lines coverage: rubrica 90.0, rubrica-markdown 82.0, rubrica-render 90.0,
# rubrica-site 95.0, rubrica-wasm 66.7; workspace 84.5.
floors() { # MARKDOWN_FLOOR: a floor file; the others sit well below their crates
  cat >"$tmp/floor.toml" <<TOML
# a comment
[floor]
rubrica = 85.0
rubrica-markdown = $1   # the one under test
rubrica-render = 85.0
rubrica-site = 90.0
rubrica-wasm = 60.0
TOML
}
report="$fx/normal.json"
root=/work/repo

# The normal case: exit 0, a table per crate sorted lowest first, a total row, a details
# block per crate with repo-relative paths.
floors 80.0
run_ "$report" "$tmp/floor.toml" --root "$root"
[ "$rc" = 0 ] || bad "normal: exit $rc: $out"
has_ "normal: heading" '### Coverage'
has_ "normal: header" '| Crate | Lines | Floor | Δ |'
has_ "normal: workspace row" '| **workspace** | **84.5%** |'
has_ "normal: markdown row" '| `rubrica-markdown` | 82.0% | 80.0% | +2.0 |'
has_ "normal: covered over total" '1230 / 1500'
has_ "normal: details" '<details>'
has_ "normal: details summary" '<summary><code>rubrica-site</code>'
has_ "normal: relative path" '| `crates/rubrica-site/src/build.rs` |'
case $out in *"/work/repo/"*) bad "normal: an absolute path leaked into the summary" ;; *) echo "test-coverage-summary: normal: no absolute path ok" ;; esac
before_ "normal: lowest first (wasm before markdown)" '| `rubrica-wasm` |' '| `rubrica-markdown` |'
before_ "normal: lowest first (markdown before site)" '| `rubrica-markdown` |' '| `rubrica-site` |'
before_ "normal: total row last" '| `rubrica-site` |' '| **workspace** |'

# Floors: above, at, just under (within 0.5), exactly 0.5 under, over 0.5 under.
floors 81.0; case_ "above floor" 0 '+1.0' "$report" "$tmp/floor.toml" --root "$root"
floors 82.0; case_ "at floor" 0 '| 82.0% | 82.0% | 0.0 |' "$report" "$tmp/floor.toml" --root "$root"
floors 82.4; case_ "just under floor" 0 'within 0.5' "$report" "$tmp/floor.toml" --root "$root"
floors 82.5; case_ "exactly 0.5 under floor" 0 'within 0.5' "$report" "$tmp/floor.toml" --root "$root"
floors 82.6; run_ "$report" "$tmp/floor.toml" --root "$root"
[ "$rc" = 1 ] || bad "over 0.5 under: exit $rc, want 1: $out"
has_ "over 0.5 under: summary still written" '| `rubrica-markdown` | 82.0% | 82.6% | -0.6 |'
has_ "over 0.5 under: message" 'rubrica-markdown is 0.6 points below its floor'

# A crate with no floor fails, and the summary says so.
grep -v '^rubrica-render' "$tmp/floor.toml" >"$tmp/nofloor.toml"
run_ "$report" "$tmp/nofloor.toml" --root "$root"
[ "$rc" = 1 ] || bad "missing floor: exit $rc, want 1: $out"
has_ "missing floor: message" 'rubrica-render has no floor'
has_ "missing floor: row" 'NO FLOOR'

# A crate in the floor file but absent from the report fails closed.
floors 80.0
case_ "missing crate" 2 'rubrica-site is in coverage-floor but absent from the report' "$fx/missing-crate.json" "$tmp/floor.toml" --root "$root"

# Bad input.
case_ "malformed json" 2 'not valid JSON' "$fx/malformed.json" "$tmp/floor.toml" --root "$root"
case_ "empty json" 2 'is empty' "$fx/empty.json" "$tmp/floor.toml" --root "$root"
case_ "unexpected shape" 2 'unexpected shape' "$fx/wrong-shape.json" "$tmp/floor.toml" --root "$root"
case_ "zero lines" 2 'no lines' "$fx/zero-lines.json" "$tmp/floor.toml" --root "$root"
jq '(.data[0].files[] | select(.filename | test("/crates/rubrica-wasm/"))) .summary.lines |= (.count=0|.covered=0)' "$report" >"$tmp/onezero.json"
case_ "one crate with zero lines" 2 'rubrica-wasm has no lines' "$tmp/onezero.json" "$tmp/floor.toml" --root "$root"
jq '.data[0].files[0].summary.lines.covered = 999' "$report" >"$tmp/over.json"
case_ "covered above count" 2 'unexpected shape' "$tmp/over.json" "$tmp/floor.toml" --root "$root"
jq '.data[0].files[0].filename = "/elsewhere/x.rs"' "$report" >"$tmp/outside.json"
case_ "file outside the repo" 2 'is not under' "$tmp/outside.json" "$tmp/floor.toml" --root "$root"
case_ "no such report" 2 'cannot read' "$tmp/nope.json" "$tmp/floor.toml" --root "$root"
case_ "no such floor file" 2 'cannot read' "$report" "$tmp/nope.toml" --root "$root"
printf '[floor]\nrubrica = 9x\n' >"$tmp/badfloor.toml"
case_ "bad floor line" 2 'line 2' "$report" "$tmp/badfloor.toml" --root "$root"
printf '[floor]\nrubrica = 80.0\nrubrica = 81.0\n' >"$tmp/dupfloor.toml"
case_ "duplicate floor" 2 'twice' "$report" "$tmp/dupfloor.toml" --root "$root"
printf '[floor]\n' >"$tmp/emptyfloor.toml"
case_ "no floors at all" 2 'no floors' "$report" "$tmp/emptyfloor.toml" --root "$root"
case_ "no arguments" 2 'usage' 

# The badge: the shields.io endpoint format, with the band for the workspace's lines.
floors 80.0
run_ "$report" "$tmp/floor.toml" --root "$root" --badge "$tmp/badge.json"
[ "$rc" = 0 ] || bad "badge: exit $rc: $out"
want='{"schemaVersion":1,"label":"coverage","message":"84.5%","color":"green"}'
[ "$(cat "$tmp/badge.json" 2>/dev/null)" = "$want" ] && echo "test-coverage-summary: badge ok" || bad "badge: got '$(cat "$tmp/badge.json" 2>/dev/null)', want $want"
# Bands: 90 brightgreen, 80 green, 70 yellowgreen, 60 yellow, 50 orange, below red.
for pair in 95:brightgreen 90:brightgreen 89.9:green 80:green 79.9:yellowgreen 70:yellowgreen 69.9:yellow 60:yellow 59.9:orange 50:orange 49.9:red 0:red; do
  pct=${pair%%:*} color=${pair##*:}
  jq --argjson p "$pct" '.data[0].files |= (map(select(.filename|test("/crates/rubrica-site/"))) | .[0:1] | map(.summary.lines |= (.count=1000|.covered=($p*10|round))))' "$report" >"$tmp/band.json"
  printf '[floor]\nrubrica-site = 0.0\n' >"$tmp/bandfloor.toml"
  run_ "$tmp/band.json" "$tmp/bandfloor.toml" --root "$root" --badge "$tmp/band-badge.json"
  got=$(jq -r .color "$tmp/band-badge.json" 2>/dev/null || echo none)
  [ "$got" = "$color" ] && echo "test-coverage-summary: band $pct ok" || bad "band $pct: colour '$got', want $color ($out)"
done
# No badge is written when the input is bad.
rm -f "$tmp/badge.json"
run_ "$fx/malformed.json" "$tmp/floor.toml" --root "$root" --badge "$tmp/badge.json"
[ ! -e "$tmp/badge.json" ] && echo "test-coverage-summary: no badge on bad input ok" || bad "a badge was written for bad input"

# The real floor file names every crate the workspace measures.
[ -f coverage-floor.toml ] || bad "coverage-floor.toml is missing"

if [ "$fail" -ne 0 ]; then exit 1; fi
echo "test-coverage-summary: all green"
