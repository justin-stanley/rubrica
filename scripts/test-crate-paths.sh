#!/usr/bin/env bash
# Tests scripts/crate-paths.sh: it must find every state it guards against, each with its
# exact exit code and message, and stay quiet on a clean tree. It runs the guard in scratch
# repositories, so the real tree never matters.
set -euo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.."
guard=$PWD/scripts/crate-paths.sh
tmp=$(mktemp -d)
trap 'chmod -R u+rwx "$tmp" 2>/dev/null || true; rm -rf "$tmp"' EXIT
fail=0

say() { echo "test-crate-paths: $*"; }
bad() { echo "test-crate-paths: $*" >&2; fail=1; }

# A scratch repository with a crate under crates/, clean.
mk_scratch() {
  local r=$1
  mkdir -p "$r/scripts" "$r/crates/x/src"
  cp "$guard" "$r/scripts/crate-paths.sh"
  echo 'fn a() {}' > "$r/crates/x/src/a.rs"
  git -C "$r" init -q
  git -C "$r" add -A
}
fresh() { local r; r=$(mktemp -d "$tmp/r.XXXXXX"); mk_scratch "$r"; echo "$r"; }

# run REPO: the repository's guard; rc in $rc, output in $tmp/out.
run() {
  set +e
  "$1/scripts/crate-paths.sh" >"$tmp/out" 2>&1
  rc=$?
  set -e
}

# check NAME WANT_RC MESSAGE: the last run exited WANT_RC and printed MESSAGE (a fixed string).
check() {
  local name=$1 want=$2 msg=$3
  if [ "$rc" -ne "$want" ] || ! grep -qF -- "$msg" "$tmp/out"; then
    bad "$name: want rc=$want and '$msg', got rc=$rc ($(head -n 1 "$tmp/out"))"
  else
    say "$name: rc=$rc"
  fi
}

# Control: the scratch repository is clean.
s=$(fresh); run "$s"; check 'clean control' 0 'crate-paths: clean'

# A tracked symlink under crates/ is read as link text, not as its target: refuse.
s=$(fresh); echo 'outside' > "$s/outside.txt"
ln -s ../../../outside.txt "$s/crates/x/src/link.rs"; git -C "$s" add -A
run "$s"; check 'tracked symlink' 2 'crate-paths: a guarded path holds a symlink or a submodule'
# ... and so is a dangling one.
s=$(fresh); ln -s nowhere "$s/crates/x/src/dangling.rs"; git -C "$s" add -A
run "$s"; check 'tracked dangling symlink' 2 'crate-paths: a guarded path holds a symlink or a submodule'

# A tracked submodule under crates/ is a gitlink: git grep does not descend into it. Refuse.
sub=$tmp/subrepo; mkdir "$sub"; git -C "$sub" init -q
echo 'fn hidden() {}' > "$sub/hidden.rs"; git -C "$sub" add -A
git -C "$sub" -c user.name=t -c user.email=t@example.invalid commit -q -m sub
s=$(fresh)
git -C "$s" -c protocol.file.allow=always submodule add -q "$sub" crates/x/src/sub 2>/dev/null
run "$s"; check 'tracked submodule' 2 'crate-paths: a guarded path holds a symlink or a submodule'

# A guarded glob that matches no tracked file narrows coverage silently: refuse.
s=$(fresh); git -C "$s" rm -q -r -f crates
run "$s"; check 'no crates' 2 'crate-paths: a guarded path matches no file: crates/**'

# node: forbidden under crates/, with no exception.
s=$(fresh); printf 'let o = Command::new("node");\n' > "$s/crates/x/src/a.rs"; git -C "$s" add -A
run "$s"
check 'node in a source file' 1 'crate-paths: node runs under crates/:'
check 'node, named' 1 'crates/x/src/a.rs:1'

# The reads-within-crate stage: a literal that climbs out of its crate is a finding,
# reported by file and line only.
reads='crate-paths: every read must stay within its crate:'
s=$(fresh); printf 'const A: &str = include_str!("../../fixtures/x");\n' > "$s/crates/x/src/a.rs"; git -C "$s" add -A
run "$s"; check 'read climbing out' 1 "$reads"
check 'read climbing out, named' 1 'crates/x/src/a.rs:1'
if grep -q 'include_str' "$tmp/out"; then bad 'the reads report echoes the matched line'; fi
s=$(fresh); printf 'const A: &str = concat!(\n    env!("CARGO_MANIFEST_DIR"),\n    "/../../fixtures/x"\n);\n' > "$s/crates/x/src/a.rs"; git -C "$s" add -A
run "$s"; check 'multi-line concat! climbing out' 1 "$reads"
check 'multi-line concat!, named' 1 'crates/x/src/a.rs:3'
s=$(fresh); printf 'const A: &str = include_str!("../../vendor/x");\n' > "$s/crates/x/src/a.rs"; git -C "$s" add -A
run "$s"; check 'read into vendor' 1 "$reads"
s=$(fresh); printf 'const A: &str = include_str!("../../fixtures/x");\n' > "$s/crates/x/src/vendored.rs"; git -C "$s" add -A
run "$s"; check 'read climbing out of a file named vendored' 1 "$reads"
check 'read in vendored.rs, named' 1 'crates/x/src/vendored.rs:1'

