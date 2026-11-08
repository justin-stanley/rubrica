#!/usr/bin/env bash
# Check for scripts/verify-release-tag.sh: it must pass an annotated tag with an SSH signature
# from a listed key, and refuse every other shape with its exit code and a named reason. Every
# repository, key and tag lives in a scratch directory that is removed on exit; the keys are
# throwaway ed25519 keys made here. Git runs with the caller's configuration, home and ssh
# agent out of reach.
set -euo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.."
verify=$PWD/scripts/verify-release-tag.sh
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0

bad() { echo "test-verify-release-tag: FAIL: $*" >&2; fail=1; }

owner=owner@example.invalid
repo=$tmp/repo

# g ARGS...: git in the scratch repository, isolated from the caller.
g() {
  env -i PATH="$PATH" HOME="$tmp" SSH_AUTH_SOCK= GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
    git -C "$repo" -c user.name=owner -c user.email="$owner" "$@"
}
# sign KEY: the signing options of a tagger who holds KEY (the private key file, no agent).
sign() { printf '%s\n' -c gpg.format=ssh -c "user.signingkey=$1"; }

keygen() {
  env -i PATH="$PATH" HOME="$tmp" SSH_AUTH_SOCK= ssh-keygen -q -t ed25519 -N '' -C "$1" -f "$tmp/$1" >/dev/null
}
keygen keyA
keygen keyB
# keyline PRINCIPAL KEYFILE: one allowed_signers line (principal, type, base64; no comment).
keyline() { printf '%s %s\n' "$1" "$(cut -d' ' -f1,2 "$2.pub")"; }

mkdir -p "$repo"
g init -q -b main
echo one > "$repo/f"
g add f
g commit -q -m one

good=$tmp/signers-good
keyline "$owner" "$tmp/keyA" > "$good"

# case NAME WANT_RC REASON SIGNERS -- TAG...: runs the script in the scratch repository with
# RELEASE_ALLOWED_SIGNERS=SIGNERS (or unset when SIGNERS is "-") and the given arguments; its
# status must be WANT_RC and its output must hold REASON.
case_() {
  local name=$1 want=$2 reason=$3 signers=$4 out rc=0
  shift 5
  if [ "$signers" = - ]; then
    out=$(cd "$repo" && env -i PATH="$PATH" HOME="$tmp" SSH_AUTH_SOCK= GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null "$verify" "$@" 2>&1) || rc=$?
  else
    out=$(cd "$repo" && env -i PATH="$PATH" HOME="$tmp" SSH_AUTH_SOCK= GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
      RELEASE_ALLOWED_SIGNERS="$signers" "$verify" "$@" 2>&1) || rc=$?
  fi
  if [ "$rc" != "$want" ]; then bad "$name: exit $rc, want $want: $out"; return; fi
  case $out in *"$reason"*) echo "test-verify-release-tag: $name ok" ;; *) bad "$name: output lacks '$reason': $out" ;; esac
}

# Tags: signed with A, signed with B, unsigned annotated, lightweight.
# shellcheck disable=SC2046
g $(sign "$tmp/keyA") tag -s -m 'release 1.0.0' v1.0.0
# shellcheck disable=SC2046
g $(sign "$tmp/keyB") tag -s -m 'release 1.0.1' v1.0.1
g tag -a -m 'release 1.0.2' v1.0.2
g tag v1.0.3
# A's signed tag object under another tag name (a replay).
g update-ref refs/tags/v9.9.9 "$(g rev-parse refs/tags/v1.0.0)"

case_ 'a tag signed with a listed key' 0 'verify-release-tag: v1.0.0 ok' "$good" -- v1.0.0
case_ 'a lightweight tag' 1 'not an annotated tag' "$good" -- v1.0.3
case_ 'an unsigned annotated tag' 1 'has no valid signature' "$good" -- v1.0.2
case_ 'a tag signed with an unlisted key' 1 'has no valid signature' "$good" -- v1.0.1
case_ 'a tag that does not exist' 1 'no tag v7.7.7' "$good" -- v7.7.7
case_ 'a signed tag object under another name' 1 'names the tag v1.0.0, not v9.9.9' "$good" -- v9.9.9

# The right key under the wrong principal: the tagger is not who the file says holds the key.
keyline someone-else@example.invalid "$tmp/keyA" > "$tmp/signers-wrong-principal"
case_ 'a listed key under another principal' 1 'principal someone-else@example.invalid is not the tagger' \
  "$tmp/signers-wrong-principal" -- v1.0.0

case_ 'a missing signers file' 1 'allowed_signers is missing' "$tmp/none" -- v1.0.0
: > "$tmp/signers-empty"
case_ 'an empty signers file' 1 'allowed_signers is empty' "$tmp/signers-empty" -- v1.0.0
echo OWNER-SIGNING-KEY-PENDING > "$tmp/signers-placeholder"
case_ 'the placeholder signers file' 1 'OWNER-SIGNING-KEY-PENDING' "$tmp/signers-placeholder" -- v1.0.0
{ cat "$good"; echo OWNER-SIGNING-KEY-PENDING; } > "$tmp/signers-mixed"
case_ 'a signers file that still holds the placeholder' 1 'OWNER-SIGNING-KEY-PENDING' "$tmp/signers-mixed" -- v1.0.0
{ cat "$good"; echo 'not a key line'; } > "$tmp/signers-malformed"
case_ 'a signers file with a malformed line' 1 'line 2 is not' "$tmp/signers-malformed" -- v1.0.0
printf '# the owner\n\n%s\n' "$(cat "$good")" > "$tmp/signers-comments"
case_ 'comments and blank lines are allowed' 0 'verify-release-tag: v1.0.0 ok' "$tmp/signers-comments" -- v1.0.0

# The default signers file is .github/allowed_signers of the repository.
mkdir -p "$repo/.github"
cp "$good" "$repo/.github/allowed_signers"
case_ 'the default signers file' 0 'verify-release-tag: v1.0.0 ok' - -- v1.0.0

# Bad arguments are a usage error.
case_ 'no argument' 2 'usage:' "$good" --
case_ 'two arguments' 2 'usage:' "$good" -- v1.0.0 v1.0.1
case_ 'an empty argument' 2 'usage:' "$good" -- ''
case_ 'an option as the tag' 2 'usage:' "$good" -- -v
case_ 'a tag with a slash' 2 'usage:' "$good" -- refs/tags/v1.0.0
case_ 'a tag with a space' 2 'usage:' "$good" -- 'v1 0'

[ "$fail" = 0 ] || exit 1
echo "test-verify-release-tag: all cases ok"
