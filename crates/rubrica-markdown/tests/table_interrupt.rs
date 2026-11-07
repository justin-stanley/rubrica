//! A table whose header row has no leading pipe interrupts a paragraph, as in cmark-gfm and
//! markdown-it: when a delimiter row follows a paragraph line, that line is the header
//! candidate, and a table opens only if its cell count equals the delimiter row's. The
//! lines above it stay a paragraph. Upstream pulldown-cmark opens such a table only when
//! the header row starts with a pipe.

use std::time::{Duration, Instant};

use rubrica_markdown::{parse, Event, Tag, TagEnd, GFM};

/// A compact rendering of the block structure: `<p>`, `<table>`, `<head>`, `<row>`,
/// `<cell>`, `<bq>`, `<li>`, text, `/` for a soft break and `<a url>`.
fn shape(src: &str) -> String {
    let mut out = String::new();
    for e in parse(src, GFM) {
        match e {
            Event::Start(t) => match t {
                Tag::Paragraph => out.push_str("<p>"),
                Tag::Table(_) => out.push_str("<table>"),
                Tag::TableHead => out.push_str("<head>"),
                Tag::TableRow => out.push_str("<row>"),
                Tag::TableCell => out.push_str("<cell>"),
                Tag::BlockQuote(_) => out.push_str("<bq>"),
                Tag::List(_) => out.push_str("<list>"),
                Tag::Item => out.push_str("<li>"),
                Tag::Link { dest_url, .. } => out.push_str(&format!("<a {dest_url}>")),
                other => out.push_str(&format!("<{other:?}>")),
            },
            Event::End(t) => match t {
                TagEnd::Paragraph => out.push_str("</p>"),
                TagEnd::Table => out.push_str("</table>"),
                TagEnd::TableHead => out.push_str("</head>"),
                TagEnd::TableRow => out.push_str("</row>"),
                TagEnd::TableCell => out.push_str("</cell>"),
                TagEnd::BlockQuote(_) => out.push_str("</bq>"),
                TagEnd::List(_) => out.push_str("</list>"),
                TagEnd::Item => out.push_str("</li>"),
                TagEnd::Link => out.push_str("</a>"),
                other => out.push_str(&format!("</{other:?}>")),
            },
            Event::Text(t) => out.push_str(&t),
            Event::SoftBreak => out.push('/'),
            other => out.push_str(&format!("[{other:?}]")),
        }
    }
    out
}

#[test]
fn a_header_without_a_leading_pipe_opens_a_table_after_a_paragraph() {
    assert_eq!(
        shape("x\na | b\n-|-\n"),
        "<p>x</p><table><head><cell>a</cell><cell>b</cell></head></table>"
    );
    // The same with a body row, and with the header on the third line.
    assert_eq!(
        shape("x\ny\na | b\n-|-\nc | d\n"),
        "<p>x/y</p><table><head><cell>a</cell><cell>b</cell></head>\
         <row><cell>c</cell><cell>d</cell></row></table>"
    );
}

#[test]
fn a_header_whose_cell_count_differs_from_the_delimiter_row_stays_in_the_paragraph() {
    assert_eq!(shape("x\na | b\n-|-|-\n"), "<p>x/a | b/-|-|-</p>");
    assert_eq!(shape("x\na | b | c\n-|-\n"), "<p>x/a | b | c/-|-</p>");
}

#[test]
fn a_header_with_a_leading_pipe_is_unchanged() {
    assert_eq!(
        shape("x\n| a | b |\n|-|-|\n"),
        "<p>x</p><table><head><cell>a</cell><cell>b</cell></head></table>"
    );
    assert_eq!(
        shape("| a | b |\n|-|-|\n"),
        "<table><head><cell>a</cell><cell>b</cell></head></table>"
    );
}

