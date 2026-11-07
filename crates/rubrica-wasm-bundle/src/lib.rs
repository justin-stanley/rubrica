//! The search page's wasm bundle, as data: the `.wasm` built from `rubrica-wasm` and the
//! loader the `wasm-bindgen` CLI wrote for it, both made by `scripts/build-wasm.sh --bundle`
//! (never committed; CI builds them, checks a second build gives the same bytes, and
//! attests them).

/// The search module (`rubrica-search_bg.wasm`).
pub const WASM: &[u8] = include_bytes!("rubrica-search_bg.wasm");

/// The `wasm-bindgen` loader for [`WASM`] (`rubrica-search.js`), byte for byte as the CLI
/// wrote it.
pub const LOADER: &str = include_str!("rubrica-search.js");

include!("meta.rs");
