# rubrica-wasm-bundle

The prebuilt WebAssembly search for [Rubrica](https://github.com/justin-stanley/rubrica) sites,
as data. The crate holds the module built from `rubrica-wasm` and the loader the `wasm-bindgen`
CLI wrote for it, so that [`rubrica-site`](https://crates.io/crates/rubrica-site) can embed the
search page's script without a WebAssembly toolchain at build time. You rarely depend on it
directly: `rubrica-site` pulls it in through its `search-wasm` feature, which is on by default.

```rust
// The module, the loader, and where they came from.
let wasm: &[u8] = rubrica_wasm_bundle::WASM;
let loader: &str = rubrica_wasm_bundle::LOADER;
println!(
    "{} bytes of wasm, {} bytes of loader, wasm-bindgen {}, commit {}",
    wasm.len(),
    loader.len(),
    rubrica_wasm_bundle::WASM_BINDGEN_VERSION,
    rubrica_wasm_bundle::SOURCE_COMMIT,
);
```

The crate has no code and no build script. Its three files, `src/rubrica-search_bg.wasm`,
`src/rubrica-search.js` and `src/meta.rs`, are never committed to the repository: CI builds
them with the pinned toolchain and the pinned `wasm-bindgen` CLI (the same version as the
`wasm-bindgen` crate in `Cargo.lock`), checks that a second build from another target directory
gives the same bytes, and attests them. A published crate holds them.

## Building it from a checkout

The crate does not compile until the files exist. From the repository root:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli@0.2.126 --locked
scripts/build-wasm.sh --bundle
```

## Links

- Documentation: <https://docs.rs/rubrica-wasm-bundle>
- Repository: <https://github.com/justin-stanley/rubrica>
- The other crates: [`rubrica-markdown`](https://crates.io/crates/rubrica-markdown),
  [`rubrica-render`](https://crates.io/crates/rubrica-render),
  [`rubrica-site`](https://crates.io/crates/rubrica-site) and the command-line tool
  [`rubrica`](https://crates.io/crates/rubrica).

## Minimum supported Rust version

Rust 1.94.

## Licence

AGPL-3.0-only.
