<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/rubrica-lockup-dark.png">
    <img src="assets/rubrica-lockup-light.png" alt="Rubrica" width="380">
  </picture>
</p>

<p align="center">
  <strong>A wiki in a git repository, written in Rust.</strong><br>
  Markdown pages in, a static site out.
</p>

<p align="center">

[![CI](https://github.com/justin-stanley/rubrica/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/justin-stanley/rubrica/actions/workflows/ci.yml)
[![Fuzz](https://github.com/justin-stanley/rubrica/actions/workflows/fuzz.yml/badge.svg?branch=main)](https://github.com/justin-stanley/rubrica/actions/workflows/fuzz.yml)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/justin-stanley/rubrica/badge)](https://scorecard.dev/viewer/?uri=github.com/justin-stanley/rubrica)
[![License: AGPL-3.0-only](https://img.shields.io/badge/license-AGPL--3.0--only-blue.svg)](LICENSE)
[![MSRV 1.94](https://img.shields.io/badge/MSRV-1.94-orange.svg)](#minimum-supported-rust-version)

</p>

---

Rubrica turns a git repository of Markdown pages into a wiki. Point it at a checkout and it
checks every link and anchor, then builds a site any file server can host. A server and
editing in the browser come later; see [Status](#status).

The name is Latin for red ochre, the earth pigment of red brick and of the red headings
(*rubrics*) scribes painted into manuscripts. The mark is a scribe's capital beside a brick
wall: a wiki is built the same way, a page at a time.

## What it does

- **Checks the repository.** Dead links and anchors, relative links, front matter, duplicate
  section ids, and paths that collide or run too long. Text or stable JSON output.
- **Builds a static site** from a commit, never from uncommitted files, with every page's
  Markdown source published beside it, recent changes from git, and a 404 page.
- **Owns its Markdown.** CommonMark 0.31.2 and the GFM extensions, passing both spec suites
  byte for byte, with a compatibility profile for wikis moving off Wiki.js.
- **Stays linear.** Every input shape that could make rendering superlinear has a bound, and a
  test checks that time grows in step with the input.
- **Ships no script of its own.** Pages carry a `script-src 'none'` Content-Security-Policy and
  code is highlighted at build time; the search page runs only Rubrica's own WebAssembly,
  checked by hash.
- **Releases you can verify.** Release tags are signed, and every binary, image and crate is
  attested.

## Quick start

```sh
rubrica check path/to/wiki
rubrica build path/to/wiki out/
```

Serve `out/` with any file server that tries `{path}.html`, then `{path}`, then
`{path}/index.html`.

## Status

Pre-release. 0.1.0 is the first release: `rubrica check` and `rubrica build`. The milestones:

| | |
|---|---|
| 0.1 | static: a checker and a site builder |
| 0.2 | server: `rubrica serve` renders the repository on request |
| 0.3 | editing, in the browser, saved as commits |
| 0.4 | permissions: who may read and who may edit |
| 0.5 | search and structure |
| 0.6 | collaboration |
| 0.7 | import and export |
| 1.0 | stable |

See [design/ROADMAP.md](design/ROADMAP.md).

## Install

Nothing is published yet. From 0.1.0 (details in [docs/install.md](docs/install.md)):

- **GitHub release**, every tarball attested; verify a download with
  `gh attestation verify <tarball> --repo justin-stanley/rubrica`. Static Linux x86_64 and
  aarch64 binaries, and a macOS Apple Silicon binary on a best-effort basis.
- **Container image**, `ghcr.io/justin-stanley/rubrica`, for x86_64 and aarch64.
- **`cargo install rubrica`**.

## `rubrica check`

```
rubrica check [--strict] [--same-page] [--wikijs-limits] [--format text|json] <repo-dir>
```

Checks a repository checkout: dead internal links and anchors, relative links to pages, front
matter, section ids across the repository, and paths (case collisions, reserved names, paths too
long). A dead anchor is a warning, and an error under `--strict`, which also fails on any
warning. `--same-page` also checks anchors that point within the same page. `--wikijs-limits`
adds what matters only while a Wiki.js instance imports the repository. `--format json` is
stable.

Exit code 0 when the repository is clean (or has warnings only), 1 when a finding fails the
check (a directory with no published page is one), 2 when the directory cannot be read.

## `rubrica build`

```
rubrica build [--rev REV] [--subtree DIR] [--config FILE] <repo-dir> <out-dir>
```

Builds the repository's git tree at a commit (`HEAD` by default; a commit id, a branch or a tag
also work), or one directory of it (`--subtree`), into a static site. The tree is a snapshot, so
nothing uncommitted is built, and no symbolic link or submodule is ever read. `out-dir` must not
exist yet or be empty. The site is written into a temporary directory beside it and renamed into
place, or removed on any failure.

Exit code 0 on success, 1 when the build is refused (the findings are printed as `check` prints
them; nothing is written), 2 when the repository or the output directory cannot be used.

The output has, per page, `<path>.html` and `<path>.md` (the source as committed), and also
`index.html` and `home.html` for the home page, `_search.html` and `_search.json`,
`_recent.html` (recent changes, from git), `404.html`, `_health`, and `_assets/`. Code is
highlighted at build time; Mermaid fences are written as source.

> [!IMPORTANT]
> Every page's source is published beside it, so whatever a rendered page hides (HTML comments,
> every front-matter field) is readable at `/<page>.md`. Put nothing in a page that its readers
> should not see.

The build is bounded. It refuses a repository over 250,000 entries, 16 MiB of paths or 128 MiB
of content, an asset over 2 MiB or more than 50 MB of assets in all, and a page over 4 MiB.
`--config` takes a `site.toml` (the site's name, origin, footer and header links, and the Edit
and History links).

A build wants the whole history, because "last updated" and the recent changes come from git. A
shallow clone builds, with a warning.

## Markdown

CommonMark 0.31.2 plus GFM tables, strikethrough, task lists, footnotes and extended autolinks.
The parser is `rubrica-markdown`, whose block and inline parsers derive from pulldown-cmark
0.13.6 by Raph Levien and contributors (MIT; see [NOTICE](NOTICE)). A compatibility profile
exists for wikis imported from Wiki.js (`render = "wikijs-compat"`). What Rubrica renders, and
where it knowingly differs from other implementations, is specified in [docs/spec/](docs/spec/).

## Security

Report vulnerabilities as [SECURITY.md](SECURITY.md) describes. Every shape of input that could
make the build superlinear has a bound and a finding, and the `linear` tests check that time
stays linear. No page carries JavaScript; the only script is the loader for Rubrica's own
WebAssembly, on the search page, checked by hash. Rendered HTML passes through a sanitizer. See
also [PRIVACY.md](PRIVACY.md).

## Minimum supported Rust version

Rubrica builds with Rust 1.94 or newer (`rust-version` in `Cargo.toml`); CI checks it on every
change. Development uses the toolchain pinned in `rust-toolchain.toml`.

## Developing

The gate is `scripts/ci.sh`; it is what CI runs, and it ends with `ci: all green`.
`rust-toolchain.toml` pins the toolchain. Besides the build and the tests, the gate runs two
guards of the repository's own (`scripts/crate-paths.sh` for what a crate may read, and
`tools/public-workflows-check.sh` for the workflows), and CI measures coverage in house with
`cargo-llvm-cov`, shared with no service. Every commit needs a DCO `Signed-off-by`. Rubrica is
developed with coding agents under review; [CONTRIBUTING.md](CONTRIBUTING.md) says how that
works.

## Licence

AGPL-3.0-only; see [LICENSE](LICENSE). Third-party notices are in [NOTICE](NOTICE). The logo
is part of the repository under the same licence; [assets/](assets/) holds every variant and
the script that builds them.
