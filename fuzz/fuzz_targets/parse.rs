//! `parse` never panics, with the GFM options and with none.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rubrica_markdown::{parse, Options, GFM};

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    for e in parse(s, GFM) {
        let _ = e;
    }
    for e in parse(s, Options::empty()) {
        let _ = e;
    }
});
