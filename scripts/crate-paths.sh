#!/usr/bin/env bash
# Guard: no crate reads a file outside itself, and every file a crate reads is a file it
# ships. Checks, in order: nothing under crates/ is a symlink or a submodule (git grep and
# the package list would not see through one); no crate runs node; no string literal climbs
# out of a crate into a top-level fixtures/, tools/ or vendor/ directory; and for every
# workspace crate, each include!/include_str!/include_bytes! literal and each
# concat!(env!("CARGO_MANIFEST_DIR"), "...") literal in a shipped src/ or tests/ file names a
# file that `cargo package --list` holds. scripts/test-crate-paths.sh tests it.
#
# Exit codes: 0 clean; 1 a finding; 2 an error, or a guard that cannot cover what it names.
# Every grep, awk, cut and sort runs under LC_ALL=C; a grep's status is read: 0 is hits, 1
# none, 2 and up an error that fails the guard.
set -euo pipefail
cd "$(dirname "$0")/.."

crate_glob='crates/**'

# tracked PATHSPEC...: the tracked files they match, one per line.
tracked() {
  LC_ALL=C git -c core.quotePath=false ls-files -- "$@" || {
    echo "crate-paths: git ls-files failed" >&2
    exit 2
  }
}

# A guarded path that matches no tracked file narrows coverage silently (a rename): refuse.
listed=$(tracked "$crate_glob")
if [ -z "$listed" ]; then
  echo "crate-paths: a guarded path matches no file: $crate_glob" >&2
  exit 2
fi
# git grep reads a symlink's link text, not its target, and does not descend into a
# submodule (mode 160000): either under crates/ hides content.
modes=$(LC_ALL=C git ls-files -s -- "$crate_glob") || {
  echo "crate-paths: git ls-files failed" >&2
  exit 2
}
set +e
printf '%s\n' "$modes" | LC_ALL=C awk '$1=="120000" || $1=="160000" {f=1} END{exit !f}'
rc=$?
set -e
if [ "$rc" -eq 0 ]; then
  echo "crate-paths: a guarded path holds a symlink or a submodule" >&2
  exit 2
elif [ "$rc" -ne 1 ]; then
  echo "crate-paths: symlink check failed ($rc)" >&2
  exit 2
fi

fail=0
# Node: nothing under crates/ runs it.
set +e
raw=$(LC_ALL=C grep -rnFa 'Command::new("node")' crates)
rc=$?
set -e
if [ "$rc" -ge 2 ]; then
  echo "crate-paths: grep failed ($rc)" >&2
  exit 2
fi
if [ "$rc" -eq 0 ]; then
  echo "crate-paths: node runs under crates/:" >&2
  printf '%s\n' "$raw" | LC_ALL=C cut -d: -f1,2 >&2
  fail=1
fi

# Every read stays within the crate: no ../fixtures, ../tools, ../vendor, with no
# exception. This catches include_str! and concat! paths that climb out.
set +e
raw=$(LC_ALL=C grep -rnE '"/?(\.\./)+(fixtures|tools|vendor)(/|")' crates --include='*.rs')
rc=$?
set -e
if [ "$rc" -ge 2 ]; then
  echo "crate-paths: reads-within-crate grep failed ($rc)" >&2
  exit 2
fi
if [ "$rc" -eq 0 ]; then
  echo "crate-paths: every read must stay within its crate:" >&2
  printf '%s\n' "$raw" | LC_ALL=C cut -d: -f1,2 >&2
  fail=1
fi

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

# What a crate reads is what it ships. For every workspace crate, `cargo package --list`
# is the set of files a published copy holds; every `include_bytes!`, `include_str!` and
# `include!` literal and every `concat!(env!("CARGO_MANIFEST_DIR"), "/...")` literal in a
# shipped `src/` or `tests/` file must name a file in that set (or a directory with a file
# in it). A path is resolved against the file (`include*!`) or the crate root (the manifest
# directory); a `..` that climbs out of the crate is a finding, one that stays inside is
# resolved first. The check needs cargo and the workspace; a repository with no Cargo.toml
# (the guard's own scratch repositories) has no crates to list. It runs offline, from the
# lock file: run it after a build has fetched the dependencies.
#
# A path that climbs out of its crate is a finding; none is excused.

