# rubrica-site

The corpus and static build of [Rubrica](https://github.com/justin-stanley/rubrica), a wiki
engine written in Rust that builds a git repository of Markdown pages into a site. Given a
wiki's files, this crate works out the pages and their URLs, the navigation, tables of
contents and link graph, finds dead links and anchors, builds the search index, and writes the
site through templates. It renders pages with
[`rubrica-render`](https://crates.io/crates/rubrica-render).

The crate does no I/O. The caller hands it the files and receives each output file through a
callback.

```rust
use rubrica_site::{Site, SiteConfig, SourceFile};

let files = vec![
    SourceFile::regular("home.md", "---\ntitle: Home\n---\n# Home\n\nSee [notes](/notes).\n"),
    SourceFile::regular("notes.md", "---\ntitle: Notes\n---\n# Notes\n"),
];
let site = Site::from_sources(files, &SiteConfig::default());

assert_eq!(site.pages().len(), 2);
assert!(site.findings().is_empty()); // no dead links, no front matter problems
```

`examples/build.rs` goes on to write the site into a temporary directory and list the files:
`cargo run -p rubrica-site --example build` in a checkout of the repository.

To build a site from a git repository, use the command-line tool
[`rubrica`](https://crates.io/crates/rubrica), which is this crate plus a git reader.

## Features

`search-wasm` (on by default) embeds the search page's WebAssembly from
[`rubrica-wasm-bundle`](https://crates.io/crates/rubrica-wasm-bundle). Build with
`--no-default-features` to leave it out.

## Links

- Documentation: <https://docs.rs/rubrica-site>
- Repository: <https://github.com/justin-stanley/rubrica>
- The other crates: [`rubrica-markdown`](https://crates.io/crates/rubrica-markdown),
  [`rubrica-render`](https://crates.io/crates/rubrica-render),
  [`rubrica-wasm-bundle`](https://crates.io/crates/rubrica-wasm-bundle) and the command-line
  tool [`rubrica`](https://crates.io/crates/rubrica).

## Minimum supported Rust version

Rust 1.94.

## Licence

AGPL-3.0-only.
