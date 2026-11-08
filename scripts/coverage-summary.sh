#!/usr/bin/env bash
# The coverage summary and the floor check. It reads the JSON that
# `cargo llvm-cov report --json --summary-only` writes (cargo-llvm-cov 0.9.1: the llvm-cov
# export format, version 2.0.1: .data[0].files[].summary.{lines,functions,regions} with
# count and covered), adds the files up per crate, and prints Markdown on stdout: a table per
# crate (lowest lines coverage first, workspace row last) and a collapsible per-file table
# for each crate. Paths are relative to the repository.
#
#   scripts/coverage-summary.sh REPORT.json coverage-floor.toml [--root DIR] [--badge FILE]
#
# Every crate must have a floor in coverage-floor.toml, and every crate with a floor must be
# in the report. A crate fails when its lines coverage is more than 0.5 points below its
# floor. --badge writes the shields.io endpoint JSON for the workspace's lines coverage.
#
# Exit codes: 0 fine; 1 a crate is below its floor, or has none (the summary is still
# printed); 2 bad input or usage (nothing is printed on stdout). Fails closed: unreadable,
# empty or malformed JSON, an unexpected shape, a crate with no lines and a crate in the
# floor file but not in the report all exit 2 with a message.
set -euo pipefail
export LC_ALL=C

die() { echo "coverage-summary: $*" >&2; exit 2; }

report=${1:-}
floor_file=${2:-}
[ "$#" -ge 2 ] || die "usage: coverage-summary.sh REPORT.json coverage-floor.toml [--root DIR] [--badge FILE]"
shift 2
root=$(cd "$(dirname "$0")/.." && pwd -P)
badge=
while [ "$#" -gt 0 ]; do
  case $1 in
    --root) [ "$#" -ge 2 ] || die "usage: --root needs a directory"; root=$2; shift 2 ;;
    --badge) [ "$#" -ge 2 ] || die "usage: --badge needs a file"; badge=$2; shift 2 ;;
    *) die "usage: unknown argument '$1'" ;;
  esac
done
root=${root%/}

[ -r "$report" ] || die "cannot read the report '$report'"
[ -r "$floor_file" ] || die "cannot read the floor file '$floor_file'"
[ -s "$report" ] || die "the report '$report' is empty"
jq -e . "$report" >/dev/null 2>&1 || die "the report '$report' is not valid JSON"

