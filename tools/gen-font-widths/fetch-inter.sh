#!/usr/bin/env bash
# Downloads the Inter 4.1 release archive into DIR, checks its size and sha256, and extracts
# the three files the width generator and the theme use. Usage: fetch-inter.sh DIR
#
# GitHub publishes no digest for this asset, so the sha256 below was recorded from two
# independent downloads (trust on first use); a mismatch means the archive changed.
set -euo pipefail
export LC_ALL=C

url=https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip
zip_bytes=33707794
zip_sha256=9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e
ttf=extras/ttf/Inter-Regular.ttf
woff2=web/Inter-Regular.woff2
licence=LICENSE.txt

[ $# -eq 1 ] || { echo "usage: fetch-inter.sh DIR" >&2; exit 2; }
dir=$1
mkdir -p "$dir"
zip=$dir/Inter-4.1.zip

[ -f "$zip" ] || curl -fsSL -o "$zip" "$url"

bytes=$(wc -c < "$zip" | tr -d ' ')
if [ "$bytes" != "$zip_bytes" ]; then
  echo "fetch-inter: Inter-4.1.zip is $bytes bytes, want $zip_bytes"; exit 1
fi
if command -v shasum >/dev/null 2>&1; then
  sum=$(shasum -a 256 "$zip" | cut -d' ' -f1)
else
  sum=$(sha256sum "$zip" | cut -d' ' -f1)
fi
if [ "$sum" != "$zip_sha256" ]; then
  echo "fetch-inter: Inter-4.1.zip has sha256 $sum, want $zip_sha256"; exit 1
fi

# The listing goes into a variable: grep -q closing the pipe early would fail under pipefail.
members=$(unzip -Z1 "$zip")
for member in "$ttf" "$woff2" "$licence"; do
  printf '%s\n' "$members" | grep -qxF "$member" || { echo "fetch-inter: $member is not in Inter-4.1.zip"; exit 1; }
done
unzip -o -j -q "$zip" "$ttf" "$woff2" "$licence" -d "$dir"
echo "fetch-inter: Inter 4.1 in $dir: Inter-Regular.ttf Inter-Regular.woff2 LICENSE.txt"
