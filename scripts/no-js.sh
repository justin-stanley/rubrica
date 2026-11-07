#!/usr/bin/env bash
# The no-script gate: nothing Rubrica builds can run script. It runs the auditor's tests
# (crates/rubrica/tests/no_js.rs) over the project's documentation site, a hostile site and
# a site made of every specification example, and, where RUBRICA_NO_JS_SITE names a
# directory, over that built site too.
#
# Exit codes: 0 clean; 1 a finding; 2 a usage error or a run that did not do its job (an
# expected test missing from the output). Fails closed: the test run's status is read, and
# so is the presence of every test it must have run.
set -euo pipefail
export LC_ALL=C

if [ "$#" -ne 0 ]; then
  echo "no-js: takes no arguments (name a built site with RUBRICA_NO_JS_SITE)" >&2
  exit 2
fi
cd "$(dirname "$0")/.." || { echo "no-js: cannot enter the repository root" >&2; exit 2; }

rc=0
out=$(cargo test --locked -p rubrica --test no_js 2>&1) || rc=$?
if [ "$rc" -ne 0 ]; then
  printf '%s\n' "$out"
  echo "no-js: failed (cargo test exited $rc)"
  exit 1
fi
for t in a_hostile_site_has_no_script a_site_with_a_redirect_has_no_script \
  an_external_site_has_no_script the_audit_finds_a_planted_script \
  the_audit_flags_a_refresh_that_leaves_the_site the_docs_site_has_no_script \
  the_spec_site_has_no_script; do
  rc=0
  grep -qxF "test $t ... ok" <<<"$out" || rc=$?
  case $rc in
    0) ;;
    1) printf '%s\n' "$out"; echo "no-js: test $t did not run and pass" >&2; exit 2 ;;
    *) echo "no-js: grep failed ($rc)" >&2; exit 2 ;;
  esac
done
echo "no-js: clean"
