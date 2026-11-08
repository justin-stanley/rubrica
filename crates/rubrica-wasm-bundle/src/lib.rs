//! The prebuilt WebAssembly search for [Rubrica](https://github.com/justin-stanley/rubrica)
//! sites, as data: the `.wasm` built from `rubrica-wasm` and the loader the `wasm-bindgen`
//! CLI wrote for it, both made by `scripts/build-wasm.sh --bundle` (never committed; CI
//! builds them, checks that a second build gives the same bytes, and attests them).
//!
//! This crate has no code and no build script. It exists so that
//! [`rubrica-site`](https://docs.rs/rubrica-site) can embed the search page's script without
//! needing the `wasm32-unknown-unknown` target and the `wasm-bindgen` CLI at build time.
//! You rarely depend on it directly; `rubrica-site` pulls it in through its `search-wasm`
//! feature (on by default).
//!
//! # Example
//!
//! ```rust
//! // The module starts with the WebAssembly magic number.
//! assert_eq!(&rubrica_wasm_bundle::WASM[..4], b"\0asm");
//! // The loader is the JavaScript `wasm-bindgen` generated for it.
//! assert!(rubrica_wasm_bundle::LOADER.contains("WebAssembly"));
//! // Provenance: which tool and which commit made them.
//! println!(
//!     "wasm-bindgen {}, commit {}",
//!     rubrica_wasm_bundle::WASM_BINDGEN_VERSION,
//!     rubrica_wasm_bundle::SOURCE_COMMIT
//! );
//! ```
//!
//! # The other crates
//!
//! - [`rubrica-site`](https://docs.rs/rubrica-site) embeds this bundle in the site it builds.
//! - [`rubrica-render`](https://docs.rs/rubrica-render) and
//!   [`rubrica-markdown`](https://docs.rs/rubrica-markdown) render and parse the pages.
//! - [`rubrica`](https://docs.rs/rubrica) is the command-line tool.
//!
//! # Minimum supported Rust version
//!
//! Rust 1.94. A raise is a minor-version change.
//!
//! # Licence
//!
//! AGPL-3.0-only.

#![warn(missing_docs)]

/// The search module (`rubrica-search_bg.wasm`).
pub const WASM: &[u8] = include_bytes!("rubrica-search_bg.wasm");

/// The `wasm-bindgen` loader for [`WASM`] (`rubrica-search.js`), byte for byte as the CLI
/// wrote it.
pub const LOADER: &str = include_str!("rubrica-search.js");

include!("meta.rs");