# normalise PATH: PATH with its . and .. components resolved, or nothing (status 1) when a
# .. climbs above the start.
normalise() {
  local out=() part IFS=/
  for part in $1; do
    case $part in
      '' | .) ;;
      ..) [ "${#out[@]}" -gt 0 ] || return 1; unset 'out[${#out[@]}-1]' ;;
      *) out+=("$part") ;;
    esac
  done
  printf '%s\n' "${out[*]-}"
}
if [ -f Cargo.toml ]; then
  root=$(pwd -P)
  names=()
  dirs=()
  for d in crates/*/; do
    d=${d%/}
    [ -f "$d/Cargo.toml" ] || continue
    name=$(LC_ALL=C sed -n 's/^name = "\(.*\)"$/\1/p' "$d/Cargo.toml" | head -n 1)
    if [ -z "$name" ]; then
      echo "crate-paths: no package name in $d/Cargo.toml" >&2
      exit 2
    fi
    names+=("$name")
    dirs+=("$d")
  done
  checked=0
  i=0
  while [ "$i" -lt "${#names[@]}" ]; do
    name=${names[$i]}
    d=${dirs[$i]}
    i=$((i + 1))
    # The workspace's other crates are not in the registry index yet: each is taken from its path.
    cfg=()
    for j in "${!names[@]}"; do
      [ "${names[$j]}" = "$name" ] || cfg+=(--config "patch.crates-io.${names[$j]}.path=\"$root/${dirs[$j]}\"")
    done
    if ! cargo package --list --allow-dirty --locked --offline ${cfg[@]+"${cfg[@]}"} -p "$name" > "$scratch/list" 2> "$scratch/list.err"; then
      echo "crate-paths: cargo package --list -p $name failed:" >&2
      cat "$scratch/list.err" >&2
      exit 2
    fi
    LC_ALL=C sort "$scratch/list" > "$scratch/shipped"
    while IFS= read -r f; do
      case $f in src/*.rs | tests/*.rs) ;; *) continue ;; esac
      [ -f "$d/$f" ] || continue
      # One line, comments (whole lines starting //) dropped: a path named in a comment is not read.
      flat=$(LC_ALL=C sed 's#^[[:space:]]*//.*$##' "$d/$f" | LC_ALL=C tr '\n' ' ')
      set +e
      rel=$(printf '%s' "$flat" | LC_ALL=C grep -aoE 'include(_bytes|_str)?!\([[:space:]]*"[^"]*"')
      rc1=$?
      man=$(printf '%s' "$flat" | LC_ALL=C grep -aoE 'env!\("CARGO_MANIFEST_DIR"\),[[:space:]]*"[^"]*"')
      rc2=$?
      set -e
      if [ "$rc1" -ge 2 ] || [ "$rc2" -ge 2 ]; then
        echo "crate-paths: literal grep failed ($rc1, $rc2) in $d/$f" >&2
        exit 2
      fi
      for kind in rel man; do
        if [ "$kind" = rel ]; then lits=$rel; else lits=$man; fi
        [ -n "$lits" ] || continue
        while IFS= read -r lit; do
          lit=${lit%\"}
          lit=${lit##*\"}
          checked=$((checked + 1))
          if [ "$kind" = rel ]; then start=$(dirname "$f")/$lit; else start=$lit; fi
          if ! want=$(normalise "$start"); then
            echo "crate-paths: $d/$f reads \"$lit\", which climbs out of $name" >&2
            fail=1
            continue
          fi
          if ! LC_ALL=C grep -qxF -- "$want" "$scratch/shipped" \
            && ! LC_ALL=C awk -v p="$want/" 'index($0, p) == 1 { f = 1 } END { exit !f }' "$scratch/shipped"; then
            echo "crate-paths: $d/$f reads \"$lit\" ($want), which $name does not ship" >&2
            fail=1
          fi
        done <<< "$lits"
      done
    done < "$scratch/shipped"
  done
  if [ "$checked" -eq 0 ]; then
    echo "crate-paths: no include or manifest-relative path found in any crate: the check cannot cover what it names" >&2
    exit 2
  fi
  if [ "$fail" -eq 0 ]; then echo "crate-paths: ok ($checked paths checked)"; fi
fi

if [ "$fail" -ne 0 ]; then
  exit 1
fi
echo "crate-paths: clean"
