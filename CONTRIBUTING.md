# Contributing to Rubrica

Thanks for looking. Rubrica renders other people's text and builds sites from other people's
repositories, so the bar for a change is plain: **it is tested, it passes the gate, and it says
why.**

## Before you start

- **Bugs**: open an issue with what you expected, what happened, and the smallest repository or
  Markdown file that shows it.
- **Security issues**: don't open an issue. See [SECURITY.md](SECURITY.md).
- **Features**: open an issue first, so we can agree on the shape before you write it.

## The gate

Every change passes `scripts/ci.sh`, which is what CI runs:

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

The gate runs `cargo fmt`, a build, the tests, clippy with `-D warnings`, rustdoc with `-D warnings`,
`cargo deny` and `cargo audit`, and a check that no test reads outside its own crate. The
toolchain is pinned in `rust-toolchain.toml`; `rustup` picks it up. Work in a git worktree of
your own, and don't point `CARGO_TARGET_DIR` at a directory another checkout uses: the script
refuses to run that way, because a shared target directory once ran another checkout's stale
test binary.

## What a good pull request has

- **One concern.** The pull request is squash-merged, so its title becomes the commit subject.
- **Tests first.** Write the test, watch it fail for the reason you expect, then make it pass.
  A test that still passes with the fix reverted proves nothing.
- **A CHANGELOG entry** under *Unreleased* in [CHANGELOG.md](CHANGELOG.md): what changed, and
  for a defect, how it was established.
- **Docs** for anything a user can see.

Pull requests are reviewed until a full pass finds nothing above a nit. Expect more than one
round: later passes regularly find problems in earlier fixes.

## Rules for the code

- **No JavaScript.** Rubrica's pages carry none; the one exception, from 0.1.2, is the loader
  generated for Rubrica's own WebAssembly.
- **No C or C++.** A dependency that compiles C or C++ (a `-sys` crate with a build script,
  `cc`, `cmake`, OpenSSL) is not accepted.
- Every input shape that could be superlinear has a bound and a finding.

## Sign-off (DCO)

Every commit carries a `Signed-off-by` line that matches the commit's author:

```
Signed-off-by: Your Name <you@example.com>
```

`git commit -s` adds it. This is the [Developer Certificate of Origin](https://developercertificate.org):
a statement that you wrote the change or have the right to submit it under this project's
licence. A check on every pull request enforces it. The sign-off is provenance, not a security
control; nothing in a deployment trusts it.

## Releases

A version is a git tag, `vX.Y.Z`. Every release is tagged first as a candidate,
`vX.Y.Z-rc.N`: the candidate is built and attested like a release and published as a GitHub
pre-release, with its container image tagged `X.Y.Z-rc.N` so the soak runs the image that
would ship. It is never published to crates.io, and `latest` only ever names a final release.
Every final release has a section in [CHANGELOG.md](CHANGELOG.md); a candidate's notes come
from its own section when it has one, and otherwise from `Unreleased`. Fixes found during the soak go to `main` as ordinary pull
requests, and a new candidate is tagged.

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

## Licensing

Rubrica is [AGPL-3.0-only](LICENSE). By submitting a contribution you agree it is licensed the
same way. There is no CLA, and no intent to relicense.

## Coding agents

Rubrica is developed with coding agents, under review: an agent writes, and cold reviewers
read the result until nothing above a nit is left. Patches written by agents are welcome under
the same review as any other.
