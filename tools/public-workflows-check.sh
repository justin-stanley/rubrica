#!/usr/bin/env bash
# Check: the GitHub workflows and the files around them under .github/, the release
# script, the Dockerfile and the install docs. Fails closed: every grep's status is captured (0 hits, 1 none, 2+ error) and an error
# fails the check. Run from anywhere: it enters the repository root. Needs ruby and
# actionlint.
set -euo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.."

gh_dir=.github
wf=$gh_dir/workflows
fail=0
say() { echo "public-workflows-check: $*"; }
bad() { echo "public-workflows-check: FAIL: $*" >&2; fail=1; }

# grep_status ARGS...: runs grep, puts its output in $out and its status in $rc; 2+ is fatal.
grep_status() {
  set +e
  out=$(grep "$@" 2>&1)
  rc=$?
  set -e
  if [ "$rc" -ge 2 ]; then
    echo "public-workflows-check: grep failed ($rc)" >&2
    exit 2
  fi
}

# 0. scripts/release-notes.sh extracts one release's CHANGELOG section.
# Run before the file checks below so a missing script is reported with the other findings.
notes_sh=scripts/release-notes.sh
notes_tmp=$(mktemp -d)
trap 'rm -rf "$notes_tmp"' EXIT
cl=$notes_tmp/CHANGELOG.md
printf '%s\n' '# Changelog' '' '## Unreleased' '' '- next' '' '---' '' \
  '## [0.1.1] - 2026-12-01' '' '- b' '' '## [0.1.0] - 2026-11-30' '' '- a' '' '- a2' '' \
  '## [0.0.1] - 2026-01-01' '' '- old' '' '## [0.0.9] - 2026-01-02' '' > "$cl"
printf '%s\n' '## Unreleased' '- u' '## [0.3.0-rc.1] - 2026-12-02' '- r' > "$notes_tmp/rc.md"
printf '%s\n' '# Changelog' '' '## [0.1.0] - 2026-11-30' '' '- a' > "$notes_tmp/nounrel.md"
# notes_case NAME WANT_RC WANT_OUT VERSION [FILE]: the script's status and stdout must match.
notes_case() {
  local name=$1 want_rc=$2 want_out=$3 version=$4 file=${5:-$cl} got got_rc
  if [ ! -x "$notes_sh" ]; then bad "release-notes.sh: $notes_sh is missing or not executable ($name)"; return; fi
  set +e
  got=$("$notes_sh" "$version" "$file" 2>/dev/null)
  got_rc=$?
  set -e
  [ "$got_rc" -eq "$want_rc" ] || bad "release-notes.sh ($name): exit $got_rc, want $want_rc"
  [ "$got" = "$want_out" ] || bad "release-notes.sh ($name): printed [$got], want [$want_out]"
}
notes_case "a final's section" 0 "$(printf '%s\n\n%s' '- a' '- a2')" 0.1.0
notes_case "the version is not a prefix match" 0 '- b' 0.1.1
notes_case "a version with no section" 1 '' 9.9.9
notes_case "a final with only Unreleased" 1 '' 0.2.0
notes_case "an empty section" 1 '' 0.0.9
notes_case "an rc falls back to Unreleased" 0 '- next' 0.2.0-rc.1
notes_case "an rc with its own section" 0 '- r' 0.3.0-rc.1 "$notes_tmp/rc.md"
notes_case "an rc with no section and no Unreleased" 1 '' 0.2.0-rc.1 "$notes_tmp/nounrel.md"
notes_case "a version with a v" 2 '' v0.1.0
notes_case "a version that is not a version" 2 '' '0.1'
notes_case "a missing CHANGELOG" 1 '' 0.1.0 "$notes_tmp/none.md"

# 1. The nine workflow files, exactly.
want="audit-daily.yml ci.yml codeql.yml dco.yml dependency-review.yml fuzz.yml release.yml scorecard.yml upstream-parser.yml"
if [ -d "$wf" ]; then
  have=$(cd "$wf" && printf '%s\n' * | sort | tr '\n' ' ' | sed 's/ $//')
else
  have="(no $wf)"
fi
[ "$have" = "$want" ] || bad "workflow files are [$have], want [$want]"
for f in dependabot.yml PULL_REQUEST_TEMPLATE.md; do
  [ -f "$gh_dir/$f" ] || bad "$gh_dir/$f is missing"
done

# From here on the files must exist; stop before greps that would error on absence.
if [ "$fail" -ne 0 ]; then say "workflow files absent or wrong; later checks skipped"; exit 1; fi

# 2. Every `uses:` is pinned by a 40-hex commit SHA.
grep_status -rhoE 'uses: [^ ]+' "$wf"
uses=$out
[ "$rc" -eq 0 ] || bad "no 'uses:' found at all"
unpinned=$(printf '%s\n' "$uses" | sort -u | { grep -vE '@[0-9a-f]{40}( |$)' || [ $? -eq 1 ]; })
[ -z "$unpinned" ] || bad "actions not pinned by a 40-hex SHA: $unpinned"

# 3. Never pull_request_target.
grep_status -rn 'pull_request_target' "$gh_dir"
[ "$rc" -eq 1 ] || bad "pull_request_target appears: $out"

# 4. No secrets in ci.yml or fuzz.yml.
for f in ci.yml fuzz.yml; do
  grep_status -n 'secrets\.' "$wf/$f"
  [ "$rc" -eq 1 ] || bad "$f uses secrets.: $out"
done

