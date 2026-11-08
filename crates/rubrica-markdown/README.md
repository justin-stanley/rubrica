# rubrica-markdown

A CommonMark 0.31.2 and GitHub Flavored Markdown pull parser that yields events with their
byte ranges in the source. It is the parser of [Rubrica](https://github.com/justin-stanley/rubrica),
a wiki engine written in Rust that builds a git repository of Markdown pages into a site.

```rust
use rubrica_markdown::{parse_with_offsets, Event, Tag, GFM};

let src = "a ~~b~~ *c*";
for (event, range) in parse_with_offsets(src, GFM) {
    if let Event::Start(Tag::Strikethrough) = event {
        println!("strikethrough starts at byte {}", range.start);
    }
}
```

- `parse` is the event stream, `parse_with_offsets` adds each event's byte range, and `events`
  is the stream Rubrica renders: text runs merged and GFM extended autolinks found.
- `GFM` is the option set Rubrica uses: tables, strikethrough, task lists and footnotes.
- This crate produces events only. For sanitized HTML, use
  [`rubrica-render`](https://crates.io/crates/rubrica-render).

## Links

- Documentation: <https://docs.rs/rubrica-markdown>
- Repository: <https://github.com/justin-stanley/rubrica>
- The other crates: [`rubrica-render`](https://crates.io/crates/rubrica-render),
  [`rubrica-site`](https://crates.io/crates/rubrica-site),
  [`rubrica-wasm-bundle`](https://crates.io/crates/rubrica-wasm-bundle) and the command-line
  tool [`rubrica`](https://crates.io/crates/rubrica).

## Attribution

The block and inline parsers derive from [pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark)
0.13.6 by Raph Levien and contributors, MIT. The vendored files keep their upstream headers;
`UPSTREAM` in this crate records the version, the files and every local edit, and the
repository's `NOTICE` carries the licence text.

## Minimum supported Rust version

Rust 1.94.

## Licence

AGPL-3.0-only AND MIT: the vendored pulldown-cmark files are MIT, the rest is AGPL-3.0-only.
