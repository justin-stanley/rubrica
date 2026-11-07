#!/usr/bin/env bash
# Print the release notes for VERSION on stdout: the body of its CHANGELOG.md section, without
# the heading, with leading and trailing blank lines dropped.
#
#   scripts/release-notes.sh 0.1.0 [CHANGELOG.md]
#
# A final version (X.Y.Z) needs its own `## [X.Y.Z] - YYYY-MM-DD` section. A release candidate
# (X.Y.Z-rc.N) uses `## [X.Y.Z-rc.N]` when there is one, and otherwise the `## Unreleased`
# section, since a candidate is cut before the changelog is dated.
#
# Exit codes: 0 the notes were printed; 1 no such section, or it is empty (nothing is printed);
# 2 a usage error (a version that is not X.Y.Z or X.Y.Z-rc.N, such as one with a leading v).
set -euo pipefail
export LC_ALL=C

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "release-notes: usage: release-notes.sh VERSION [CHANGELOG.md]" >&2
  exit 2
fi
version=$1
changelog=${2:-CHANGELOG.md}

if ! printf '%s\n' "$version" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?$'; then
  echo "release-notes: '$version' is not X.Y.Z or X.Y.Z-rc.N (no leading v)" >&2
  exit 2
fi
if [ ! -f "$changelog" ]; then
  echo "release-notes: $changelog does not exist" >&2
  exit 1
fi

heading=$(awk -v p="## [$version]" '
  index($0, p) == 1 { rest = substr($0, length(p) + 1); if (rest == "" || substr(rest, 1, 1) == " ") { print; exit } }
' "$changelog")
case "$version" in
  *-rc.*)
    if [ -z "$heading" ]; then
      heading=$(awk '$0 == "## Unreleased" || $0 == "## [Unreleased]" { print; exit }' "$changelog")
    fi ;;
esac
if [ -z "$heading" ]; then
  echo "release-notes: no section for $version in $changelog" >&2
  exit 1
fi

# The section: every line after the heading up to the next `## ` heading, trimmed of blank
# lines (and a closing horizontal rule) at both ends.
body=$(awk -v h="$heading" '
  $0 == h && !seen { on = 1; seen = 1; next }
  on && /^## / { exit }
  on { lines[++n] = $0 }
  END {
    first = 1; while (first <= n && lines[first] ~ /^[ \t]*$/) first++
    last = n; while (last >= first && lines[last] ~ /^[ \t]*$|^---+[ \t]*$/) last--
    for (i = first; i <= last; i++) print lines[i]
  }
' "$changelog")
if [ -z "$body" ]; then
  echo "release-notes: the section '$heading' in $changelog is empty" >&2
  exit 1
fi
printf '%s\n' "$body"
