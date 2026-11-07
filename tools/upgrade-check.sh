#!/usr/bin/env bash
# The upgrade test: the candidate binary renders the same sites as the previous release's.
#
#   tools/upgrade-check.sh <candidate-bin> <version> [<previous-tag>]
#
# Run it from the repository root. The previous tag defaults to the newest final release tag
# (`v[0-9]*` without a `-`) other than the candidate's; with none, it prints
# `upgrade-check: no previous release` and passes. Otherwise it downloads that release's
# x86_64-linux-musl tarball and its .sha256, checks the sha256, verifies the build
# attestation (`gh attestation verify`), unpacks the binary, and runs tools/render-diff.sh
# with the previous binary and the candidate on the specification repository
# (tools/render-diff-spec-repo.sh) and on `.` with `--subtree docs`.
#
# A difference fails the check unless the CHANGELOG notes for the version
# (scripts/release-notes.sh) hold a line starting `- Render change:`: the deliberate-change
# escape hatch, which the maintainer sees at review.
#
# Test mode (the tests only; a workflow never sets it): GH_NO_NETWORK=1 skips the download,
# the sha256 and the attestation, prints that it did, and takes the previous binary from
# UPGRADE_PREVIOUS_BIN. Either variable without the other is refused.
#
# Exit codes: 0 the renders are identical, or the difference is declared, or there is no
# previous release; 1 a difference that is not declared; 2 a usage error or a step that did
# not run (a download, a checksum, an attestation, a build). Fails closed: every status is read.
set -euo pipefail
export LC_ALL=C

repo_url=https://github.com/justin-stanley/rubrica
repo_slug=justin-stanley/rubrica
signer=justin-stanley/rubrica/.github/workflows/release.yml

say() { echo "upgrade-check: $*"; }
fail() {
  echo "upgrade-check: $1" >&2
  exit 2
}

[ "$#" -ge 2 ] && [ "$#" -le 3 ] || fail "usage: upgrade-check.sh <candidate-bin> <version> [<previous-tag>]"
candidate=$1
version=$2
tag=${3:-}
[ -f "$candidate" ] && [ -x "$candidate" ] || fail "not an executable file: $candidate"
printf '%s\n' "$version" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?$' \
  || fail "'$version' is not X.Y.Z or X.Y.Z-rc.N (no leading v)"