# --- the packaged file list ---------------------------------------------------
# With a Cargo.toml the guard also checks every crate's reads against `cargo package
# --list`: a mini workspace with one crate, offline.
mk_ws() { # REPO INCLUDE_LINE LIB_RS
  local r=$1
  mkdir -p "$r/crates/x/src"
  printf '[workspace]\nresolver = "2"\nmembers = ["crates/x"]\n' > "$r/Cargo.toml"
  printf '[package]\nname = "x"\nversion = "0.1.0"\nedition = "2021"\n%s\n' "$2" > "$r/crates/x/Cargo.toml"
  printf '%s\n' "$3" > "$r/crates/x/src/lib.rs"
  echo data > "$r/crates/x/src/data.txt"
  echo data > "$r/crates/x/notes.txt"
  (cd "$r" && cargo generate-lockfile --offline -q) || bad 'cannot make the mini workspace lock file'
  git -C "$r" add -A
}
ws() { local r; r=$(fresh); mk_ws "$r" "$1" "$2"; echo "$r"; }
s=$(ws '' 'pub const A: &str = include_str!("data.txt");')
run "$s"; check 'a shipped include_str!' 0 'crate-paths: ok (1 paths checked)'
s=$(ws '' 'pub const A: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/notes.txt"
));')
run "$s"; check 'a shipped manifest-relative path, over several lines' 0 'crate-paths: ok (1 paths checked)'
s=$(ws 'include = ["src/lib.rs", "Cargo.toml"]' 'pub const A: &str = include_str!("data.txt");')
run "$s"; check 'an include_str! of a file the crate does not ship' 1 'crates/x/src/lib.rs reads "data.txt" (src/data.txt), which x does not ship'
s=$(ws 'include = ["src/lib.rs", "Cargo.toml"]' 'pub const A: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/notes.txt"));')
run "$s"; check 'a manifest-relative path of a file the crate does not ship' 1 'which x does not ship'
s=$(ws '' 'pub const A: &str = include_str!("../../../outside.txt");')
run "$s"; check 'a path that climbs out of the crate' 1 'crates/x/src/lib.rs reads "../../../outside.txt", which climbs out of x'
s=$(ws '' 'pub const A: &str = include_str!("../notes.txt");')
run "$s"; check 'a .. that stays inside the crate is resolved' 0 'crate-paths: ok (1 paths checked)'
s=$(ws '' '// include_str!("../../../outside.txt") is a comment
pub const A: &str = include_str!("data.txt");')
run "$s"; check 'a path in a comment is not read' 0 'crate-paths: ok (1 paths checked)'
s=$(ws '' 'pub fn f() {}')
run "$s"; check 'no path to check at all' 2 'the check cannot cover what it names'

# A test of one crate reading a sibling crate's directory is a finding like any other.
s=$(fresh)
mkdir -p "$s/crates/rubrica/src" "$s/crates/rubrica/tests"
printf '[workspace]\nresolver = "2"\nmembers = ["crates/rubrica"]\n' > "$s/Cargo.toml"
printf '[package]\nname = "rubrica"\nversion = "0.1.0"\nedition = "2021"\n' > "$s/crates/rubrica/Cargo.toml"
echo 'pub fn f() {}' > "$s/crates/rubrica/src/lib.rs"
printf 'const T: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../rubrica-site/theme");\n' > "$s/crates/rubrica/tests/build.rs"
(cd "$s" && cargo generate-lockfile --offline -q) || bad 'cannot make the escape workspace lock file'
git -C "$s" add -A
run "$s"; check 'a read of a sibling crate is a finding' 1 'crates/rubrica/tests/build.rs reads "/../rubrica-site/theme", which climbs out of rubrica'

# A grep that errors must not read as clean (an unreadable file under crates/).
if [ "$(id -u)" -ne 0 ]; then
  s=$(fresh); echo 'fn b() {}' > "$s/crates/x/src/b.rs"; chmod 000 "$s/crates/x/src/b.rs"
  run "$s"; check 'grep error' 2 'crate-paths: grep failed (2)'
  chmod 644 "$s/crates/x/src/b.rs"
fi

[ "$fail" -eq 0 ] || exit 1
echo "test-crate-paths: ok"