# 5. The job-level names of ci.yml.
names=$(ruby -ryaml -e 'd = YAML.load_file(ARGV[0]); p d["jobs"].values.map { |j| j["name"] }.sort' "$wf/ci.yml")
want_names='["MSRV", "Rust", "Secret scan", "cargo audit", "cargo deny", "coverage", "linear", "linear-arm64", "no-js", "package", "spec", "wasm"]'
[ "$names" = "$want_names" ] || bad "ci.yml job names are $names, want $want_names"

# 6. YAML parses.
ruby -ryaml -e 'ARGV.each { |f| YAML.load_file(f) }; puts "yaml-ok"' "$wf"/*.yml "$gh_dir/dependabot.yml" >/dev/null \
  || bad "a workflow or dependabot.yml does not parse"

# 7. actionlint is clean.
if command -v actionlint >/dev/null 2>&1; then
  al=$(actionlint "$wf"/*.yml 2>&1) || bad "actionlint: $al"
else
  bad "actionlint is not installed"
fi

# 8. The MSRV job overrides rustup's toolchain-file choice.
grep_status -nE '^      RUSTUP_TOOLCHAIN: "1\.94"$' "$wf/ci.yml"
[ "$rc" -eq 0 ] || bad 'ci.yml has no RUSTUP_TOOLCHAIN: "1.94" in the MSRV job env'
grep_status -c 'RUSTUP_TOOLCHAIN' "$wf/ci.yml"
[ "$out" = 1 ] || bad "RUSTUP_TOOLCHAIN appears $out times in ci.yml, want 1"

# 9. cargo-fuzz is pinned.
grep_status -nE 'cargo install cargo-fuzz@0\.13\.2 --locked' "$wf/fuzz.yml"
[ "$rc" -eq 0 ] || bad "fuzz.yml does not install cargo-fuzz@0.13.2 --locked"

# 9b. No workflow installs a tool with `cargo install` unless it is pinned to an exact
# version and `--locked`.
grep_status -rnE 'cargo install ' "$wf"
if [ "$rc" -eq 0 ]; then
  loose=$(printf '%s\n' "$out" | { grep -vE 'cargo install [A-Za-z0-9_-]+@[0-9]+\.[0-9]+\.[0-9]+ .*--locked' || [ $? -eq 1 ]; })
  [ -z "$loose" ] || bad "cargo install without an exact @version and --locked: $loose"
fi

# 9c. The fuzz crate's own lock file is audited (it is a separate workspace, so the main
# audit never reads it), with a pinned cargo-audit.
grep_status -nE 'cargo audit .*--file fuzz/Cargo\.lock' "$wf/fuzz.yml"
[ "$rc" -eq 0 ] || bad "fuzz.yml does not run cargo audit on fuzz/Cargo.lock"
# The workflow's own text is matched literally, so single quotes are intended.
# shellcheck disable=SC2016
grep_status -nE 'tool: cargo-audit@(\$\{\{ env\.CARGO_AUDIT_VERSION \}\}|[0-9]+\.[0-9]+\.[0-9]+)$' "$wf/fuzz.yml"
[ "$rc" -eq 0 ] || bad "fuzz.yml does not install cargo-audit at a pinned version"
grep_status -nE '^  CARGO_AUDIT_VERSION: [0-9]+\.[0-9]+\.[0-9]+$' "$wf/fuzz.yml"
[ "$rc" -eq 0 ] || bad "fuzz.yml has no exact CARGO_AUDIT_VERSION"

# 10. The fuzz budget is shared out: the time per target is the budget divided by the number
# of targets, so the whole run fits within timeout-minutes.
grep_status -nE 'per_target=\$\(\( *FUZZ_BUDGET */ *n *\)\)' "$wf/fuzz.yml"
[ "$rc" -eq 0 ] || bad "fuzz.yml does not divide FUZZ_BUDGET by the number of targets (per_target=\$((FUZZ_BUDGET / n)))"
grep_status -nE -- '-max_total_time=\$\{per_target\}' "$wf/fuzz.yml"
[ "$rc" -eq 0 ] || bad "fuzz.yml does not run each target with -max_total_time=\${per_target}"
grep_status -nE 'cargo fuzz list' "$wf/fuzz.yml"
[ "$rc" -eq 0 ] || bad "fuzz.yml does not count the targets with cargo fuzz list"

# 11. The DCO check exempts a bot by the pull request's user login only, never by the commit's
# author name or email (either is whatever the committer typed).
grep_status -n 'github.event.pull_request.user.login' "$wf/dco.yml"
[ "$rc" -eq 0 ] || bad "dco.yml does not read github.event.pull_request.user.login"
# The $name and $email here are the workflow's own text, matched literally.
# shellcheck disable=SC2016
grep_status -nE '\[bot\]@users\.noreply|case +"\$name"|case +"\$email"' "$wf/dco.yml"
[ "$rc" -eq 1 ] || bad "dco.yml exempts by author name or email: $out"

