# rubrica-markdown

Rubrica's Markdown parser: a pull parser for CommonMark 0.31.2 that yields events, each with its byte range in the source.
The block and inline parsers derive from pulldown-cmark 0.13.6 by Raph Levien and contributors, MIT; the vendored files keep their upstream headers, `UPSTREAM` records the version, the files and every local edit, and the repository's `NOTICE` carries the licence text.
`GFM` is Rubrica's option set: tables, strikethrough, task lists and footnotes (upstream's `ENABLE_GFM` blockquote alerts are not in it; extended autolinks are Rubrica's own pass over the events, not an option).
`parse(src, options)` is the event stream of `src`.
`parse_with_offsets(src, options)` is the same stream with each event's byte range in `src`.
`events(src, options)` is the stream Rubrica renders: `parse_with_offsets` with consecutive text events merged into runs (`textmerge`) and GFM extended autolinks (`www.`, `http://`, `https://`, `ftp://`, email) found in them (`autolink`), each as `Start(Link)`/`Text`/`End(Link)` with its source range; the scan reads each run's source bytes once, forward, every `LinkType::Email` link out of `events` carries `mailto:`, and `docs/spec/autolinks.md` states the rule, Rubrica's nested-candidate decisions, the differences from cmark-gfm and the step bound `7·n + 16`.
`autolink::scan_counting`, `Found` and `Kind` are `pub` but `#[doc(hidden)]`: they exist for the linearity tests (`tests/autolink.rs`, the private suite's and the fuzz crate's) and nothing else may call them; `events` is the public surface.
`tests/autolink.rs` pins the 11 GFM autolink examples, the nested-candidate table, what is never scanned, the delimiters, exact ranges (proptest, a dev-dependency only) and the scan's linearity; `tests/linear.rs` times the parser with the scanner on 37 pathological shapes.
HTML writing lives in `rubrica-render`; this crate produces events only, and has no features.
`tests/spec.rs` proves, for every CommonMark and GFM spec example under `tests/spec/`, that the events and offsets equal upstream pulldown-cmark 0.13.6's (a dev-dependency only; it never ships).
Re-sync rule: upstream changes are ported by hand after reading the diff (`tools/upstream-diff.sh` shows the local edits); never merged blind.
Licence: AGPL-3.0-only AND MIT (see `Cargo.toml`, `UPSTREAM` and `NOTICE`).
