#!/usr/bin/env bash
# The local edits of crates/rubrica-markdown's vendored files, as unified diffs against the
# upstream pulldown-cmark sources cargo unpacked into the registry. Reviewers read it; the
# re-sync lane (the weekly upstream-parser workflow) ports upstream changes by hand after
# reading it; never merged blind.
#
# Everything comes from crates/rubrica-markdown/UPSTREAM: the version from its first line
# (`pulldown-cmark <ver>`) and the file list from its `vendored:` line (the `src/<f>` paths
# after the colon). No list is hard-coded here: a package that vendors or drops a file edits
# UPSTREAM only. The hunks are the output, not a failure: the script exits 0 whatever `diff`
# returns, and 2 only when UPSTREAM or the unpacked upstream source cannot be read.
set -euo pipefail
cd "$(dirname "$0")/.."

record=crates/rubrica-markdown/UPSTREAM
[ -r "$record" ] || { echo "upstream-diff: cannot read $record" >&2; exit 2; }
first=$(head -n 1 "$record")
case $first in
  'pulldown-cmark '*) ver=${first#pulldown-cmark } ;;
  *) echo "upstream-diff: the first line of $record is not 'pulldown-cmark <ver>'" >&2; exit 2 ;;
esac
case $ver in
  *[!0-9.]*|'') echo "upstream-diff: '$ver' is not a version" >&2; exit 2 ;;
esac
files=$(LC_ALL=C awk '/^vendored:/ { sub(/^vendored:/, ""); print }' "$record")
[ -n "${files// /}" ] || { echo "upstream-diff: no 'vendored:' line in $record" >&2; exit 2; }

upstream=
for d in "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/"pulldown-cmark-$ver"; do
  [ -d "$d" ] && { upstream=$d; break; }
done
[ -n "$upstream" ] || {
  echo "upstream-diff: pulldown-cmark-$ver is not unpacked under ${CARGO_HOME:-$HOME/.cargo}/registry/src (cargo fetch unpacks it)" >&2
  exit 2
}

for f in $files; do
  [ -f "$upstream/$f" ] || { echo "upstream-diff: $upstream/$f is missing" >&2; exit 2; }
  diff -u "$upstream/$f" "crates/rubrica-markdown/$f" || true
done