#[test]
fn a_line_without_a_pipe_is_a_one_cell_header_for_a_one_cell_delimiter_row() {
    // A delimiter row needs a pipe, so a one-column table needs `|-` or `-|`.
    assert_eq!(
        shape("x\ny\n|-\n"),
        "<p>x</p><table><head><cell>y</cell></head></table>"
    );
    // Without a pipe it is a setext heading or a thematic break, as before.
    assert_eq!(
        shape("x\n-\n"),
        "<Heading { level: H2, id: None, classes: [], attrs: [] }>x</Heading(H2)>"
    );
}

#[test]
fn definitions_then_an_underline_then_a_pipe_row_open_a_table() {
    let head = "<head><cell><a /u>a</a></cell><cell>b</cell></head>";
    assert_eq!(
        shape("[r]: /d\n=\n[a](/u) | b\n-|-\n"),
        format!("<p>=</p><table>{head}</table>")
    );
    assert_eq!(
        shape("> [r]: /d\n> =\n> [a](/u) | b\n> -|-\n"),
        format!("<bq><p>=</p><table>{head}</table></bq>")
    );
    assert_eq!(
        shape("[r]: /d\n=\n[l](/v\n\"t\") [m](/w)\n[a](/u) | b\n-|-\n[c](/x) | d\n"),
        format!(
            "<p>=/<a /v>l</a> <a /w>m</a></p><table>{head}\
             <row><cell><a /x>c</a></cell><cell>d</cell></row></table>"
        )
    );
    // The header on a lazy line, in nested quotes and in a list item.
    assert_eq!(
        shape("> > [r]: /d\n> > =\n>\t[a](/u) | b\n> > -|-\n"),
        format!("<bq><bq><p>=</p><table>{head}</table></bq></bq>")
    );
    assert_eq!(
        shape("> 1. [y]: /d\n>    =\n>\t[a](/u) | b\n>    -|-\n"),
        format!("<bq><list><li>=<table>{head}</table></li></list></bq>")
    );
}

#[test]
fn an_underline_is_the_header_when_a_one_cell_delimiter_row_follows() {
    assert_eq!(
        shape("[q]: /w\n=\n|-\n"),
        "<table><head><cell>=</cell></head></table>"
    );
    assert_eq!(
        shape("> [q]: /w\n> =\n> |-\n> [c](/u) | d\n"),
        "<bq><table><head><cell>=</cell></head><row><cell><a /u>c</a></cell></row></table></bq>"
    );
}

/// Many paragraph lines, each followed by a pipe-less header candidate with a delimiter
/// row of the wrong width: nothing is rescanned, so time grows linearly.
#[test]
fn mismatched_header_candidates_are_linear() {
    let doc = |size: usize| "a | b\n-|-|-\n".repeat(size / 12);
    let time = |size: usize| {
        let src = doc(size);
        (0..3)
            .map(|_| {
                let t = Instant::now();
                let n = parse(&src, GFM).count();
                assert!(n > 0);
                t.elapsed()
            })
            .min()
            .unwrap()
    };
    let (a, b, c) = (time(1 << 19), time(1 << 20), time(1 << 21));
    if cfg!(not(debug_assertions)) {
        for (small, big) in [(a, b), (b, c)] {
            if small >= Duration::from_millis(50) {
                assert!(
                    big.as_secs_f64() <= 2.5 * small.as_secs_f64(),
                    "{a:?} {b:?} {c:?}"
                );
            }
        }
        assert!(c < Duration::from_secs(2), "{c:?}");
    }
}

/// Many paragraph lines, each a header candidate that begins a link reference definition
/// whose title never closes: each check of a candidate scans only to the next one, so time
/// grows linearly.
#[test]
fn header_candidates_that_start_definitions_are_linear() {
    let doc = |size: usize| format!("x\n{}", "[a]: /u 'x\n-|\n".repeat(size / 13));
    let time = |size: usize| {
        let src = doc(size);
        (0..3)
            .map(|_| {
                let t = Instant::now();
                let n = parse(&src, GFM).count();
                assert!(n > 0);
                t.elapsed()
            })
            .min()
            .unwrap()
    };
    let (a, b, c) = (time(1 << 19), time(1 << 20), time(1 << 21));
    if cfg!(not(debug_assertions)) {
        for (small, big) in [(a, b), (b, c)] {
            if small >= Duration::from_millis(50) {
                assert!(
                    big.as_secs_f64() <= 2.5 * small.as_secs_f64(),
                    "{a:?} {b:?} {c:?}"
                );
            }
        }
        assert!(c < Duration::from_secs(2), "{c:?}");
    }
}

