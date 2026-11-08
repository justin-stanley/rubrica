# Changelog

<!--
  Release headings have exactly this form, and the release workflow reads them:

    ## [X.Y.Z] - YYYY-MM-DD

  A release candidate has no section; only a final version does.
-->

All notable changes to Rubrica are recorded here, newest first, in the
[Keep a Changelog](https://keepachangelog.com) format. Work that has landed on `main` but is not
yet released appears under **Unreleased**.

Each entry says what changed, the mechanism, and, where a defect is involved, how it was
established rather than assumed.

Rubrica follows [Semantic Versioning](https://semver.org). Before 1.0, a breaking change bumps
the minor version. Versions are git tags (`vX.Y.Z`); a release candidate (`vX.Y.Z-rc.N`) is
tagged first and is never published (see [CONTRIBUTING.md](CONTRIBUTING.md)). Fixed
vulnerabilities are named here too (see [SECURITY.md](SECURITY.md)).

---

## Unreleased

### Added

- `rubrica check`, which checks a Markdown git repository for dead internal links and anchors,
  relative links to pages, front matter, section ids and paths, with text and JSON reports and
  exit codes 0, 1 and 2.
- `rubrica build`, which builds a repository's git tree at a commit, or one directory of it,
  into a static site: a page and its source per document, search, recent changes, a 404 page, a
  health file and the site's assets, written into a temporary directory and renamed into place.
- `rubrica-markdown`, Rubrica's own Markdown parser: CommonMark 0.31.2 with GFM tables,
  strikethrough, task lists, footnotes and extended autolinks, derived from pulldown-cmark
  0.13.6.
- A compatibility profile for wikis imported from Wiki.js (`render = "wikijs-compat"`).
- Bounds on every superlinear shape of input, each with a finding, and a test that time stays
  linear: 250,000 entries, 16 MiB of paths and 128 MiB of content per repository, 2 MiB per
  asset and 50 MB of assets, 4 MiB per page.
- Search in the browser without a script of Rubrica's own: `rubrica-wasm` searches
  `_search.json` on the search page, loaded by the generated loader whose hash the build checks,
  under a Content-Security-Policy that allows WebAssembly on that page only. Every other page
  keeps `script-src 'none'`.
- Build-time syntax highlighting.
- Releases for Linux x86_64 and aarch64 (static), macOS on Apple Silicon (best effort) and a
  multi-architecture container image, each attested, from signed tags; the crates on crates.io.
- Coverage reports in CI: a readable summary per crate and file, a lines-coverage floor for each
  crate in `coverage-floor.toml` (the job fails more than 0.5 points below it, or with no floor),
  and the HTML report with a badge file published to GitHub Pages on a push to `main`. The site
  carries no script. `scripts/test-coverage-summary.sh` tests the summary, the floors and the
  site against fixture and planted reports.