case $candidate in /*) ;; *) candidate=$PWD/$candidate ;; esac

# The test mode comes as a pair; a CI run sets neither.
no_network=${GH_NO_NETWORK:-}
override=${UPGRADE_PREVIOUS_BIN:-}
if [ -n "$override" ] && [ "$no_network" != 1 ]; then
  fail "UPGRADE_PREVIOUS_BIN is the test mode and needs GH_NO_NETWORK=1"
fi

here=$(cd "$(dirname "$0")" && pwd -P) || fail "cannot find the script directory"

# The previous release: the tag given, else the newest final tag that is not the candidate's.
if [ -z "$tag" ]; then
  rc=0
  tags=$(git tag -l 'v[0-9]*') || rc=$?
  [ "$rc" -eq 0 ] || fail "git tag failed ($rc)"
  tag=$(printf '%s\n' "$tags" | { grep -v -- '-' || [ $? -eq 1 ]; } | { grep -vxF "v$version" || [ $? -eq 1 ]; } | sort -V | tail -n 1)
fi
if [ -z "$tag" ] && [ -z "$override" ]; then
  say "no previous release"
  exit 0
fi
case $tag in
  v[0-9]*.[0-9]*.[0-9]* | '') ;;
  *) fail "'$tag' is not a release tag (vX.Y.Z)" ;;
esac

tmp=$(mktemp -d "${TMPDIR:-/tmp}/upgrade-check.XXXXXX") || fail "cannot make a scratch directory"
cleanup() { rm -rf "$tmp"; }
trap cleanup EXIT

if [ "$no_network" = 1 ]; then
  [ -n "$override" ] || fail "GH_NO_NETWORK=1 needs UPGRADE_PREVIOUS_BIN, the previous binary"
  [ -f "$override" ] && [ -x "$override" ] || fail "not an executable file: $override"
  case $override in /*) ;; *) override=$PWD/$override ;; esac
  previous=$override
  say "skipping the download, the sha256 and the attestation (GH_NO_NETWORK=1): the previous binary is $previous"
else
  prev_version=${tag#v}
  name=rubrica-$prev_version-x86_64-linux-musl
  base=$repo_url/releases/download/$tag
  for f in "$name.tar.gz" "$name.tar.gz.sha256"; do
    curl --proto '=https' --tlsv1.2 -sSfL -o "$tmp/$f" "$base/$f" || fail "cannot download $base/$f"
  done
  # The .sha256 file is one line, `<sha256>  <tarball name>`. It is read and compared here
  # rather than handed to `sha256sum -c`, which some versions let pass on a malformed line.
  sums=$(cat "$tmp/$name.tar.gz.sha256") || fail "cannot read $name.tar.gz.sha256"
  rc=0
  printf '%s\n' "$sums" | grep -qE "^[0-9a-f]{64} [ *]$name\.tar\.gz\$" || rc=$?
  [ "$rc" -eq 0 ] || fail "$name.tar.gz.sha256 is not '<sha256>  $name.tar.gz'"
  [ "$(printf '%s\n' "$sums" | wc -l | tr -d ' ')" -eq 1 ] || fail "$name.tar.gz.sha256 has more than one line"
  want=${sums%% *}
  if command -v sha256sum > /dev/null; then
    got=$(sha256sum "$tmp/$name.tar.gz") || fail "sha256sum failed"
  elif command -v shasum > /dev/null; then
    got=$(shasum -a 256 "$tmp/$name.tar.gz") || fail "shasum failed"
  else
    fail "neither sha256sum nor shasum is installed"
  fi
  got=${got%% *}
  [ "$got" = "$want" ] || fail "the sha256 of $name.tar.gz is $got, the release lists $want"
  command -v gh > /dev/null || fail "gh is not installed (gh attestation verify needs 2.49 or later)"
  gh attestation verify "$tmp/$name.tar.gz" --repo "$repo_slug" --signer-workflow "$signer" \
    || fail "the attestation of $name.tar.gz does not verify"
  tar -xzf "$tmp/$name.tar.gz" -C "$tmp" --strip-components=1 "$name/rubrica" \
    || fail "cannot unpack $name/rubrica"
  previous=$tmp/rubrica
  [ -f "$previous" ] && [ -x "$previous" ] || fail "the tarball holds no executable rubrica"
fi

# Render the specification repository and the documentation with both binaries.
differs=0
run_diff() { # what, args...
  local what=$1 rc=0
  shift
  say "$what"
  "$here/render-diff.sh" "$previous" "$candidate" "$@" || rc=$?
  case $rc in
    0) ;;
    1) differs=1 ;;
    *) fail "render-diff failed on $what ($rc)" ;;
  esac
}
"$here/render-diff-spec-repo.sh" "$tmp/spec" || fail "cannot write the specification repository"
run_diff "the specification repository, $tag against $version" "$tmp/spec"
run_diff "docs/, $tag against $version" . --subtree docs

if [ "$differs" -eq 0 ]; then
  say "the renders are identical ($tag against $version)"
  exit 0
fi

# A difference: allowed only when the version's changelog notes declare it.
rc=0
notes=$(scripts/release-notes.sh "$version") || rc=$?
if [ "$rc" -ne 0 ]; then
  echo "upgrade-check: the renders differ and the changelog notes for $version cannot be read (release-notes.sh exited $rc)" >&2
  exit 1
fi
rc=0
declared=$(printf '%s\n' "$notes" | grep -E '^- Render change:') || rc=$?
if [ "$rc" -ge 2 ]; then fail "grep failed ($rc)"; fi
if [ "$rc" -eq 0 ]; then
  say "render change declared in the changelog for $version:"
  printf '%s\n' "$declared"
  exit 0
fi
echo "upgrade-check: the renders differ from $tag, and the changelog notes for $version hold no line starting '- Render change:'. Name the change there, or fix it." >&2
exit 1
