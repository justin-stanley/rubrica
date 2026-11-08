//! `events`: the text is the parser's text (the scanner only wraps spans of it), every
//! autolink is well formed, and the scan of every text run stays within its step bound.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rubrica_markdown::textmerge::{Merged, TextMerge};
#[path = "../common.rs"]
mod common;

use rubrica_markdown::{events, parse_with_offsets, Event, LinkType, Tag, GFM};

fn upstream_text(s: &str) -> String {
    use pulldown_cmark::{Event, Options, Parser};
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let mut out = String::new();
    for e in Parser::new_ext(s, options) {
        if let Event::Text(t) = e {
            out.push_str(&t);
        }
    }
    out
}

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };

    let mut ours = String::new();
    let mut in_link: Option<String> = None; // the text of the autolink being read
    for (e, _) in events(s, GFM) {
        match e {
            Event::Text(t) => {
                ours.push_str(&t);
                if let Some(text) = in_link.as_mut() {
                    text.push_str(&t);
                }
            }
            Event::Start(Tag::Link {
                link_type: LinkType::Autolink | LinkType::Email,
                dest_url,
                ..
            }) => {
                assert!(
                    !dest_url.chars().any(|c| c.is_whitespace() || c == '<'),
                    "autolink destination {dest_url:?}"
                );
                in_link = Some(String::new());
            }
            Event::End(_) => {
                if let Some(text) = in_link.take() {
                    assert!(!text.is_empty(), "an autolink with no text");
                }
            }
            _ => {}
        }
    }
    // Upstream is compared with tables on, so the one documented table divergence (a table
    // with no leading pipe, or with its header on a lazy line, interrupting a paragraph, as
    // in cmark-gfm) is excluded, and only that: tables off on both sides would also hide
    // table-cell text differences.
    if !common::documented_table_divergence(s) {
        assert_eq!(ours, upstream_text(s), "the text differs from upstream's");
    }

    // The step bound, on the worst case (every run starts delimited). `n` is the length of
    // the run's source view, which the scan reads.
    for m in TextMerge::new(parse_with_offsets(s, GFM)) {
        if let Merged::Run(run) = m {
            let view = &s[run.range.clone()];
            let (_, steps) = rubrica_markdown::autolink::scan_counting(view, true);
            assert!(
                steps <= 7 * view.len() + 16,
                "{steps} steps for a run of {} bytes: {view:?}",
                view.len()
            );
        }
    }
});