# 12. release.yml: one approval gate for the release, per-platform image builds, minimal
# permissions, and the remap that cannot silently remap nothing. (The crates.io publish
# chain is 12g.)
rel=$wf/release.yml
grep_status -cE '^ *environment: release( +#.*)?$' "$rel"
[ "$out" = 1 ] || bad "release.yml has 'environment: release' $out times, want 1"
# Per-platform builds on native runners: no single build step lists two platforms; each one
# takes its platform from the job's matrix, and the manifest is joined by imagetools (12e).
grep_status -nE 'platforms:' "$rel"
[ "$rc" -eq 0 ] || bad "release.yml sets no platforms"
# The workflow's own text is matched literally, so single quotes are intended.
# shellcheck disable=SC2016
notmatrix=$(printf '%s\n' "$out" | { grep -vE 'platforms: \$\{\{ matrix\.platform \}\}$' || [ $? -eq 1 ]; })
[ -z "$notmatrix" ] || bad "release.yml has a platforms: that is not the matrix's one platform: $notmatrix"
grep_status -nE 'platforms: *linux/amd64,linux/arm64|platforms:.*,' "$rel"
[ "$rc" -eq 1 ] || bad "release.yml builds two platforms in one step: $out"
# The patterns below match the workflow's own text, so single quotes are intended.
# shellcheck disable=SC2016
grep_status -n 'CARGO_HOME:-\$HOME/\.cargo' "$rel"
# shellcheck disable=SC2016
[ "$rc" -eq 0 ] || bad 'release.yml has no ${CARGO_HOME:-$HOME/.cargo} in the path remap'
# shellcheck disable=SC2016
grep_status -nE '[^{]\$CARGO_HOME=' "$rel"
[ "$rc" -eq 1 ] || bad "release.yml remaps a bare CARGO_HOME (unset in some steps): $out"
# Per job: contents: write only in the release job, which has the environment and needs
# both build and image; the only trigger is a tag push.
rel_report=$(ruby -ryaml -e '
  d = YAML.load_file(ARGV[0])
  top = d["permissions"] || {}
  puts "top-write" if top.values.include?("write")
  d["jobs"].each do |name, j|
    perms = j["permissions"] || {}
    puts "#{name}-contents-write" if perms["contents"] == "write"
    puts "#{name}-environment" if j["environment"]
  end
  on = d[true] || d["on"] || {}
  puts "tag-only" if on.keys == ["push"] && on["push"]["tags"] == ["v*"]
  puts "release-needs=" + Array(d["jobs"]["release"] && d["jobs"]["release"]["needs"]).sort.join(",")
' "$rel" 2>&1) || bad "release.yml: the permission check failed: $rel_report"
want_report=$(printf '%s\n' 'tag-only' 'release-contents-write' 'release-environment' 'release-needs=build,image,publish-bin' \
  'publish-markdown-environment' 'publish-render-environment' 'publish-wasm-bundle-environment' \
  'publish-site-environment' 'publish-bin-environment' | sort)
got_report=$(printf '%s\n' "$rel_report" | sort)
[ "$got_report" = "$want_report" ] || bad "release.yml jobs: [$(printf '%s' "$got_report" | tr '\n' ' ')], want [$(printf '%s' "$want_report" | tr '\n' ' ')]"

# 12b. The first job of release.yml verifies the tag's signature (scripts/verify-release-tag.sh,
# after fetching the tag object), and every other job needs it, directly or through the chain.
# The Ruby text is matched literally, so single quotes are intended.
# shellcheck disable=SC2016
gate_report=$(ruby -ryaml -e '
  d = YAML.load_file(ARGV[0])
  jobs = d["jobs"]
  first = jobs.keys.first
  text = (jobs[first]["steps"] || []).map { |s| s["run"].to_s }.join("\n")
  fetch = text.index("git fetch --force origin \"refs/tags/$TAG:refs/tags/$TAG\"")
  verify = text.index("scripts/verify-release-tag.sh")
  puts "first-fetches-tag" if fetch
  puts "first-verifies" if verify
  puts "fetch-before-verify" if fetch && verify && fetch < verify
  puts "first-needs-nothing" if Array(jobs[first]["needs"]).empty?
  reach = lambda do |name, seen|
    Array(jobs[name]["needs"]).each do |n|
      next if seen.include?(n)
      seen << n
      reach.call(n, seen)
    end
    seen
  end
  jobs.keys.each do |name|
    next if name == first
    puts "#{name}-gated" if reach.call(name, []).include?(first)
  end
  puts "jobs=" + jobs.keys.sort.join(",")
' "$rel" 2>&1) || bad "release.yml: the tag-verification check failed: $gate_report"
gated=$(printf '%s\n' "$gate_report" | sed -n 's/^\(.*\)-gated$/\1/p' | sort | tr '\n' ',')
alljobs=$(printf '%s\n' "$gate_report" | sed -n 's/^jobs=//p' | tr ',' '\n')
first_job=$(ruby -ryaml -e 'puts YAML.load_file(ARGV[0])["jobs"].keys.first' "$rel" 2>&1) || bad "release.yml: no first job"
want_gated=$(printf '%s\n' "$alljobs" | { grep -vxF "$first_job" || [ $? -eq 1 ]; } | sort | tr '\n' ',')
for k in first-fetches-tag first-verifies fetch-before-verify first-needs-nothing; do
  printf '%s\n' "$gate_report" | grep -qxF "$k" || bad "release.yml: $k does not hold (the first job must fetch the tag object, then run scripts/verify-release-tag.sh, and need nothing)"
done
[ "$gated" = "$want_gated" ] || bad "release.yml: jobs gated by the first job are [$gated], want [$want_gated]"

# 12c. The signers file the verifier reads: one line per key, the owner's
# principal then a key of a known type and its base64, or the single pending marker until the
# owner's key is in.
signers=$gh_dir/allowed_signers
if [ ! -f "$signers" ]; then
  bad "$signers is missing"
else
  n=0
  while IFS= read -r line || [ -n "$line" ]; do
    n=$((n + 1))
    [ "$line" = OWNER-SIGNING-KEY-PENDING ] && continue
    printf '%s\n' "$line" | grep -qE '^[^ #]+ (ssh-ed25519|sk-ssh-ed25519@openssh\.com|ecdsa-sha2-nistp256|sk-ecdsa-sha2-nistp256@openssh\.com) [A-Za-z0-9+/]+=*( .*)?$' \
      || bad "$signers line $n is neither the pending marker nor 'principal key-type base64'"
  done < "$signers"
  [ "$n" -ge 1 ] || bad "$signers is empty"
fi

# 12d. The wasm search bundle. Its files are never committed (no binaries in git), so every
# job that compiles rubrica-site with its default features has them first: ci.yml's `wasm`
# job builds them with the pinned target and CLI (the version Cargo.toml pins for the
# wasm-bindgen crate), checks a second build gives the same bytes, tests the embed and
# uploads them; the jobs that build the workspace need it and download them. In release.yml
# the check job builds them itself (scripts/ci.sh), and the wasm job builds them once, records
# their sha256 and the CLI's, packs the bundle crate and attests the .wasm and the .crate; the
# binary builds on every platform download those files (12e).
bindgen=$(sed -n 's/^wasm-bindgen = "=\([0-9][0-9.]*\)"$/\1/p' Cargo.toml)
[ -n "$bindgen" ] || bad "Cargo.toml pins no exact wasm-bindgen version"
# The Ruby text is matched literally, so single quotes are intended.
# shellcheck disable=SC2016
wasm_report=$(ruby -ryaml -e '
  ci = YAML.load_file(ARGV[0])["jobs"]
  rel = YAML.load_file(ARGV[1])["jobs"]
  v = ARGV[2]
  install = "cargo install wasm-bindgen-cli@#{v} --locked"
  target = "rustup target add wasm32-unknown-unknown"
  runs = lambda { |j| (j["steps"] || []).map { |s| s["run"].to_s } }
  uses = lambda { |j, a| (j["steps"] || []).select { |s| s["uses"].to_s.start_with?(a + "@") } }
  w = ci["wasm"]
  if w
    r = runs.call(w).join("\n")
    puts "wasm-named" if w["name"] == "wasm"
    puts "wasm-target" if r.include?(target)
    puts "wasm-cli" if r.include?(install)
    puts "wasm-bundle" if r.include?("scripts/build-wasm.sh --bundle")
    puts "wasm-reproducible" if r.include?("scripts/wasm-reproducible.sh")
    puts "wasm-embed-test" if r.include?("cargo test --locked -p rubrica-wasm-bundle -p rubrica-site")
    up = uses.call(w, "actions/upload-artifact").find { |s| (s["with"] || {})["name"] == "wasm-bundle" }
    puts "wasm-upload" if up && up["with"]["if-no-files-found"] == "error"
  end
  %w[rust msrv linear no-js].each do |n|
    j = ci[n] or next
    puts "#{n}-needs-wasm" if Array(j["needs"]).include?("wasm")
    dl = uses.call(j, "actions/download-artifact").find do |s|
      (s["with"] || {})["name"] == "wasm-bundle" && s["with"]["path"] == "crates/rubrica-wasm-bundle/src"
    end
    puts "#{n}-downloads" if dl
  end
  %w[check wasm].each do |n|
    j = rel[n] or next
    r = runs.call(j).join("\n")
    puts "release-#{n}-target" if r.include?(target)
    puts "release-#{n}-cli" if r.include?(install)
  end
  # The bundle is built once, in the wasm job; the binary builds only download it (12e).
  if rel["build"] && rel["wasm"]
    bd = rel["build"]
    dl = uses.call(bd, "actions/download-artifact").find do |s|
      (s["with"] || {})["name"] == "wasm-bundle" && s["with"]["path"] == "crates/rubrica-wasm-bundle/src"
    end
    puts "release-bundle-before-binary" if Array(bd["needs"]).include?("wasm") && dl
  end
  b = rel["wasm"]
  if b
    r = runs.call(b).join("\n")
    puts "release-summary" if r.include?("GITHUB_STEP_SUMMARY") && r.include?("rubrica-search_bg.wasm") && r.include?("command -v wasm-bindgen")
    puts "release-package" if r.include?("cargo package --locked --allow-dirty -p rubrica-wasm-bundle")
    att = uses.call(b, "actions/attest-build-provenance").map { |s| (s["with"] || {})["subject-path"].to_s }
    puts "release-attests-wasm" if att.any? { |p| p.include?("crates/rubrica-wasm-bundle/src/rubrica-search_bg.wasm") }
    puts "release-attests-crate" if att.any? { |p| p.include?("target/package/rubrica-wasm-bundle-") && p.include?(".crate") }
  end
' "$wf/ci.yml" "$rel" "$bindgen" 2>&1) || bad "the wasm bundle check failed: $wasm_report"
for k in wasm-named wasm-target wasm-cli wasm-bundle wasm-reproducible wasm-embed-test wasm-upload \
  rust-needs-wasm rust-downloads msrv-needs-wasm msrv-downloads linear-needs-wasm linear-downloads \
  no-js-needs-wasm no-js-downloads release-check-target release-check-cli release-wasm-target \
  release-wasm-cli release-bundle-before-binary release-summary release-package \
  release-attests-wasm release-attests-crate; do
  printf '%s\n' "$wasm_report" | grep -qxF "$k" || bad "the wasm bundle: $k does not hold"
done

# 12e. The platforms (0.1.1). The binary builds natively on three targets (the static
# x86_64 and aarch64 Linux musl binaries and the unsigned macOS arm64 one), all three embed
# byte-identical bundle files (built once by the wasm job, passed as an artifact, and
# compared by sha256 before the build), and each tarball is attested. The image is built per
# platform on its native runner, pushed by digest, and joined by `imagetools create` into a
# manifest whose digest is attested. The Ruby text is matched literally.
# shellcheck disable=SC2016
plat_report=$(ruby -ryaml -e '
  jobs = YAML.load_file(ARGV[0])["jobs"]
  text = lambda { |j| (j["steps"] || []).map { |s| s["run"].to_s }.join("\n") }
  uses = lambda { |j, a| (j["steps"] || []).select { |s| s["uses"].to_s.start_with?(a + "@") } }
  b = jobs["build"]
  if b
    inc = ((b["strategy"] || {})["matrix"] || {})["include"] || []
    got = inc.map { |e| [e["target"], e["runner"], e["static"], e["short"]] }.sort_by(&:first)
    want = [
      ["aarch64-apple-darwin", "macos-14", false, "aarch64-apple-darwin"],
      ["aarch64-unknown-linux-musl", "ubuntu-24.04-arm", true, "aarch64-linux-musl"],
      ["x86_64-unknown-linux-musl", "ubuntu-latest", true, "x86_64-linux-musl"],
    ]
    puts "build-matrix" if got == want
    puts "build-runs-on-matrix" if b["runs-on"] == "${{ matrix.runner }}"
    t = text.call(b)
    puts "build-static-check" if t.include?("readelf -l") && t.include?("INTERP")
    puts "build-macos-check" if t.include?("otool -L") && t.include?("grep -v " + 39.chr + "^/usr/lib/" + 39.chr)
    puts "build-no-cross" unless t =~ /cargo[- ]zigbuild|\bcross\b/
    puts "build-no-bundle-build" unless t.include?("build-wasm.sh") || t.include?("wasm-bindgen-cli")
    puts "build-needs-wasm" if Array(b["needs"]).include?("wasm")
    w = jobs["wasm"]
    outs = (w && w["outputs"] || {}).keys.sort
    puts "wasm-outputs" if outs == %w[loader meta wasm]
    puts "build-compares-bundle" if %w[wasm loader meta].all? { |k| b.to_s.include?("needs.wasm.outputs.#{k}") } && t.include?("shasum -a 256")
    puts "build-attests-each" if uses.call(b, "actions/attest-build-provenance").any? { |s| (s["with"] || {})["subject-path"].to_s.include?("matrix.short") }
    puts "build-uploads-each" if uses.call(b, "actions/upload-artifact").any? { |s| (s["with"] || {})["name"].to_s.include?("matrix.short") }
  end
  puts "bundle-built-once" if jobs.count { |_, j| text.call(j).include?("scripts/build-wasm.sh --bundle") } == 1 && jobs["wasm"] && text.call(jobs["wasm"]).include?("scripts/build-wasm.sh --bundle")
  ib = jobs["image-build"]
  if ib
    inc = ((ib["strategy"] || {})["matrix"] || {})["include"] || []
    got = inc.map { |e| [e["platform"], e["runner"]] }.sort
    puts "image-matrix" if got == [["linux/amd64", "ubuntu-latest"], ["linux/arm64", "ubuntu-24.04-arm"]]
    puts "image-runs-on-matrix" if ib["runs-on"] == "${{ matrix.runner }}"
    puts "image-by-digest" if ib.to_s.include?("push-by-digest=true")
  end
  m = jobs["image"]
  if m
    puts "manifest-needs-image-build" if Array(m["needs"]).include?("image-build")
    puts "manifest-imagetools" if text.call(m).include?("docker buildx imagetools create")
    puts "manifest-attested" if uses.call(m, "actions/attest-build-provenance").any? { |s| (s["with"] || {})["subject-digest"].to_s.include?("steps.") }
  end
  r = jobs["release"]
  if r
    t = text.call(r)
    puts "release-three-tarballs" if %w[x86_64-linux-musl aarch64-linux-musl aarch64-apple-darwin].all? { |k| t.include?(k) }
    puts "release-merges-dist" if uses.call(r, "actions/download-artifact").any? { |s| (s["with"] || {})["pattern"] == "dist-*" && (s["with"] || {})["merge-multiple"] == true }
    puts "release-says-unsigned" if t.include?("unsigned") && t.include?("not notarised")
  end
' "$rel" 2>&1) || bad "the platform check failed: $plat_report"
for k in build-matrix build-runs-on-matrix build-static-check build-macos-check build-no-cross \
  build-no-bundle-build build-needs-wasm wasm-outputs build-compares-bundle build-attests-each \
  build-uploads-each bundle-built-once image-matrix image-runs-on-matrix image-by-digest \
  manifest-needs-image-build manifest-imagetools manifest-attested release-three-tarballs \
  release-merges-dist release-says-unsigned; do
  printf '%s\n' "$plat_report" | grep -qxF "$k" || bad "the platforms: $k does not hold"
done
grep_status -c 'target: ' "$rel"
[ "$out" = 3 ] || bad "release.yml has $out 'target: ' lines, want the 3 matrix targets"
grep_status -c 'ubuntu-24\.04-arm' "$rel"
[ "$out" -ge 1 ] || bad "release.yml has no ubuntu-24.04-arm runner"
grep_status -c 'imagetools create' "$rel"
[ "$out" = 1 ] || bad "release.yml has 'imagetools create' $out times, want 1"
# ci.yml: the parser and renderer ratio tests also run natively on an arm64 runner, with no
# emulation.
la_report=$(ruby -ryaml -e '
  j = YAML.load_file(ARGV[0])["jobs"]["linear-arm64"]
  exit 0 unless j
  puts "runner" if j["runs-on"] == "ubuntu-24.04-arm"
  t = (j["steps"] || []).map { |s| s["run"].to_s }.join("\n")
  puts "render" if t.include?("cargo test --locked -p rubrica-render --test linear --release")
  puts "markdown" if t.include?("cargo test --locked -p rubrica-markdown --test linear --release")
' "$wf/ci.yml" 2>&1) || bad "the linear-arm64 check failed: $la_report"
for k in runner render markdown; do
  printf '%s\n' "$la_report" | grep -qxF "$k" || bad "ci.yml linear-arm64: $k does not hold"
done
grep_status -c 'ubuntu-24\.04-arm' "$wf/ci.yml"
[ "$out" = 1 ] || bad "ci.yml has ubuntu-24.04-arm $out times, want 1"

# ci.yml: both linear jobs set the hosted-runner switch RUBRICA_LINEAR_CI=1 (a wider ratio and
# a best of five, read by the linear tests); nothing else in the workflows sets it, and the
# local gate does not either, so a local run keeps the strict limits.
lc_report=$(ruby -ryaml -e '
  jobs = YAML.load_file(ARGV[0])["jobs"]
  %w[linear linear-arm64].each do |n|
    v = (jobs[n] || {}).dig("env", "RUBRICA_LINEAR_CI")
    puts "#{n}-set" if v.to_s == "1"
  end
  jobs.each do |n, j|
    next if %w[linear linear-arm64].include?(n)
    puts "stray-#{n}" if j.to_s.include?("RUBRICA_LINEAR_CI")
  end
' "$wf/ci.yml" 2>&1) || bad "the RUBRICA_LINEAR_CI check failed: $lc_report"
for k in linear-set linear-arm64-set; do
  printf '%s\n' "$lc_report" | grep -qxF "$k" || bad "ci.yml: the ${k%-set} job does not set RUBRICA_LINEAR_CI=1"
done
printf '%s\n' "$lc_report" | grep -q '^stray-' && bad "ci.yml: a job other than linear and linear-arm64 sets RUBRICA_LINEAR_CI"
grep_status -rl 'RUBRICA_LINEAR_CI' "$wf" --exclude=ci.yml
[ "$rc" -eq 1 ] || bad "a workflow other than ci.yml names RUBRICA_LINEAR_CI: $out"
grep_status -l 'RUBRICA_LINEAR_CI' scripts/ci.sh
[ "$rc" -eq 1 ] || bad "scripts/ci.sh sets or names RUBRICA_LINEAR_CI; the local gate keeps the strict limits"

# 12f. docs/install.md carries the Homebrew formula template (the tap itself is the owner's).
grep_status -c 'class Rubrica < Formula' docs/install.md
[ "$out" = 1 ] || bad "docs/install.md has 'class Rubrica < Formula' $out times, want 1"

# 12g. The crates.io publish chain. Five jobs in the order the crates depend on each other
# (the bundle crate before rubrica-site, which depends on it), each needing only its
# predecessor (and the job that makes its input), each under the `crates-io` environment
# (the owner's approval), none run for a candidate tag, each skipping a crate that is already
# on crates.io, packing it with scripts/package-check.sh, publishing with Trusted Publishing
# (the first publish of a crate with the one-day token instead), checking that crates.io
# holds the packed bytes, and attesting the .crate. The GitHub release waits for the last. The
# bundle crate publishes the .crate the wasm job packed and attested, after checking the
# bytes it would publish are those. CI's `package` job packs every crate and builds each
# unpacked on its own. Nothing else publishes. The Ruby text is matched literally.
# shellcheck disable=SC2016
chain_report=$(ruby -ryaml -e '
  rel = YAML.load_file(ARGV[0])["jobs"]
  ci = YAML.load_file(ARGV[1])["jobs"]
  chain = [
    ["publish-markdown", "rubrica-markdown", "FIRST_PUBLISH_RUBRICA_MARKDOWN", []],
    ["publish-render", "rubrica-render", "FIRST_PUBLISH_RUBRICA_RENDER", ["publish-markdown"]],
    ["publish-wasm-bundle", "rubrica-wasm-bundle", "FIRST_PUBLISH_RUBRICA_WASM_BUNDLE", ["publish-render", "wasm"]],
    ["publish-site", "rubrica-site", "FIRST_PUBLISH_RUBRICA_SITE", ["publish-wasm-bundle"]],
    ["publish-bin", "rubrica", "FIRST_PUBLISH_RUBRICA", ["publish-site"]],
  ]
  puts "chain-order" if rel.keys.select { |k| k.start_with?("publish-") } == chain.map(&:first)
  text = lambda { |j| (j["steps"] || []).map { |s| s["run"].to_s }.join("\n") }
  uses = lambda { |j, a| (j["steps"] || []).select { |s| s["uses"].to_s.start_with?(a + "@") } }
  chain.each do |name, krate, first_var, prev|
    j = rel[name] or next
    t = text.call(j)
    all = j.to_s
    puts "#{name}-needs" if (Array(j["needs"]) - ["check"]).sort == prev.sort && (name != "publish-markdown" || Array(j["needs"]).include?("check"))
    puts "#{name}-environment" if j["environment"] == "crates-io"
    perms = j["permissions"] || {}
    puts "#{name}-permissions" if perms["id-token"] == "write" && perms["contents"] == "read" && perms["attestations"] == "write" && perms.size == 3
    puts "#{name}-no-rc" if j["if"].to_s.include?("!contains(github.ref_name, " + 39.chr + "-rc." + 39.chr + ")")
    puts "#{name}-published-check" if t.include?("https://crates.io/api/v1/crates/#{krate}/") && t.include?("already published")
    puts "#{name}-package-check" if t =~ /scripts\/package-check\.sh #{Regexp.escape(krate)}(\s|$)/
    puts "#{name}-oidc" if uses.call(j, "rust-lang/crates-io-auth-action").size == 1
    puts "#{name}-first-token" if all.include?("vars.#{first_var}") && all.include?("secrets.CRATES_IO_PUBLISH_NEW")
    puts "#{name}-publish" if t =~ /cargo publish --locked --no-verify -p #{Regexp.escape(krate)}( |$)/
    puts "#{name}-verify-download" if t.include?("https://static.crates.io/crates/#{krate}/#{krate}-") && t.include?("sha256sum") && t.include?("seq 12")
    att = uses.call(j, "actions/attest-build-provenance").map { |s| (s["with"] || {})["subject-path"].to_s }
    puts "#{name}-attests" if att.any? { |x| x.include?("target/package/#{krate}-") && x.end_with?(".crate") }
  end
  # The bundle crate publishes the attested bytes.
  b = rel["publish-wasm-bundle"]
  if b
    dl = uses.call(b, "actions/download-artifact")
    names = dl.map { |s| (s["with"] || {})["name"] }
    puts "bundle-downloads-attested" if names.include?("wasm-bundle") && names.include?("wasm-bundle-crate")
    puts "bundle-same-bytes" if text.call(b).include?("cmp ") && text.call(b).include?("gh attestation verify")
  end
  all_pub = rel.select { |_, j| text.call(j).include?("cargo publish") }.keys
  puts "publish-only-in-chain" if all_pub == chain.map(&:first)
  puts "environment-only-in-chain" if rel.select { |_, j| j["environment"] == "crates-io" }.keys == chain.map(&:first)
  r = rel["release"]
  if r
    puts "release-after-chain" if Array(r["needs"]).include?("publish-bin")
    puts "release-runs-for-rc" if r["if"].to_s.include?("always()") == false && r["if"].to_s.include?("!cancelled()") && r["if"].to_s.include?("needs.publish-bin.result") && r["if"].to_s.include?("-rc.")
  end
  # CI: every crate is packed and built on its own, with the wasm bundle downloaded first.
  p = ci["package"]
  if p
    t = text.call(p)
    puts "ci-package-needs-wasm" if Array(p["needs"]).include?("wasm")
    puts "ci-package-downloads" if uses.call(p, "actions/download-artifact").any? { |s| (s["with"] || {})["name"] == "wasm-bundle" && s["with"]["path"] == "crates/rubrica-wasm-bundle/src" }
    puts "ci-package-all" if chain.all? { |_, krate, _, _| t =~ /scripts\/package-check\.sh --patch-siblings #{Regexp.escape(krate)}(\s|$)/ }
    puts "ci-package-no-publish" unless p.to_s.include?("cargo publish")
  end
' "$rel" "$wf/ci.yml" 2>&1) || bad "the publish chain check failed: $chain_report"
chain_keys="chain-order publish-only-in-chain environment-only-in-chain release-after-chain release-runs-for-rc
  bundle-downloads-attested bundle-same-bytes ci-package-needs-wasm ci-package-downloads ci-package-all ci-package-no-publish"
for j in publish-markdown publish-render publish-wasm-bundle publish-site publish-bin; do
  for k in needs environment permissions no-rc published-check package-check oidc first-token publish verify-download attests; do
    chain_keys="$chain_keys $j-$k"
  done
done
for k in $chain_keys; do
  printf '%s\n' "$chain_report" | grep -qxF "$k" || bad "the publish chain: $k does not hold"
done
grep_status -c 'environment: crates-io' "$rel"
[ "$out" = 5 ] || bad "release.yml has 'environment: crates-io' $out times, want 5 (one per published crate)"
# The shell script behind the jobs and the CI job exists and is executable.
[ -x scripts/package-check.sh ] || bad "scripts/package-check.sh is missing or not executable"
# Only release.yml publishes.
grep_status -rln 'cargo publish' "$gh_dir"
[ "$out" = "$rel" ] || bad "cargo publish appears outside release.yml: $out"

# 12h. The upgrade test: the `check` job builds the candidate binary, then runs
# tools/upgrade-check.sh with it and the version (the script downloads the previous release,
# verifies its sha256 and attestation, and compares renders). The test mode (GH_NO_NETWORK,
# UPGRADE_PREVIOUS_BIN) appears in no workflow, so CI always downloads and verifies.
# The Ruby text is matched literally, so single quotes are intended.
# shellcheck disable=SC2016
upgrade_report=$(ruby -ryaml -e '
  d = YAML.load_file(ARGV[0])
  c = d["jobs"]["check"] || {}
  steps = c["steps"] || []
  runs = steps.map { |s| s["run"].to_s }
  build = runs.index { |r| r =~ /cargo build .*--release/ && r.include?("-p rubrica") }
  up = runs.index { |r| r.include?("tools/upgrade-check.sh") }
  puts "upgrade-in-check" if up
  puts "upgrade-after-build" if build && up && build < up
  puts "upgrade-version" if up && (runs[up].include?("$VERSION") || runs[up].include?("steps.tag.outputs.version"))
  puts "upgrade-token" if up && steps[up]["env"].to_h.key?("GH_TOKEN")
  puts "upgrade-no-test-mode" unless File.read(ARGV[0]).match?(/GH_NO_NETWORK|UPGRADE_PREVIOUS_BIN/)
' "$rel" 2>&1) || bad "the upgrade check failed: $upgrade_report"
for k in upgrade-in-check upgrade-after-build upgrade-version upgrade-token upgrade-no-test-mode; do
  printf '%s\n' "$upgrade_report" | grep -qxF "$k" || bad "release.yml: $k does not hold (the check job builds the candidate, then runs tools/upgrade-check.sh with GH_TOKEN, and never sets the test-mode variables)"
done
[ -x tools/upgrade-check.sh ] || bad "tools/upgrade-check.sh is missing or not executable"
# 12i. The coverage job. Coverage is measured in house: `cargo llvm-cov` over the workspace,
# the summary table written to the job summary, the HTML report kept as an artifact. No
# third-party service sees it: no uploader action, no token, no badge. The tool is installed
# at an exact version with --locked, every action is SHA-pinned (check 2), and the job needs
# the wasm bundle like every job that builds rubrica-site. coverage_findings FILE prints one
# key per property that holds; the planted variants below must each lose the key they break,
# so the check cannot pass on a job that does not have the shape.
# The Ruby text is matched literally, so single quotes are intended.
# shellcheck disable=SC2016
coverage_findings() {
  ruby -ryaml -e '
    j = (YAML.load_file(ARGV[0])["jobs"] || {})["coverage"]
    exit 0 unless j
    steps = j["steps"] || []
    runs = steps.map { |s| s["run"].to_s }
    text = runs.join("\n")
    uses = steps.map { |s| s["uses"].to_s.sub(/@.*/, "") }.reject(&:empty?)
    puts "named" if j["name"] == "coverage"
    puts "needs-wasm" if Array(j["needs"]).include?("wasm")
    dl = steps.find { |s| s["uses"].to_s.start_with?("actions/download-artifact@") }
    puts "downloads-bundle" if dl && dl["with"]["name"] == "wasm-bundle" && dl["with"]["path"] == "crates/rubrica-wasm-bundle/src"
    install = runs.index { |r| r =~ /cargo install cargo-llvm-cov@[0-9]+\.[0-9]+\.[0-9]+ .*--locked/ }
    run = runs.index { |r| r =~ /cargo llvm-cov(?! report)\b.*--workspace/ && r.include?("--locked") }
    puts "pinned-install" if install
    puts "workspace-run" if run
    puts "install-before-run" if install && run && install < run
    puts "component" if text.include?("rustup component add llvm-tools-preview")
    puts "summary" if runs.any? { |r| r.include?("cargo llvm-cov report") && r.include?("GITHUB_STEP_SUMMARY") }
    up = steps.find { |s| s["uses"].to_s.start_with?("actions/upload-artifact@") && s["with"]["name"] == "coverage-html" }
    puts "html-artifact" if up && up["with"]["path"].to_s.include?("target/llvm-cov/html") && up["with"]["if-no-files-found"] == "error"
    allowed = %w[actions/checkout actions/download-artifact actions/upload-artifact dtolnay/rust-toolchain Swatinem/rust-cache]
    puts "only-first-party-actions" if (uses - allowed).empty?
    puts "no-uploader" unless j.to_s =~ /codecov|coveralls|codacy|sonar|CODECOV|COVERALLS|secrets\.|token/i
  ' "$1"
}
coverage_keys="named needs-wasm downloads-bundle pinned-install workspace-run install-before-run component summary html-artifact only-first-party-actions no-uploader"
cov_report=$(coverage_findings "$wf/ci.yml" 2>&1) || bad "the coverage check failed: $cov_report"
for k in $coverage_keys; do
  printf '%s\n' "$cov_report" | grep -qxF "$k" || bad "ci.yml coverage: $k does not hold"
done
# Planted variants of the real file: each must lose exactly the key it breaks.
cov_variant() { # NAME KEY SED_EXPR: the variant of ci.yml built with the expression loses KEY
  local name=$1 key=$2 expr=$3 got
  sed "$expr" "$wf/ci.yml" > "$notes_tmp/cov.yml"
  if cmp -s "$wf/ci.yml" "$notes_tmp/cov.yml"; then bad "coverage variant ($name): the edit changed nothing"; return; fi
  got=$(coverage_findings "$notes_tmp/cov.yml" 2>&1) || { bad "coverage variant ($name): $got"; return; }
  if printf '%s\n' "$got" | grep -qxF "$key"; then bad "coverage variant ($name): still reports $key"; fi
}
if printf '%s\n' "$cov_report" | grep -qxF named; then
  cov_variant "an unpinned tool" pinned-install 's/cargo install cargo-llvm-cov@[0-9.]* /cargo install cargo-llvm-cov /'
  cov_variant "an unlocked tool" pinned-install 's/cargo install cargo-llvm-cov@\([0-9.]*\) --locked/cargo install cargo-llvm-cov@\1/'
  cov_variant "an uploader" only-first-party-actions 's#actions/upload-artifact@\([0-9a-f]*\) \# v7.0.2#codecov/codecov-action@\1 \# v5#'
  # shellcheck disable=SC2016
  cov_variant "a token" no-uploader 's#cargo llvm-cov report --html#cargo llvm-cov report --html; curl -F t=$CODECOV_TOKEN x#'
  cov_variant "no job summary" summary 's/GITHUB_STEP_SUMMARY/SOME_FILE/'
  cov_variant "no workspace" workspace-run 's/cargo llvm-cov --workspace/cargo llvm-cov/'
fi
grep_status -rniE 'codecov|coveralls|codacy|sonarcloud' "$wf"
[ "$rc" -eq 1 ] || bad "a workflow names a coverage service: $out"

# 13. The Dockerfile's base image is pinned by digest, and it runs as a non-root user.
[ -f Dockerfile ] || { bad "Dockerfile is missing"; exit 1; }
first=$(head -1 Dockerfile)
printf '%s\n' "$first" | grep -qE '^FROM gcr\.io/distroless/static-debian12:nonroot@sha256:[0-9a-f]{64}$' \
  || bad "Dockerfile: the first line is not a digest-pinned distroless FROM"
grep_status -nE '^USER nonroot:nonroot$' Dockerfile
[ "$rc" -eq 0 ] || bad "Dockerfile: no USER nonroot:nonroot"
grep_status -nE '^ENTRYPOINT \["/usr/local/bin/rubrica"\]$' Dockerfile
[ "$rc" -eq 0 ] || bad "Dockerfile: the entrypoint is not /usr/local/bin/rubrica"

if [ "$fail" -ne 0 ]; then exit 1; fi
say "all green"
