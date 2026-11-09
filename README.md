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
[![Coverage](https://img.shields.io/endpoint?url=https://justin-stanley.github.io/rubrica/coverage/badge.json)](https://justin-stanley.github.io/rubrica/coverage/)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/justin-stanley/rubrica/badge)](https://scorecard.dev/viewer/?uri=github.com/justin-stanley/rubrica)
[![License: AGPL-3.0-only](https://img.shields.io/badge/license-AGPL--3.0--only-blue.svg)](LICENSE)
[![MSRV 1.94](https://img.shields.io/badge/MSRV-1.94-orange.svg)](#minimum-supported-rust-version)

</p>

---

Rubrica turns a git repository of Markdown pages into a wiki. `rubrica check` finds dead links,
dead anchors and bad paths. `rubrica build` writes a static site that any file server can host.
A server (0.2) and editing in the browser (0.3) come next; see [Status](#status).

## What it does

- **Checks the repository.** Dead links and anchors, relative links, front matter, duplicate
  section ids, and paths that collide or run too long. Output is text or JSON; the JSON format is
  stable.
- **Builds from a commit, never from the working tree.** Each page ships with its Markdown
  source, a recent-changes page comes from git history, and a 404 page is included.
- **Owns its Markdown parser.** CommonMark 0.31.2 and the GFM extensions, passing both spec
  suites byte for byte. A compatibility profile reproduces Wiki.js output for wikis moving off it.
- **Renders in linear time.** Every input shape that could make rendering superlinear has a
  bound, and a test fails if time grows faster than the input.
- **Ships no JavaScript of its own.** Pages carry a `script-src 'none'` Content-Security-Policy.
  Code is highlighted at build time. The search page runs Rubrica's own WebAssembly through one
  generated loader, which the build checks by hash.
- **Releases are verifiable.** Tags are signed. Every binary, image and crate carries a build
  attestation.

## Quick start

```sh
cargo install rubrica --locked
rubrica check path/to/wiki
rubrica build path/to/wiki out/
```

Serve `out/` with a file server that resolves `{path}.html`, then `{path}`, then
`{path}/index.html`.

## Install

Details in [docs/install.md](docs/install.md).

- **GitHub release.** Static binaries for Linux x86_64 and aarch64, and a macOS Apple Silicon
  binary (built and attested, not signed or notarised). Verify a download with
  `gh attestation verify <tarball> --repo justin-stanley/rubrica`.
- **Container image.** `ghcr.io/justin-stanley/rubrica`, for linux/amd64 and linux/arm64.
- **crates.io.** `cargo install rubrica --locked`.

## Status

0.1.0 is the first release. It ships `rubrica check` and `rubrica build`.

| Version | Delivers |
|---|---|
| 0.1 | a checker and a static site builder |
| 0.2 | `rubrica serve`: renders the repository on request |
| 0.3 | editing in the browser, saved as commits |
| 0.4 | permissions: who may read, who may edit |
| 0.5 | search and structure |
| 0.6 | collaboration |
| 0.7 | import and export |
| 1.0 | stable |

See [design/ROADMAP.md](design/ROADMAP.md).

## `rubrica check`

```
rubrica check [--strict] [--same-page] [--wikijs-limits] [--format text|json] <repo-dir>
```

Checks a repository checkout for dead internal links and anchors, relative links to pages, front
matter, duplicate section ids, and path problems (case collisions, reserved names, paths over the
limit).

- A dead anchor is a warning. `--strict` makes every warning an error.
- `--same-page` also checks anchors that point within their own page.
- `--wikijs-limits` adds the checks that matter only while a Wiki.js instance imports the
  repository.
- `--format json` output is stable across releases.

Exit codes: 0 when the repository is clean or has only warnings; 1 when a finding fails the check;
2 when the directory cannot be read.

## `rubrica build`

```
rubrica build [--rev REV] [--subtree DIR] [--config FILE] <repo-dir> <out-dir>
```

Builds the git tree at a commit into a static site. `--rev` takes a commit id, branch or tag
(default `HEAD`). `--subtree` builds one directory of the tree. The build reads the committed
tree only: uncommitted files, symbolic links and submodules are never read. `out-dir` must be
new or empty. The site is written to a temporary directory beside it and renamed into place; on
failure the temporary directory is removed.

Exit codes: 0 on success; 1 when the build is refused (findings are printed as `check` prints
them, and nothing is written); 2 when the repository or the output directory cannot be used.

Output, per page: `<path>.html` and `<path>.md` (the source as committed). Site-wide:
`index.html` and `home.html` for the home page, `_search.html` and `_search.json`,
`_recent.html`, `404.html`, `_health`, and `_assets/`. Code is highlighted at build time.
Mermaid fences are written as source.

> [!IMPORTANT]
> Every page's source is published beside it. Anything a rendered page hides, such as HTML
> comments and front-matter fields, is readable at `/<page>.md`. Put nothing in a page that its
> readers should not see.

Limits. The build refuses a repository over 250,000 entries, 16 MiB of paths, or 128 MiB of
content; an asset over 2 MiB; more than 50 MB of assets in total; or a page over 4 MiB.

`--config` takes a `site.toml` that sets the site's name, origin, header and footer links, and
the Edit and History links.

The build reads the full git history for "last updated" dates and the recent-changes page. A
shallow clone builds, with a warning.

## Markdown

CommonMark 0.31.2 plus GFM tables, strikethrough, task lists, footnotes and extended autolinks.
The parser, `rubrica-markdown`, derives its block and inline parsers from pulldown-cmark 0.13.6
by Raph Levien and contributors (MIT; see [NOTICE](NOTICE)). `render = "wikijs-compat"` selects
the Wiki.js compatibility profile. [docs/spec/](docs/spec/) specifies what Rubrica renders and
where it differs from other implementations.

## Security

Report vulnerabilities as [SECURITY.md](SECURITY.md) describes.

- Every input shape that could make the build superlinear has a bound and a finding. The
  `linear` tests check that time stays linear.
- No page carries JavaScript. The one script is the generated loader for Rubrica's WebAssembly
  search, on the search page only, checked by hash.
- Rendered HTML passes through a sanitizer.

See also [PRIVACY.md](PRIVACY.md).

## Minimum supported Rust version

Rubrica builds with Rust 1.94 or newer (`rust-version` in `Cargo.toml`). CI tests the MSRV on
every change. Development uses the toolchain pinned in `rust-toolchain.toml`.

## Developing

`scripts/ci.sh` is the gate. CI runs it, and it ends with `ci: all green`. Beyond the build and
tests, it runs two repository guards: `scripts/crate-paths.sh` (what each crate may read) and
`tools/public-workflows-check.sh` (the shape of the workflows). CI measures coverage with
`cargo-llvm-cov` and publishes the report to GitHub Pages; no third-party service is involved.
Every commit needs a DCO `Signed-off-by`. Rubrica is developed with coding agents under review;
[CONTRIBUTING.md](CONTRIBUTING.md) explains the process.

## The name

*Rubrica* is Latin for red ochre, the pigment of red brick and of the red headings (rubrics)
that scribes painted into manuscripts. The mark is a scribe's capital beside a brick wall: a
wiki is built the same way, one page at a time.

## Licence

AGPL-3.0-only; see [LICENSE](LICENSE). Third-party notices are in [NOTICE](NOTICE). The logo is
part of the repository under the same licence; [assets/](assets/) holds every variant and the
script that builds them.
