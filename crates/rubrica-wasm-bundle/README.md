# rubrica-wasm-bundle

The search page's WebAssembly bundle, as data, for `rubrica-site` to embed: the module
built from `rubrica-wasm` (`WASM`), the loader the `wasm-bindgen` CLI wrote for it
(`LOADER`), and their provenance (`WASM_BINDGEN_VERSION`, `SOURCE_COMMIT`).

The crate has no code and no build script. Its three files, `src/rubrica-search_bg.wasm`,
`src/rubrica-search.js` and `src/meta.rs`, are never committed: CI builds them with the
pinned toolchain and the pinned `wasm-bindgen` CLI (the same version as the `wasm-bindgen`
crate in `Cargo.lock`), checks that a second build from another target directory gives the
same bytes (`scripts/wasm-reproducible.sh`), and attests them.

## Building it locally

The crate does not compile until the files exist. From the repository root:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli@0.2.126 --locked
scripts/build-wasm.sh --bundle
```

`rubrica-site` uses the bundle through its `search-wasm` feature (on by default); build it
with `--no-default-features` to leave the bundle out.

`PACKAGE-FILES` lists exactly what a published crate holds.
