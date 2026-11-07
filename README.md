# Rubrica

Rubrica is a wiki in Rust that renders a Markdown git repository. Today it is a static build:
point it at a repository and it writes a site you can serve from any file server. A server and
in-browser editing come later (see Status). It is named for *rubrica*, red ochre: the iron
oxide scribes wrote a manuscript's headings in.

## Status

Pre-release. 0.1.0 is the first release: `rubrica check` and `rubrica build`. The milestones:

- 0.1: static, a checker and a site builder for a Markdown git repository.
- 0.2: server, `rubrica serve` renders the repository on request.
- 0.3: editing, in the browser, saved as commits.
- 0.4: permissions, who may read and who may edit.
- 0.5: search and structure.
- 0.6: collaboration.
- 0.7: import and export.
- 1.0: stable.

See `design/ROADMAP.md`.

## Install

Nothing is published yet. What each release adds (see `docs/install.md`):

- **GitHub release**, every binary attested; verify a download with
  `gh attestation verify <binary> --repo justin-stanley/rubrica`:
  - 0.1.0: a static Linux x86_64 binary.
  - 0.1.1: a static Linux aarch64 binary, and a macOS Apple Silicon binary on a best-effort basis.
- **Container image**, `ghcr.io/justin-stanley/rubrica`: x86_64 from 0.1.0; multi-arch from 0.1.1.
- **`cargo install rubrica`** (from 0.1.1, not in 0.1.0).

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
highlighted at build time; Mermaid fences are written as source. The static site is served by
any file server with `try_files {path}.html {path} {path}/index.html`.

Because every page's source is published beside it, whatever a rendered page hides (HTML
comments, every front-matter field) is readable at `/<page>.md`. Put nothing in a page that its
readers should not see.

The build is bounded. It refuses a repository over 250,000 entries, 16 MiB of paths or 128 MiB
of content, an asset over 2 MiB or more than 50 MB of assets in all, and a page over 4 MiB.
`--config` takes a `site.toml` (the site's name, origin, footer and header links, and the Edit
and History links).

A build wants the whole history, because "last updated" and the recent changes come from git. A
shallow clone builds, with a warning.

## Markdown

CommonMark 0.31.2 plus GFM tables, strikethrough, task lists, footnotes and extended autolinks.
The parser is `rubrica-markdown`, whose block and inline parsers derive from pulldown-cmark
0.13.6 by Raph Levien and contributors (MIT; see `NOTICE`). A compatibility profile exists for
wikis imported from Wiki.js (`render = "wikijs-compat"`). The specification of what Rubrica
renders is in `docs/spec/`.

## Security

Report vulnerabilities as `SECURITY.md` describes. Every shape of input that could make the
build superlinear has a bound and a finding, and `tests/linear.rs` checks that time stays
linear. No page carries JavaScript (0.1.0), and from 0.1.2 the only script is the loader for
Rubrica's own WebAssembly. Rendered HTML passes through a sanitizer. See also `PRIVACY.md`.

## Developing

The gate is `scripts/ci.sh`; it is what CI runs, and it ends with `ci: all green`.
`rust-toolchain.toml` pins the toolchain. Every commit needs a DCO `Signed-off-by`. Rubrica is
developed with coding agents under review; `CONTRIBUTING.md` says how that works.

## Licence

AGPL-3.0-only; see `LICENSE`. Third-party notices are in `NOTICE`.
