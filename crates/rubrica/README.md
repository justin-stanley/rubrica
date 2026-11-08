# rubrica

A wiki in a git repository, written in Rust. Markdown pages in, a static site out.

Point `rubrica` at a checkout of Markdown pages and it checks every link and anchor, then builds
a site any file server can host, with search, recent changes from git, and no script of its own
beyond the WebAssembly that runs the search. A server and editing in the browser come later.

```sh
cargo install rubrica --locked
rubrica check --strict path/to/wiki
rubrica build path/to/wiki site
```

`rubrica check` prints nothing to fix on a clean repository and exits 0. `rubrica build` reads
the repository's git tree at a commit (`HEAD` by default, or `--rev`), never uncommitted files,
and writes the site into a directory that does not exist yet. Serve it with any file server that
tries `{path}.html`, then `{path}`, then `{path}/index.html`.

Release binaries, the container image and how to verify a download are described in the
project's [install guide](https://github.com/justin-stanley/rubrica/blob/main/docs/install.md).

## The libraries

This crate is the command-line tool. The libraries behind it can be used on their own:

- [`rubrica-markdown`](https://crates.io/crates/rubrica-markdown), the CommonMark and GFM parser;
- [`rubrica-render`](https://crates.io/crates/rubrica-render), Markdown to sanitized HTML;
- [`rubrica-site`](https://crates.io/crates/rubrica-site), the corpus and the static build;
- [`rubrica-wasm-bundle`](https://crates.io/crates/rubrica-wasm-bundle), the prebuilt search.

## Links

- Repository and documentation: <https://github.com/justin-stanley/rubrica>
- The crate's page on docs.rs: <https://docs.rs/rubrica>

## Minimum supported Rust version

Rust 1.94.

## Licence

AGPL-3.0-only.
