---
title: Install
description: Three ways to get Rubrica, and how to verify a download.
---

# Install

Rubrica is one binary, `rubrica`. There are three ways to get it. Which of them exist for a
given version is listed under each route.

## The release binary

Each GitHub release carries a tarball per platform, with its `.sha256`:

- `rubrica-X.Y.Z-x86_64-linux-musl.tar.gz` and `rubrica-X.Y.Z-aarch64-linux-musl.tar.gz`:
  statically linked Linux binaries.
- `rubrica-X.Y.Z-aarch64-apple-darwin.tar.gz`: macOS on Apple Silicon, built on a best-effort
  basis. It is unsigned and not notarised, so macOS asks once before it runs.

Releases are built by the project's release workflow, natively on a runner of each platform,
and it attests every tarball. Verify what you downloaded before you unpack it:

```sh
gh attestation verify rubrica-X.Y.Z-aarch64-linux-musl.tar.gz --repo justin-stanley/rubrica
```

The command succeeds only if the file was built by that repository's workflow from the tagged
commit. If it fails, do not run the file. A release's tag is an annotated tag signed with an SSH
key; check it in a clone with `git -c gpg.ssh.allowedSignersFile=.github/allowed_signers verify-tag vX.Y.Z`.

From 0.1.0: Linux x86_64 and aarch64, and macOS on Apple Silicon on a best-effort basis.

There is no Windows build and no build for Intel Macs.

### Homebrew

The tap `justin-stanley/homebrew-rubrica` is maintained by hand from this template; unsigned:
macOS asks once. For each release, set `version`, and set each `sha256` from that tarball's
`.sha256` asset:

```ruby
class Rubrica < Formula
  desc "A wiki in Rust that renders a Markdown git repository"
  homepage "https://github.com/justin-stanley/rubrica"
  version "X.Y.Z"
  license "AGPL-3.0-only"

  on_macos do
    on_arm do
      url "https://github.com/justin-stanley/rubrica/releases/download/v#{version}/rubrica-#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "SHA256-FROM-THE-SHA256-ASSET"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/justin-stanley/rubrica/releases/download/v#{version}/rubrica-#{version}-aarch64-linux-musl.tar.gz"
      sha256 "SHA256-FROM-THE-SHA256-ASSET"
    end
    on_intel do
      url "https://github.com/justin-stanley/rubrica/releases/download/v#{version}/rubrica-#{version}-x86_64-linux-musl.tar.gz"
      sha256 "SHA256-FROM-THE-SHA256-ASSET"
    end
  end

  def install
    bin.install "rubrica"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/rubrica --version")
  end
end
```

## From crates.io

```sh
cargo install rubrica --locked
```

Available from 0.1.0. The repository pins its toolchain in `rust-toolchain.toml`.

The release workflow publishes the workspace's crates (`rubrica-markdown`, `rubrica-render`,
`rubrica-wasm-bundle`, `rubrica-site` and `rubrica`) in that order, each after the owner
approves it, and a crate already published at a version is skipped, so a failed run is run
again. Release candidates (`vX.Y.Z-rc.N`) are never published to crates.io. The
`rubrica-wasm-bundle` crate holds the search page's WebAssembly module and its loader, built by
the same workflow and attested like the binaries; check the `.crate` crates.io serves against
the attestation of the release that published it.

## The container image

```sh
docker pull ghcr.io/justin-stanley/rubrica
```

Available from 0.1.0, for x86_64 and aarch64. The image holds
only the release binary, runs as a non-root user and writes nothing to its root filesystem, so
run it with `--read-only`:

```sh
docker run --rm --read-only ghcr.io/justin-stanley/rubrica --version
```

Pull by digest in anything you deploy, and verify the image's attestation the same way as a
binary:

```sh
gh attestation verify oci://ghcr.io/justin-stanley/rubrica@sha256:<digest> --repo justin-stanley/rubrica
```

## Upgrades do not change your site

Before a release is published, its candidate binary renders the specification examples and
this documentation, and the output is compared byte for byte with what the previous final
release renders; the previous release's checksum and attestation are verified first. A
difference fails the release unless the changelog's notes for that version hold a line starting
`- Render change:` that names it, so a change to the generated HTML is never silent. The
check is `tools/upgrade-check.sh` in the source repository; to see what a new binary changes in
your own wiki, run `tools/render-diff.sh <old-binary> <new-binary> <your-wiki>`.

## Check that it works

```sh
rubrica --version
rubrica check --strict path/to/your/wiki
```

The second command prints nothing to fix on a clean repository and exits 0.
