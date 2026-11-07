# rubrica-wasm

The search on `/_search`, in Rust compiled to WebAssembly. It reads `?q=` from the page's
address, fetches `/_search.json` from the same origin, scores every page and writes the
hits into the page as elements whose text is set with `textContent` and whose links are set
with `setAttribute("href", …)`; it never writes markup. The `wasm-bindgen` loader it ships
with is the one script a Rubrica site runs.

Never published: a `cdylib` for the browser, and an `rlib` so its native tests link.

## The rules

- The query is trimmed and lowercased, and split on whitespace into words.
- A page must hold every word somewhere. A word scores 10 in the title, 4 in the first
  heading that holds it (once per word) and 1 in the body.
- The first heading that matched any word is the hit's anchor: the link reads
  `Title › Heading` and points at `url#id`, the id encoded as `encodeURIComponent` encodes
  it. Without one, the link reads `Title` and points at `url`.
- Only entries whose `url` is root-relative (starts with `/`, not `//`) are considered.
- Hits sort by score, highest first, then by title; the first 50 are written; the status
  line counts every match (`No page matches.`, or `N page(s) match.`).
- The snippet is up to 60 characters before the first word's first match in the body and
  100 from it, every whitespace run one space, with `…` before it when cut and always after.

## Where it differs from the JavaScript it replaces

- Ties sort by title in **byte order**, not by `localeCompare`: `Gamma` comes before
  `epsilon`.
- The snippet is cut in **characters** (Unicode scalar values), not UTF-16 code units, so a
  cut never splits a character.
- Whitespace is JavaScript's `\s` set exactly (U+FEFF included, U+0085 not), not
  `char::is_whitespace`.

## Building

`scripts/build-wasm.sh` builds it with the `wasm` profile (`cargo build --profile wasm`,
never `--release`) for `wasm32-unknown-unknown`, writes the loader with the `wasm-bindgen`
CLI into `target/wasm-bindgen/` (`rubrica-search_bg.wasm`, `rubrica-search.js`; with
`--bundle`, into `crates/rubrica-wasm-bundle/src/` too), checks the CLI is
the version `Cargo.lock` pins for the `wasm-bindgen` crate, and fails when the `.wasm` is
over 1.5 MiB or the loader over 32 KiB. It needs the target on the pinned toolchain
(`rustup target add wasm32-unknown-unknown`) and the CLI
(`cargo install wasm-bindgen-cli@<the locked version> --locked`).
