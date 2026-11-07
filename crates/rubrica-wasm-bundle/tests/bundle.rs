//! The bundle is what `scripts/build-wasm.sh --bundle` wrote: a wasm module, its loader,
//! and the provenance beside them.

use rubrica_wasm_bundle::{LOADER, SOURCE_COMMIT, WASM, WASM_BINDGEN_VERSION};

#[test]
fn the_bundle_crate_exposes_wasm_and_loader_with_metadata() {
    assert!(WASM.starts_with(b"\0asm"));
    assert!(
        LOADER.contains("rubrica-search_bg.wasm"),
        "the loader names no module"
    );
    // wasm-bindgen 0.2.126 writes its default export as `export { …, __wbg_init as default };`.
    assert!(
        LOADER.contains("export default") || LOADER.contains("__wbg_init as default"),
        "the loader has no default export"
    );
    let v: Vec<&str> = WASM_BINDGEN_VERSION.split('.').collect();
    assert!(
        v.len() == 3
            && v.iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())),
        "WASM_BINDGEN_VERSION is {WASM_BINDGEN_VERSION:?}"
    );
    // A loader whose header names its generator must name this version.
    let head = &LOADER[..LOADER.len().min(400)];
    if let Some(at) = head.find("wasm-bindgen ") {
        let named = head[at + "wasm-bindgen ".len()..]
            .split(|c: char| !(c.is_ascii_digit() || c == '.'))
            .next()
            .unwrap_or("");
        assert_eq!(
            named, WASM_BINDGEN_VERSION,
            "the loader's header names another version"
        );
    }
    assert!(
        SOURCE_COMMIT.len() == 40 && SOURCE_COMMIT.bytes().all(|b| b.is_ascii_hexdigit()),
        "SOURCE_COMMIT is {SOURCE_COMMIT:?}"
    );
}