/// Every event with its byte range, one per line, as `range event`.
fn ranges(src: &str) -> Vec<String> {
    rubrica_markdown::parse_with_offsets(src, GFM)
        .map(|(e, r)| format!("{r:?} {e:?}"))
        .collect()
}

/// The same for upstream pulldown-cmark 0.13.6 on the same flags.
fn upstream_ranges(src: &str) -> Vec<String> {
    use pulldown_cmark::Options as O;
    let options =
        O::ENABLE_TABLES | O::ENABLE_STRIKETHROUGH | O::ENABLE_TASKLISTS | O::ENABLE_FOOTNOTES;
    pulldown_cmark::Parser::new_ext(src, options)
        .into_offset_iter()
        .map(|(e, r)| format!("{r:?} {e:?}"))
        .collect()
}

/// The header candidate `3.` would be an ordered list starting at 3 at the start of a
/// block, which may not interrupt a paragraph, so no table opens: the paragraph continues
/// through every line, as upstream parses it (fuzz `autolinks`, minimized).
#[test]
fn a_header_candidate_that_starts_a_list_does_not_cut_the_paragraph() {
    let src = ".\n3.\n-|";
    let want = [
        "0..7 Start(Paragraph)",
        "0..1 Text(Borrowed(\".\"))",
        "1..2 SoftBreak",
        "2..4 Text(Borrowed(\"3.\"))",
        "4..5 SoftBreak",
        "5..7 Text(Borrowed(\"-|\"))",
        "0..7 End(Paragraph)",
    ];
    assert_eq!(ranges(src), want);
    assert_eq!(ranges(src), upstream_ranges(src));
}

/// The header candidate `[` begins a link reference definition (`[\n-|\n]:a`) at the
/// start of a block, which may not interrupt a paragraph, so no table opens and the
/// lines stay paragraph text, as upstream parses them (fuzz `differential`, minimized).
#[test]
fn a_header_candidate_that_starts_a_definition_does_not_cut_the_paragraph() {
    let src = "f\n[\n-|\n]:a";
    let want = [
        "0..10 Start(Paragraph)",
        "0..1 Text(Borrowed(\"f\"))",
        "1..2 SoftBreak",
        "2..3 Text(Borrowed(\"[\"))",
        "3..4 SoftBreak",
        "4..6 Text(Borrowed(\"-|\"))",
        "6..7 SoftBreak",
        "7..8 Text(Borrowed(\"]\"))",
        "8..10 Text(Borrowed(\":a\"))",
        "0..10 End(Paragraph)",
    ];
    assert_eq!(ranges(src), want);
    assert_eq!(ranges(src), upstream_ranges(src));
}

/// The header candidate `\` ends in a backslash, a hard break, which comes before any
/// table on that line, so no table opens: the paragraph continues, as upstream parses it
/// (fuzz `differential`, found after the fixes above, minimized).
#[test]
fn a_header_candidate_ending_in_a_hard_break_does_not_cut_the_paragraph() {
    let src = "|\n\\\n-|";
    let want = [
        "0..6 Start(Paragraph)",
        "0..1 Text(Borrowed(\"|\"))",
        "1..2 SoftBreak",
        "2..4 HardBreak",
        "4..6 Text(Borrowed(\"-|\"))",
        "0..6 End(Paragraph)",
    ];
    assert_eq!(ranges(src), want);
    assert_eq!(ranges(src), upstream_ranges(src));
}
