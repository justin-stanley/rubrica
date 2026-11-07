# rubrica

A wiki for Markdown repositories, written in Rust, with no JavaScript of its own: it checks
a Markdown git repository and builds it into a static site.

```sh
cargo install rubrica --locked
rubrica check --strict path/to/wiki
rubrica build path/to/wiki site
```

`rubrica check` prints nothing to fix on a clean repository and exits 0. Release binaries,
the container image and how to verify a download are described in the project's
[install guide](https://github.com/justin-stanley/rubrica/blob/main/docs/install.md).

Project home: <https://github.com/justin-stanley/rubrica>. Licence: AGPL-3.0-only.
