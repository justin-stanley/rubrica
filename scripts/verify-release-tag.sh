#!/usr/bin/env bash
# scripts/verify-release-tag.sh <tag>
#
# Passes only for an annotated tag carrying an SSH signature from a key listed in
# .github/allowed_signers, made by the tagger the list names for that key. The release
# workflow runs it before any build job, so a release is built only from a tag the owner signed.
#
# Refused (exit 1, the reason on stderr): a tag that does not exist; a lightweight tag; an
# annotated tag with no valid SSH signature, or signed with a key that is not listed; a listed
# key whose principal is not the tagger's e-mail; a signed tag object that names another tag
# (a replay under a new name); a signers file that is missing, empty, holds no key line, holds
# a malformed line, or still holds the OWNER-SIGNING-KEY-PENDING placeholder.
# Exit 2: bad arguments, or a setup the script cannot work in (no repository, no ssh-keygen,
# git older than 2.34, which cannot verify SSH signatures).
#
# Environment: RELEASE_ALLOWED_SIGNERS names another signers file (the tests do); the default
# is .github/allowed_signers at the top of the repository.
set -euo pipefail
export LC_ALL=C

refuse() { echo "verify-release-tag: refused: $*" >&2; exit 1; }
setup() { echo "verify-release-tag: $*" >&2; exit 2; }
usage() { echo "usage: scripts/verify-release-tag.sh <tag>   (the tag name, e.g. v1.2.3)" >&2; exit 2; }

[ "$#" -eq 1 ] || usage
tag=$1
tag_re='^[A-Za-z0-9][A-Za-z0-9._-]*$'
[[ $tag =~ $tag_re ]] || usage

gv=$(git --version) || setup "git is not available"
gv_re='^git version ([0-9]+)\.([0-9]+)'
[[ $gv =~ $gv_re ]] || setup "cannot read the git version from '$gv'"
if [ "${BASH_REMATCH[1]}" -lt 2 ] || { [ "${BASH_REMATCH[1]}" -eq 2 ] && [ "${BASH_REMATCH[2]}" -lt 34 ]; }; then
  setup "git ${BASH_REMATCH[1]}.${BASH_REMATCH[2]} cannot verify SSH signatures; 2.34 or newer is needed"
fi
command -v ssh-keygen >/dev/null 2>&1 || setup "ssh-keygen is not available"
top=$(git rev-parse --show-toplevel 2>/dev/null) || setup "not inside a git repository"

# 1. The signers file: present, not empty, no placeholder, only key lines.
signers=${RELEASE_ALLOWED_SIGNERS:-$top/.github/allowed_signers}
[ -f "$signers" ] || refuse "allowed_signers is missing ($signers)"
[ -s "$signers" ] || refuse "allowed_signers is empty ($signers)"
key_re='^[^ #]+ (ssh-ed25519|sk-ssh-ed25519@openssh\.com|ecdsa-sha2-nistp256|sk-ecdsa-sha2-nistp256@openssh\.com) [A-Za-z0-9+/]+=*( .*)?$'
blank_re='^[[:space:]]*$'
n=0
keys=0
while IFS= read -r line || [ -n "$line" ]; do
  n=$((n + 1))
  case $line in
    *OWNER-SIGNING-KEY-PENDING*) refuse "allowed_signers still holds the placeholder OWNER-SIGNING-KEY-PENDING (line $n): the owner's key is not in yet" ;;
    '#'*) continue ;;
  esac
  [[ $line =~ $blank_re ]] && continue
  [[ $line =~ $key_re ]] || refuse "allowed_signers line $n is not 'principal key-type base64'"
  keys=$((keys + 1))
done < "$signers"
[ "$keys" -ge 1 ] || refuse "allowed_signers has no key line"

# 2. The tag exists and is an annotated tag object.
ref=refs/tags/$tag
git rev-parse --verify --quiet "$ref" >/dev/null || refuse "no tag $tag"
kind=$(git cat-file -t "$ref") || setup "git cat-file failed on $ref"
[ "$kind" = tag ] || refuse "$tag is not an annotated tag (a $kind object: a lightweight tag proves nothing)"
obj=$(git cat-file tag "$ref") || setup "git cat-file failed on $ref"

# 3. The signed object names this tag (a signed tag pushed under another name is a replay), and
# says who tagged it. Only the header counts: the message below the blank line is free text.
header=$(printf '%s\n' "$obj" | awk '/^$/ { exit } { print }')
named=$(printf '%s\n' "$header" | sed -n 's/^tag //p')
[ "$named" = "$tag" ] || refuse "the signed object names the tag $named, not $tag"
tagger=$(printf '%s\n' "$header" | sed -n 's/^tagger .*<\(.*\)> [0-9][0-9]* [-+][0-9][0-9]*$/\1/p')
[ -n "$tagger" ] || refuse "$tag has no tagger e-mail"

# 4. The signature: SSH, from a listed key.
rc=0
verdict=$(git -c gpg.ssh.allowedSignersFile="$signers" verify-tag "$ref" 2>&1) || rc=$?
if [ "$rc" -ne 0 ] || ! printf '%s\n' "$obj" | grep -qxF -- '-----BEGIN SSH SIGNATURE-----'; then
  refuse "$tag has no valid signature from a listed key ($(printf '%s' "$verdict" | tr '\n' ' '))"
fi

# 5. The key's principal is the tagger. git checks the key against the file, not who tagged.
principal=$(printf '%s\n' "$verdict" | sed -n 's/^Good "git" signature for \(.*\) with [A-Za-z0-9-]* key SHA256:.*$/\1/p')
[ -n "$principal" ] || refuse "$tag: git did not report which principal signed it ($(printf '%s' "$verdict" | tr '\n' ' '))"
[ "$principal" = "$tagger" ] || refuse "$tag: principal $principal is not the tagger $tagger (the key is listed for another identity)"

echo "verify-release-tag: $tag ok (signed by $principal)"
