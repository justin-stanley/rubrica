# Contributing to Rubrica

Thanks for looking. Rubrica renders other people's text and builds sites from other people's
repositories, so the bar for a change is plain: **it is tested, it passes the gate, and it says
why.**

Development happens here, on GitHub: `main` is the source of truth, and every change reaches it
through a pull request.

## Before you start

- **Bugs**: open an issue with the bug form: what you expected, what happened, and the
  smallest repository or Markdown file that shows it.
- **Security issues**: don't open an issue. Use
  [private vulnerability reporting](https://github.com/justin-stanley/rubrica/security/advisories/new);
  [SECURITY.md](SECURITY.md) says what to include.
- **Features**: open an issue with the feature form first, so we can agree on the shape before
  you write it.
- **Questions**: ask in Discussions; the [docs](docs/) answer many.

## Sending a change

1. **Fork** the repository, or, if you have write access, **branch** from `main`.
2. Make the change with tests (below), and run the gate locally.
3. Open a **pull request into `main`**. It is squash-merged, so its title becomes the commit
   subject on `main`: say what it does, in the imperative. The template lists what a pull
   request needs.
4. Sign off every commit (below), and answer review. Expect more than one round.

### Required checks

CI runs on every pull request, forks included, and all of these must pass before it merges:

| Job | What it holds |
|---|---|
| `Rust` | `scripts/ci.sh`, the gate below |
| `MSRV` | build and tests on the minimum supported Rust version |
| `spec` | the CommonMark and GFM spec examples |
| `linear`, `linear-arm64` | the parser, renderer and site build stay linear in their input, on x86_64 and arm64 |
| `no-js` | no JavaScript in the repository or in a built site |
| `package` | every crate packs to its `PACKAGE-FILES` and builds on its own |
| `wasm` | the search page's WebAssembly bundle builds, reproducibly, within its size budget |
| `cargo deny`, `cargo audit` | licences, bans, sources and advisories |
| `Secret scan` | no secret in the tree or the history |
| `DCO` | every commit is signed off |
| `Dependency review` | no vulnerable or disallowed dependency enters |
| `CodeQL` | static analysis of the Rust code and the workflows |

The `coverage` job also runs; see [Coverage](#coverage). It reports, it does not gate.

## The gate

`scripts/ci.sh` is what the `Rust` job runs, and what you run locally before you push:

```sh
scripts/ci.sh   # ends with "ci: all green"
```

The gate builds the search page's WebAssembly bundle, so it needs two tools and exits 1
without them: the `wasm32-unknown-unknown` target on the pinned toolchain, and the
`wasm-bindgen` command-line tool at the exact version `Cargo.lock` pins for the crate. One
command each:

```sh
rustup target add wasm32-unknown-unknown --toolchain 1.97.0
cargo install wasm-bindgen-cli@0.2.126 --locked
```

It also needs `ruby` and [`actionlint`](https://github.com/rhysd/actionlint), which the
workflows check uses.

The gate runs `cargo fmt`, a build, the tests, clippy with `-D warnings`, rustdoc with
`-D warnings`, `cargo deny` and `cargo audit`. It also runs two guards of the repository's
own: `scripts/crate-paths.sh` (no crate reads a file outside itself or one it does not ship;
`scripts/test-crate-paths.sh` tests it against planted bad cases) and
`tools/public-workflows-check.sh` (every action pinned by commit SHA, the shape of each
workflow, the release and the coverage job). The toolchain is pinned in
`rust-toolchain.toml`; `rustup` picks it up. Work in a git worktree of your own, and don't
point `CARGO_TARGET_DIR` at a directory another checkout uses: the script refuses to run that
way, because a shared target directory once ran another checkout's stale test binary.

### Coverage

Coverage is measured in house, with [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov),
and shared with no service: the `coverage` job in CI runs it over the whole workspace, writes
the summary table into the run's summary page, and keeps the HTML report as the
`coverage-html` artifact of the run. To see it locally:

```sh
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov@0.9.1 --locked
cargo llvm-cov --workspace --all-features --locked --open
```

Look at it when you write tests: a new branch with no test is the thing to find.

## What a good pull request has

- **One concern.** A squash-merge makes one commit of it.
- **Tests first.** Write the test, watch it fail for the reason you expect, then make it pass.
  A test that still passes with the fix reverted proves nothing.
- **A CHANGELOG entry** under *Unreleased* in [CHANGELOG.md](CHANGELOG.md): what changed, and
  for a defect, how it was established.
- **Docs** for anything a user can see.

Pull requests are reviewed until a full pass finds nothing above a nit. Expect more than one
round: later passes regularly find problems in earlier fixes.

Dependabot's pull requests (Cargo and GitHub Actions, weekly, minor and patch grouped) need
no changelog entry or sign-off. They are reviewed for what they pull in, and merged on GitHub
once the checks pass.

## Rules for the code

- **No JavaScript.** Rubrica's pages carry none; the one exception is the loader
  generated for Rubrica's own WebAssembly.
- **No C or C++.** A dependency that compiles C or C++ (a `-sys` crate with a build script,
  `cc`, `cmake`, OpenSSL) is not accepted.
- Every input shape that could be superlinear has a bound and a finding.

## Sign-off (DCO)

Every commit carries a `Signed-off-by` line that matches the commit's author:

```
Signed-off-by: Your Name <you@example.com>
```

`git commit -s` adds it; if you forgot, `git rebase --signoff main` adds it to a branch. This
is the [Developer Certificate of Origin](https://developercertificate.org): a statement that
you wrote the change or have the right to submit it under this project's licence. The `DCO`
check on every pull request enforces it. The sign-off is provenance, not a security control;
nothing in a deployment trusts it.

## Releases

A version is a git tag, `vX.Y.Z`. Every release is tagged first as a candidate,
`vX.Y.Z-rc.N`: the candidate is built and attested like a release and published as a GitHub
pre-release, with its container image tagged `X.Y.Z-rc.N` so the soak runs the image that
would ship. It is never published to crates.io, and `latest` only ever names a final release.
Every final release has a section in [CHANGELOG.md](CHANGELOG.md); a candidate's notes come
from its own section when it has one, and otherwise from `Unreleased`. Fixes found during the
soak go to `main` as ordinary pull requests, and a new candidate is tagged.

Release tags are signed. A release is made only from an annotated tag with an SSH signature
from a key listed in [.github/allowed_signers](.github/allowed_signers), made by the tagger
that file names for the key. The principal is the owner's commit e-mail,
`233813550+justin-stanley@users.noreply.github.com`. Tag with `gpg.format=ssh` and your
signing key:

```sh
git config gpg.format ssh
git config user.signingkey ~/.ssh/id_ed25519.pub
git tag -s vX.Y.Z -m "Rubrica X.Y.Z"
```

The release workflow's first job runs `scripts/verify-release-tag.sh` on the pushed tag and
refuses any other: a lightweight tag, an unsigned tag, a tag signed with a key that is not
listed, or a listed key under another e-mail. No build job runs after a refusal.
The signers file lists the maintainer's release key (an SSH Ed25519 key used only to sign
Rubrica's release tags). A new or replacement key is added to `.github/allowed_signers` in a
reviewed pull request, before the first tag it signs.

## Licensing

Rubrica is [AGPL-3.0-only](LICENSE). By submitting a contribution you agree it is licensed the
same way. There is no CLA, and no intent to relicense.

## Coding agents

Rubrica is developed with coding agents, under review: an agent writes, and cold reviewers
read the result until nothing above a nit is left. Patches written by agents are welcome under
the same review as any other: the same tests-first rule, the same gate, the same sign-off, and
the same rounds of review until a full pass finds nothing above a nit. Whoever opens the pull
request answers for it, so read the diff before you do.
