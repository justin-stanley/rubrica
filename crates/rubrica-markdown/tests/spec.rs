//! The differential against upstream pulldown-cmark 0.13.6 (a dev-dependency only): for
//! every CommonMark 0.31.2 and GFM 0.29 spec example, the vendored parser's event stream
//! and offsets equal upstream's, both parsers on the same flags.

use std::ops::Range;

// the data shape
#[derive(serde::Deserialize)]
struct Example {
    markdown: String,
    // Unused here; the renderer's spec test reads it.
    #[allow(dead_code)]
    html: String,
    #[serde(default)]
    example: usize,
    #[serde(default)]
    extension: String,
    #[serde(default)]
    section: String,
}

fn commonmark() -> Vec<Example> {
    serde_json::from_str(include_str!("spec/commonmark-0.31.2.json")).unwrap()
}

fn gfm() -> Vec<Example> {
    serde_json::from_str(include_str!("spec/gfm-0.29.json")).unwrap()
}

/// The example's name in a failure: its number (the suite's own, else its position) and
/// its section or extension.
fn label(i: usize, ex: &Example) -> String {
    let n = if ex.example == 0 { i + 1 } else { ex.example };
    let what = if ex.section.is_empty() {
        &ex.extension
    } else {
        &ex.section
    };
    format!("example {n} ({what})")
}

fn upstream_options() -> pulldown_cmark::Options {
    use pulldown_cmark::Options;
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
}

fn ours(md: &str) -> Vec<String> {
    rubrica_markdown::parse(md, rubrica_markdown::GFM)
        .map(|e| format!("{e:?}"))
        .collect()
}

fn theirs(md: &str) -> Vec<String> {
    pulldown_cmark::Parser::new_ext(md, upstream_options())
        .map(|e| format!("{e:?}"))
        .collect()
}

fn ours_with_offsets(md: &str) -> Vec<(String, Range<usize>)> {
    rubrica_markdown::parse_with_offsets(md, rubrica_markdown::GFM)
        .map(|(e, r)| (format!("{e:?}"), r))
        .collect()
}

fn theirs_with_offsets(md: &str) -> Vec<(String, Range<usize>)> {
    pulldown_cmark::Parser::new_ext(md, upstream_options())
        .into_offset_iter()
        .map(|(e, r)| (format!("{e:?}"), r))
        .collect()
}

#[test]
fn commonmark_has_652_examples_and_gfm_672() {
    assert_eq!(commonmark().len(), 652);
    assert_eq!(gfm().len(), 672);
}

fn events_equal_upstream(suite: &[Example]) {
    for (i, ex) in suite.iter().enumerate() {
        let ours = ours(&ex.markdown);
        let theirs = theirs(&ex.markdown);
        assert!(
            ours == theirs,
            "{}: ours {ours:?} theirs {theirs:?}",
            label(i, ex)
        );
    }
}

#[test]
fn commonmark_events_equal_upstream() {
    events_equal_upstream(&commonmark());
}

#[test]
fn gfm_events_equal_upstream() {
    events_equal_upstream(&gfm());
}

#[test]
fn offsets_equal_upstream_and_are_in_bounds() {
    for suite in [commonmark(), gfm()] {
        for (i, ex) in suite.iter().enumerate() {
            let md = &ex.markdown;
            let ours = ours_with_offsets(md);
            let theirs = theirs_with_offsets(md);
            assert!(
                ours == theirs,
                "{}: ours {ours:?} theirs {theirs:?}",
                label(i, ex)
            );
            for (event, range) in &ours {
                let Range { start, end } = *range;
                assert!(
                    start <= end
                        && end <= md.len()
                        && md.is_char_boundary(start)
                        && md.is_char_boundary(end),
                    "{}: {event} has range {start}..{end} in {} bytes",
                    label(i, ex),
                    md.len()
                );
            }
        }
    }
}

#[test]
fn the_gfm_option_set_is_exactly_four_flags() {
    let ours = format!("{:?}", rubrica_markdown::GFM);
    let theirs = format!("{:?}", upstream_options());
    assert_eq!(ours, theirs);
    // bitflags' Debug prints in declaration order: TABLES 1<<1, FOOTNOTES 1<<2,
    // STRIKETHROUGH 1<<3, TASKLISTS 1<<4.
    assert_eq!(
        ours,
        "Options(ENABLE_TABLES | ENABLE_FOOTNOTES | ENABLE_STRIKETHROUGH | ENABLE_TASKLISTS)"
    );
}

#[test]
fn no_html_module_and_no_features() {
    // Never `include_str!("../Cargo.toml")`: a packaged test may not read outside its crate
    // by a `..` path.
    let manifest = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
    assert!(
        !manifest.contains("[features]"),
        "the manifest has a [features] section"
    );
    assert!(
        !manifest.contains("pulldown-cmark-escape"),
        "the manifest names pulldown-cmark-escape"
    );
}
