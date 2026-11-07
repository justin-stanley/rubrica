#!/usr/bin/env bash
# Check for scripts/no-js.sh: it must pass the real tree and refuse each bad case, with its
# exact exit code and message. The bad sites are scratch directories named by
# RUBRICA_NO_JS_SITE; the cases about the test run itself put a stand-in `cargo` first on
# PATH.
set -euo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.."
guard=$PWD/scripts/no-js.sh
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0

bad() { echo "test-no-js: $*" >&2; fail=1; }

# case_ NAME EXPECTED_RC EXPECTED_SUBSTRING [VAR=VALUE]... -- [ARG]...
case_() {
  local name=$1 want=$2 msg=$3 out rc=0
  shift 3
  local envs=()
  while [ "$#" -gt 0 ] && [ "$1" != -- ]; do envs+=("$1"); shift; done
  [ "$#" -eq 0 ] || shift
  out=$(env "${envs[@]+"${envs[@]}"}" "$guard" "$@" 2>&1) || rc=$?
  if [ "$rc" != "$want" ]; then bad "$name: exit $rc, want $want: $out"; return; fi
  case $out in *"$msg"*) echo "test-no-js: $name ok" ;; *) bad "$name: message lacks '$msg': $out" ;; esac
}

csp="<meta http-equiv=\"Content-Security-Policy\" content=\"script-src 'none'; connect-src 'none'\">"
site() { mkdir -p "$tmp/$1"; printf '<!doctype html><html><head>%s</head><body>%s</body></html>' "$2" "$3" >"$tmp/$1/index.html"; }

case_ clean 0 'no-js: clean' --

case_ argument 2 'takes no arguments' -- some-dir

site good "$csp" '<p>fine</p>'
case_ external-clean 0 'no-js: clean' "RUBRICA_NO_JS_SITE=$tmp/good" --

site script "$csp" '<script>alert(1)</script>'
case_ external-script 1 'no-js: failed' "RUBRICA_NO_JS_SITE=$tmp/script" --

site handler "$csp" '<a href="/x" onclick="y()">x</a>'
case_ external-handler 1 'no-js: failed' "RUBRICA_NO_JS_SITE=$tmp/handler" --

site url "$csp" '<a href="javascript:alert(1)">x</a>'
case_ external-script-url 1 'no-js: failed' "RUBRICA_NO_JS_SITE=$tmp/url" --

site nocsp '' '<p>no policy</p>'
case_ external-no-csp 1 'no-js: failed' "RUBRICA_NO_JS_SITE=$tmp/nocsp" --

site weakcsp '<meta http-equiv="Content-Security-Policy" content="default-src *">' '<p>x</p>'
case_ external-weak-csp 1 'no-js: failed' "RUBRICA_NO_JS_SITE=$tmp/weakcsp" --

site jsfile "$csp" '<p>x</p>'
echo 'alert(1)' >"$tmp/jsfile/app.js"
case_ external-js-file 1 'no-js: failed' "RUBRICA_NO_JS_SITE=$tmp/jsfile" --

case_ external-missing 1 'no-js: failed' "RUBRICA_NO_JS_SITE=$tmp/does-not-exist" --
case_ external-empty-name 1 'no-js: failed' "RUBRICA_NO_JS_SITE=" --

# A stand-in cargo: one that fails, and one that succeeds without running the tests.
mkdir -p "$tmp/failing" "$tmp/silent"
printf '#!/bin/sh\necho boom\nexit 101\n' >"$tmp/failing/cargo"
printf '#!/bin/sh\necho "test result: ok. 0 passed"\nexit 0\n' >"$tmp/silent/cargo"
chmod +x "$tmp/failing/cargo" "$tmp/silent/cargo"
case_ cargo-fails 1 'no-js: failed (cargo test exited 101)' "PATH=$tmp/failing:$PATH" --
case_ tests-did-not-run 2 'did not run and pass' "PATH=$tmp/silent:$PATH" --

[ "$fail" = 0 ] || exit 1
echo "test-no-js: all cases ok"
