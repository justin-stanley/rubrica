#!/usr/bin/env bash
# Stages the GitHub Pages site for the coverage report:
#
#   scripts/coverage-site.sh HTML_DIR BADGE.json OUT_DIR
#
# OUT_DIR (it must not exist) gets index.html (.github/pages/index.html, a static page that
# links to coverage/), coverage/ (HTML_DIR, cargo-llvm-cov's report) and coverage/badge.json
# (the shields.io endpoint file scripts/coverage-summary.sh wrote). Rubrica ships no script of
# its own, so llvm-cov's one (control.js, keyboard navigation) is removed, and the staged site
# is searched for any script, event handler or javascript: URL left: one is a failure.
#
# Exit codes: 0 staged; 1 a script is left in the site; 2 bad input or usage.
set -euo pipefail
export LC_ALL=C

die() { echo "coverage-site: $1" >&2; exit "${2:-2}"; }

[ "$#" -eq 3 ] || die "usage: coverage-site.sh HTML_DIR BADGE.json OUT_DIR"
html=$1 badge=$2 out=$3
root=$(cd "$(dirname "$0")/.." && pwd -P)
landing=$root/.github/pages/index.html

[ -d "$html" ] || die "cannot read the report directory '$html'"
[ -f "$html/index.html" ] || die "the report directory '$html' has no index.html"
[ -f "$badge" ] || die "cannot read the badge '$badge'"
jq -e '.schemaVersion == 1 and (.label | type == "string") and (.message | type == "string") and (.color | type == "string")' "$badge" >/dev/null 2>&1 \
  || die "the badge '$badge' is not a shields.io endpoint file (schemaVersion 1, label, message, color)"
[ -f "$landing" ] || die "the landing page '$landing' is missing"
[ ! -e "$out" ] || die "the output directory '$out' already exists"

mkdir -p "$out/coverage"
cp "$landing" "$out/index.html"
cp -R "$html"/. "$out/coverage/"
cp "$badge" "$out/coverage/badge.json"

# llvm-cov's pages each load control.js with one tag, and link to its functions with
# javascript: URLs in a "control" span (next uncovered line, region, branch). Take the tags and
# the span out, then the file.
tag="<script src='[^']*control\.js'></script>"
span="<span class='control'>\(<a href='javascript:[^']*'>[^<]*</a>,\{0,1\} \{0,1\}\)*</span>"
while IFS= read -r -d '' f; do
  sed -e "s#$tag##g" -e "s#$span##g" "$f" >"$f.tmp"
  mv "$f.tmp" "$f"
done < <(find "$out/coverage" -name '*.html' -print0)
find "$out/coverage" -name control.js -type f -delete

# Nothing may run: any script tag, event-handler attribute or javascript: URL, inside a tag, is
# a failure. (The report's source listings are HTML-escaped, so code text never matches.)
rc=0
left=$(grep -rIliE -e '<script|<[a-z][^>]*[[:space:]]on[a-z]+[[:space:]]*=|(href|src|action)[[:space:]]*=[[:space:]]*.?[[:space:]]*javascript:' "$out" 2>&1) || rc=$?
case $rc in
  0) printf '%s\n' "$left" | head -5 >&2; die "a script is left in the staged site (the files above)" 1 ;;
  1) ;;
  *) die "grep failed ($rc): $left" ;;
esac
echo "coverage-site: staged $out ($(find "$out" -type f | wc -l | tr -d ' ') files, no script)"
