# rubrica-site

The corpus of a [Rubrica](https://github.com/justin-stanley/rubrica) wiki: the page set, URLs,
navigation, table of contents, link graph and search index, and the templates that write the
static site. It is built on `rubrica-render`.

The search page's script is the WebAssembly bundle in `rubrica-wasm-bundle`, embedded by the
`search-wasm` feature (on by default). Build with `--no-default-features` to leave it out.

This is a library crate of the Rubrica workspace. Most readers want the `rubrica` program:

```sh
cargo install rubrica --locked
```