# The floor file: a `[floor]` table of `crate = N.N` lines, comments with `#`. Each floor is
# kept in tenths of a point so the comparison below is exact.
re_blank='^[[:space:]]*(#.*)?$'
re_table='^\[floor\][[:space:]]*(#.*)?$'
re_entry='^([a-z0-9][a-z0-9_-]*)[[:space:]]*=[[:space:]]*([0-9]{1,3})\.([0-9])[[:space:]]*(#.*)?$'
in_floor=0 n=0 pairs=
while IFS= read -r line || [ -n "$line" ]; do
  n=$((n + 1))
  if [[ $line =~ $re_blank ]]; then continue
  elif [[ $line =~ $re_table ]]; then in_floor=1
  elif [ "$in_floor" = 1 ] && [[ $line =~ $re_entry ]]; then
    tenths=$((10#${BASH_REMATCH[2]} * 10 + BASH_REMATCH[3]))
    [ "$tenths" -le 1000 ] || die "$floor_file line $n: a floor above 100.0"
    case " $pairs" in *" ${BASH_REMATCH[1]}="*) die "$floor_file line $n: ${BASH_REMATCH[1]} is given twice" ;; esac
    pairs="$pairs ${BASH_REMATCH[1]}=$tenths"
  else
    die "$floor_file line $n: not understood (want '[floor]' then 'crate = N.N'): $line"
  fi
done <"$floor_file"
[ -n "$pairs" ] || die "$floor_file has no floors"
# shellcheck disable=SC2086 # the pairs are words
floors_json=$(printf '%s\n' $pairs | jq -Rn '[inputs | split("=") | {key: .[0], value: (.[1] | tonumber)}] | from_entries')

# The model: per-crate and per-file numbers, the Markdown, the failures, the badge.
# shellcheck disable=SC2016
model=$(jq --arg root "$root" --argjson floors "$floors_json" '
  def shape: error("unexpected shape: " + .);
  def tenths: (. * 10 | round);
  def fmt1: tenths as $t | (if $t < 0 then "-" else "" end) + (($t | fabs) as $a | "\($a / 10 | floor).\($a % 10)");
  def delta: if . > 0 then "+" + fmt1 else fmt1 end;
  def pct(c; n): if n == 0 then null else c * 100 / n end;
  def show($p): if $p == null then "n/a" else ($p | fmt1) + "%" end;
  def cell($k): (.summary[$k]) as $s
    | if ($s | type) == "object" and ($s.count | type) == "number" and ($s.covered | type) == "number"
         and $s.count >= 0 and $s.covered >= 0 and $s.covered <= $s.count
      then {count: $s.count, covered: $s.covered} else ("file \(.filename): \($k)" | shape) end;

  (if (.data | type) == "array" and (.data | length) == 1 and (.data[0].files | type) == "array" and (.data[0].files | length) > 0
   then .data[0].files else ("need .data[0].files, a non-empty array" | shape) end)
  | map(
      if (.filename | type) != "string" then ("a file with no name" | shape) else . end
      | if (.filename | test("[|`\n]")) then error("a file name with a table character: \(.filename)") else . end
      | if (.filename | startswith($root + "/")) then . else error("\(.filename) is not under \($root)/") end
      | .rel = .filename[($root | length) + 1:]
      | if (.rel | test("^crates/[^/]+/")) then . else error("\(.rel) is not under crates/<name>/") end
      | .crate = (.rel | capture("^crates/(?<c>[^/]+)/").c)
      | {rel, crate, l: cell("lines"), f: cell("functions"), r: cell("regions")})
  | . as $files
  | def agg($fs): {
      lc: ($fs | map(.l.count) | add), lv: ($fs | map(.l.covered) | add),
      fc: ($fs | map(.f.count) | add), fv: ($fs | map(.f.covered) | add),
      rc: ($fs | map(.r.count) | add), rv: ($fs | map(.r.covered) | add)};
    def row($name; $a): $a + {name: $name, lp: pct($a.lv; $a.lc), fp: pct($a.fv; $a.fc), rp: pct($a.rv; $a.rc)};
    ($files | group_by(.crate)
      | map(. as $g | row($g[0].crate; agg($g)) + {files: ($g | map(row(.rel; agg([.])) ) | sort_by(.name))})) as $crates
  | ($crates | map(select(.lc == 0) | .name)) as $empty
  | if ($empty | length) > 0 then error("\($empty[0]) has no lines to cover") else . end
  | ([$floors | keys[] | select(. as $k | $crates | map(.name) | index($k) | not)]) as $absent
  | if ($absent | length) > 0 then error("\($absent[0]) is in coverage-floor but absent from the report") else . end
  | row("workspace"; agg($files)) as $total
  | if $total.lc == 0 then error("the report has no lines") else . end
  | ($crates | map(. + (
      if $floors[.name] == null then {ft: null, status: "NO FLOOR"}
      else {ft: $floors[.name], status:
        (if .lv * 1000 >= .lc * $floors[.name] then "ok"
         elif .lv * 1000 >= .lc * ($floors[.name] - 5) then "below floor, within 0.5"
         else "FAIL" end)} end))
    | sort_by([.lp, .name])) as $rows
  | ($rows | map(select(.status == "FAIL" or .status == "NO FLOOR")
      | if .status == "NO FLOOR" then "\(.name) has no floor in coverage-floor.toml"
        else "\(.name) is \(((.ft / 10) - .lp) | . * 10 | round / 10 | fmt1) points below its floor (\(.lp | fmt1)% against \(.ft / 10 | fmt1)%)" end)) as $failures
  | def sline($x): "| \($x.lv) / \($x.lc) | \(show($x.fp)) | \(show($x.rp)) |";
    ([
      "### Coverage", "",
      "| Crate | Lines | Floor | Δ | Functions | Regions | Lines covered / total | Status |",
      "| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |",
      ($rows[] | "| `\(.name)` | \(show(.lp)) | \(if .ft == null then "none" else show(.ft / 10) end) | \(if .ft == null then "-" else ((.lp | tenths) - .ft | . / 10 | delta) end) | \(show(.fp)) | \(show(.rp)) | \(.lv) / \(.lc) | \(.status) |"),
      "| **workspace** | **\(show($total.lp))** | | | \(show($total.fp)) | \(show($total.rp)) | \($total.lv) / \($total.lc) | |",
      "",
      "A crate fails when its lines coverage is more than 0.5 points below its floor in `coverage-floor.toml`, or when it has none.",
      (if ($failures | length) > 0 then "", "**Coverage floor check failed:**", "", ($failures[] | "- " + .) else empty end),
      "",
      ($rows[] | . as $c |
        "<details>", "<summary><code>\($c.name)</code>: \(show($c.lp)) of lines (\($c.lv) / \($c.lc))</summary>", "",
        "| File | Lines | Functions | Regions | Lines covered / total |",
        "| --- | ---: | ---: | ---: | ---: |",
        ($c.files[] | "| `\(.name)` | \(show(.lp)) | \(show(.fp)) | \(show(.rp)) | \(.lv) / \(.lc) |"),
        "", "</details>", "")
    ] | join("\n")) as $md
  | {md: $md, failures: $failures, message: (show($total.lp)),
     color: (($total.lp | tenths / 10) as $p
       | if $p >= 90 then "brightgreen" elif $p >= 80 then "green" elif $p >= 70 then "yellowgreen"
         elif $p >= 60 then "yellow" elif $p >= 50 then "orange" else "red" end)}
' "$report" 2>&1) || die "$(printf '%s' "$model" | sed -e 's/^jq: error[^:]*: //' -e 's/ (while parsing.*//' | head -3)"

printf '%s\n' "$model" | jq -r .md
if [ -n "$badge" ]; then
  printf '%s\n' "$model" | jq -c '{schemaVersion: 1, label: "coverage", message: .message, color: .color}' >"$badge"
fi
nfail=$(printf '%s\n' "$model" | jq '.failures | length')
if [ "$nfail" -gt 0 ]; then
  printf '%s\n' "$model" | jq -r '.failures[] | "coverage-summary: " + .' >&2
  exit 1
fi
