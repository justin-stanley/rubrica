#!/usr/bin/env bash
# What crates.io would receive for one crate, checked.
#
#   scripts/package-check.sh [--patch-siblings] CRATE
#
# 1. `cargo package --list` is exactly crates/CRATE/PACKAGE-FILES (the committed, reviewed
#    list: a change to what ships is a diff there);
# 2. the .crate (target/package/CRATE-VERSION.crate) is packed with the lock file, holds
#    exactly those files, and is under crates.io's 10 MB limit;
# 3. the unpacked crate builds on its own: nothing it needs is outside it.
#
# --patch-siblings: cargo packs a crate against the registry's copy of the workspace's other
# crates (the packed manifest names them by version), so each must be on crates.io at that
# version. On a branch they are not: before the first release none is, and after it the
# branch's version is ahead of the last release. CI's `package` job passes this and each
# sibling is taken from its path (the packed manifest is unchanged). The release jobs pass
# nothing: they run in dependency order, so the unpacked crate is built against the siblings
# the chain published just before it. The bundle crate's files must exist
# (scripts/build-wasm.sh --bundle, or CI's downloaded artifact).
set -euo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.."
root=$(pwd -P)

patch=0
if [ "${1:-}" = "--patch-siblings" ]; then patch=1; shift; fi
[ "$#" -eq 1 ] || { echo "usage: scripts/package-check.sh [--patch-siblings] CRATE" >&2; exit 2; }
crate=$1
[ -f "crates/$crate/PACKAGE-FILES" ] || { echo "package-check: crates/$crate/PACKAGE-FILES is missing" >&2; exit 1; }

cfg=()
if [ "$patch" -eq 1 ]; then
  for s in rubrica-markdown rubrica-render rubrica-wasm-bundle rubrica-site; do
    [ "$s" = "$crate" ] || cfg+=(--config "patch.crates-io.$s.path=\"$root/crates/$s\"")
  done
fi

version=$(cargo pkgid --locked -p "$crate" | sed 's/.*[#@]//')
want=$(sort "crates/$crate/PACKAGE-FILES")

# 1. The list.
listed=$(cargo package --list --allow-dirty --locked ${cfg[@]+"${cfg[@]}"} -p "$crate" | sort)
if [ "$listed" != "$want" ]; then
  echo "package-check: $crate: cargo package --list differs from crates/$crate/PACKAGE-FILES:" >&2
  diff <(printf '%s\n' "$want") <(printf '%s\n' "$listed") >&2 || true
  exit 1
fi

# 2. The .crate.
cargo package --allow-dirty --locked --no-verify ${cfg[@]+"${cfg[@]}"} -p "$crate"
file=target/package/$crate-$version.crate
[ -f "$file" ] || { echo "package-check: $file was not written" >&2; exit 1; }
size=$(wc -c < "$file" | tr -d ' ')
[ "$size" -lt 10000000 ] || { echo "package-check: $file is $size bytes, over crates.io's 10 MB limit" >&2; exit 1; }
packed=$(tar -tzf "$file" | sed "s#^$crate-$version/##" | { grep -v '/$' || [ $? -eq 1 ]; } | sort)
if [ "$packed" != "$want" ]; then
  echo "package-check: $crate: the .crate holds a different set of files than PACKAGE-FILES:" >&2
  diff <(printf '%s\n' "$want") <(printf '%s\n' "$packed") >&2 || true
  exit 1
fi

# 3. The unpacked crate builds alone. The workspace's lock file keeps the dependencies at
# the versions the repository tests; a crate that packs its own keeps that one.
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
tar -xzf "$file" -C "$work"
dir=$work/$crate-$version
[ -f "$dir/Cargo.lock" ] || cp Cargo.lock "$dir/Cargo.lock"
(cd "$dir" && cargo build --target-dir "$root/target/package-check" ${cfg[@]+"${cfg[@]}"})

echo "package-check: ok $crate $version ($size bytes, $(printf '%s\n' "$want" | wc -l | tr -d ' ') files)"
