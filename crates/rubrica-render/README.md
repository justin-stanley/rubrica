# rubrica-render

Markdown in, sanitized HTML out. This is the renderer of
[Rubrica](https://github.com/justin-stanley/rubrica), a wiki engine written in Rust that builds
a git repository of Markdown pages into a site. It is built on
[`rubrica-markdown`](https://crates.io/crates/rubrica-markdown).

```rust
use rubrica_render::{render, RenderOptions};

let rendered = render("# Hello\n\nSome *Markdown*.\n", &RenderOptions::native());
println!("{}", rendered.html);
// <h1 id="hello">Hello</h1>
// <p>Some <em>Markdown</em>.</p>
```

`render` gives the HTML and what a wiki needs from the same parse: the headings and their ids,
the links, task items, the plain text for search, and findings for what is wrong with the page.
`render_page` does the same for a page file with YAML front matter. There is no I/O.

- **Profiles.** `RenderOptions::native()` is Rubrica's own rules. `RenderOptions::wikijs_compat()`
  reproduces the output of Wiki.js 2 (line breaks, heading ids, task items) for wikis that
  moved from it.
- **Sanitized.** Raw HTML is filtered to a short list of tags with no attributes, or escaped.

Run the example with `cargo run -p rubrica-render --example render` in a checkout of the
repository.

## Links

- Documentation: <https://docs.rs/rubrica-render>
- Repository: <https://github.com/justin-stanley/rubrica>
- The other crates: [`rubrica-markdown`](https://crates.io/crates/rubrica-markdown),
  [`rubrica-site`](https://crates.io/crates/rubrica-site),
  [`rubrica-wasm-bundle`](https://crates.io/crates/rubrica-wasm-bundle) and the command-line
  tool [`rubrica`](https://crates.io/crates/rubrica).

## Minimum supported Rust version

Rust 1.94.

## Licence

AGPL-3.0-only.
