//! Our event stream equals upstream pulldown-cmark 0.13.6's (as Debug strings), on no
//! options and on the GFM set, both sides on the same flags.
#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../common.rs"]
mod common;

fn ours(s: &str, o: rubrica_markdown::Options) -> Vec<String> {
    rubrica_markdown::parse(s, o)
        .map(|e| format!("{e:?}"))
        .collect()
}

fn theirs(s: &str, o: pulldown_cmark::Options) -> Vec<String> {
    pulldown_cmark::Parser::new_ext(s, o)
        .map(|e| format!("{e:?}"))
        .collect()
}

fn same(s: &str, what: &str, a: Vec<String>, b: Vec<String>) {
    if let Some(i) = (0..a.len().max(b.len())).find(|&i| a.get(i) != b.get(i)) {
        panic!(
            "{what}: first difference at event {i}: ours {:?}, upstream {:?}\ninput: {s:?}",
            a.get(i),
            b.get(i)
        );
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    same(
        s,
        "CommonMark",
        ours(s, rubrica_markdown::Options::empty()),
        theirs(s, pulldown_cmark::Options::empty()),
    );
    // The GFM comparison skips the one documented table divergence: a table with no leading
    // pipe, or with its header on a lazy line, interrupting a paragraph (see common.rs). The
    // comparison above has tables off.
    if common::documented_table_divergence(s) {
        return;
    }
    let upstream_gfm = pulldown_cmark::Options::ENABLE_TABLES
        | pulldown_cmark::Options::ENABLE_STRIKETHROUGH
        | pulldown_cmark::Options::ENABLE_TASKLISTS
        | pulldown_cmark::Options::ENABLE_FOOTNOTES;
    same(
        s,
        "GFM",
        ours(s, rubrica_markdown::GFM),
        theirs(s, upstream_gfm),
    );
});
