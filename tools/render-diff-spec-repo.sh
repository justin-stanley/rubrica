#!/usr/bin/env bash
# Writes a git repository with one page per specification example into a directory:
#
#   tools/render-diff-spec-repo.sh <dir>
#
# `commonmark/ex-<n>.md` and `gfm/ex-<n>.md` (n from 1) each hold front matter with the title
# `Example n`, then the example's Markdown verbatim; `home.md` is the home page. The two
# example files are crates/rubrica-markdown/tests/spec/*.json. The commit's identity and dates
# are fixed, so two runs write the same commit.
#
# Exit codes: 0 written; 2 a usage error, an unusable target, or a step that failed. The
# target must not exist or must be empty.
set -euo pipefail
export LC_ALL=C

fail() {
  echo "render-diff-spec-repo: $1" >&2
  exit 2
}

[ "$#" -eq 1 ] || fail "usage: render-diff-spec-repo.sh <dir>"
dir=$1
[ -n "$dir" ] || fail "the directory is empty"
if [ -e "$dir" ] || [ -L "$dir" ]; then
  [ -d "$dir" ] && [ ! -L "$dir" ] || fail "not a directory: $dir"
  rc=0
  entries=$(ls -A "$dir") || rc=$?
  [ "$rc" -eq 0 ] || fail "cannot list $dir"
  [ -z "$entries" ] || fail "not empty: $dir"
fi
case $dir in /*) ;; *) dir=$PWD/$dir ;; esac

here=$(cd "$(dirname "$0")" && pwd -P) || fail "cannot find the script directory"
root=$(git -C "$here" rev-parse --show-toplevel) || fail "not inside a checkout"
spec="$root/crates/rubrica-markdown/tests/spec"
for f in commonmark-0.31.2.json gfm-0.29.json; do
  [ -f "$spec/$f" ] || fail "missing $spec/$f"
done
command -v python3 >/dev/null || fail "python3 is required"

mkdir -p "$dir" || fail "cannot make $dir"
python3 - "$spec" "$dir" <<'PY' || fail "writing the pages failed"
import json
import os
import sys

spec, out = sys.argv[1], sys.argv[2]


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(text.encode("utf-8"))


write(os.path.join(out, "home.md"), '---\ntitle: "Home"\n---\n# Home\n')
for tag, name in (("commonmark", "commonmark-0.31.2.json"), ("gfm", "gfm-0.29.json")):
    with open(os.path.join(spec, name), "rb") as f:
        examples = json.load(f)
    for i, ex in enumerate(examples, start=1):
        head = "---\ntitle: %s\n---\n" % json.dumps("Example %d" % i)
        write(os.path.join(out, tag, "ex-%d.md" % i), head + ex["markdown"])
PY

g() { git -C "$dir" -c user.name=spec -c user.email=spec@example.invalid -c commit.gpgsign=false "$@"; }
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_DATE='2026-01-01T00:00:00+0000' GIT_COMMITTER_DATE='2026-01-01T00:00:00+0000'
g init -q || fail "git init failed"
g checkout -q -b main || fail "git checkout failed"
g add -A || fail "git add failed"
g commit -q -m "specification examples" || fail "git commit failed"
