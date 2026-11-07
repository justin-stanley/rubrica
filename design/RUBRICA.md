# Rubrica

## What Rubrica is

Rubrica is full wiki software, written entirely in Rust. A wiki is a git repository of
Markdown pages; Rubrica checks it, builds it into a static site, and, from 0.2, serves it.

- It is complete wiki software, not a renderer for another wiki's syntax. Editing, permissions,
  search and collaboration are on the roadmap (`ROADMAP.md`).
- It ships no JavaScript of its own. The one exception, from 0.1.2, is the loader that
  `wasm-bindgen` generates for Rubrica's own WebAssembly search.
- Diagrams are rendered in Rust, to SVG, at build time.
- It owns its Markdown parser. The parser is Rubrica's code, vendored from a permissively
  licensed core and attributed in `NOTICE`.
- It is licensed AGPL-3.0-only. Contributions are accepted under the Developer Certificate of
  Origin.

## Architecture

| crate | role |
|---|---|
| `rubrica-markdown` | The Markdown parser. |
| `rubrica-render` | The writer that turns parse events into HTML, the sanitizer, front matter, slugs and `@sec` markers. |
| `rubrica-site` | The corpus model (pages, URLs), navigation, tables of contents, the link graph, the search index, templates, and the static build. |
| `rubrica` | The command-line tool: `check` and `build`; `serve` later. |
| `rubrica-wasm`, `rubrica-diagram` | Later: search in the browser, and diagrams. |

### Data flow of `rubrica build`

1. **Git tree.** The repository's tree at a commit is read through gix, as a snapshot with
   file modes. The working directory is never read.
2. **Sources.** Each file becomes a source: a page (`.md`) or an asset.
3. **Render.** Each page's front matter is parsed, its Markdown is parsed and written as HTML,
   and the HTML is sanitized.
4. **Site.** The pages form the corpus: URLs, navigation, tables of contents, backlinks, the
   findings of `check`. A repository with errors is refused here, and nothing is written.
5. **Files.** The site is written into a temporary directory that is renamed onto the output
   directory, so a failed build leaves nothing half-written.

### Budgets and bounds

Every input is bounded, and a bound that is exceeded is a finding, not a crash. The bounds are
constants in the code.

| constant | value | what it bounds |
|---|---|---|
| `MAX_PAGE_BYTES` | 4 MiB | one page's source |
| `MAX_REPO_FILES` | 250,000 | files in a repository |
| `MAX_REPO_PATH_BYTES` | 16 MiB | the total length of all paths |
| `MAX_REPO_BYTES` | 128 MiB | the bytes read from a repository |
| `MAX_ASSET_BYTES` | 2 MiB | one asset |
| `MAX_OUTPUT_BYTES` | 2 GiB | everything one build writes |
| `MAX_FOOTNOTE_BACKREFS` | 100 | back-reference links of one footnote |
| `MAX_RAW_ATTRIBUTES` | 256 | attributes the sanitizer accepts on one raw HTML element |
| `MAX_PAGE_HIGHLIGHT_BYTES` | 16 MiB | the highlighted HTML of one page; past it the remaining blocks are written plain (finding: yes, `code-block-not-highlighted`) |
| `MAX_ATTRIBUTE_COST` | 2^22 | the sanitizer's total attribute work on one page |
| `MAX_HIGHLIGHT_LINE_BYTES` | 2,000 | one line of a fenced block that is highlighted; over it the block is written plain (finding: yes, `code-block-not-highlighted`) |
| `MAX_HIGHLIGHT_BLOCK_BYTES` | 64 KiB | one fenced block that is highlighted; over it the block is written plain (finding: yes, `code-block-not-highlighted`) |
| front matter | 64 KiB, depth 32 | one page's YAML block |
| `MAX_OBJECT_BYTES` | 4 GiB | one git object when unpacked |
| `MAX_ALLOC_BYTES` | 64 MiB | one allocation while reading git objects |
| `MAX_DELTA_DEPTH` | 4095 | a git delta chain |

## Markdown

Pages are CommonMark with the GitHub Flavored Markdown extensions. The native rules are the
default. A compatibility profile reproduces the output of Wiki.js for wikis that migrated from
it; it differs in line breaks, strikethrough, task items, heading ids and a few details. The
table of differences is in `docs/spec/compat-profile.md`.

## Serving model (0.2 and later)

- The source of truth is a **bare git repository**. `rubrica serve` renders a snapshot per
  commit.
- Serve publishes the repository over a **dumb-HTTP, read-only** git endpoint (`HEAD`,
  `info/refs`, `objects/info/packs`, `objects/**`) as path-confined static files.
- At 0.2, serve is a read-only **mirror of a forge**: it fetches an upstream over HTTPS, keeps
  it under `refs/remotes/<name>/*`, and fast-forwards `refs/heads/*` from it.
- At 0.3, serve writes and becomes the source of truth. The forge mirrors serve's repository.
- Later, an owned push lets operators keep the forge primary. It uses the same bare
  repository, so no migration is needed; only who is upstream changes.

## Editing (0.3 and later)

Sign-in is by atproto identity, through the `calamus` OAuth library. Saving a page is a commit
written through gix into serve's repository, never through a forge API. The commit's author is
the signed-in identity's DID; the committer is Rubrica. Every write re-checks authorisation.

## Permissions (0.4)

Who may edit or read is recorded in the wiki owner's own atproto repository, as a signed
roster record: the wiki's editors, groups, and per-path rules. Rubrica verifies the record's
signature and freshness against the configured DID and fails closed if it cannot fetch it. A
read-private wiki exposes nothing to unauthorised readers, including through search, feeds,
raw Markdown, history and the git endpoint.

## Non-goals

- Running arbitrary code from a page: no scripts, no plugins that execute in the page.
- A visual or rich-text editor that stores anything but Markdown.
- Windows and Intel Macs as supported platforms.
- Telemetry, update checks, or remote fonts and scripts.
- Being a general-purpose content management system.
