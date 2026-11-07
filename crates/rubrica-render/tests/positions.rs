//! The `LineTable` (a byte offset to its 1-based line and UTF-8 column), the writer's
//! positions as lines, and the position property of the new pipeline: the source bytes at a
//! reported position are the text the event names.
//!
//! Two tests are green before the table exists and guard what was built earlier:
//! `offset_of_a_merged_run_maps_to_the_same_line_as_its_piece` (`TextRun::offset_of`)
//! and `every_text_event_sits_where_it_is_written` (the parser's ranges, the characterisation
//! the parser switch is held to).

use std::borrow::Cow;

use proptest::prelude::*;
use rubrica_markdown::textmerge::{Merged, TextMerge};
use rubrica_markdown::{events, parse_with_offsets, Event, GFM};
use rubrica_render::writer::{write, WriterOptions};
use rubrica_render::{render, LineTable, RenderOptions};

/// `markdown.rs`'s private `normalize_line_ends`, copied: `\r\n` to `\n`, then a bare `\r`
/// to `\n`. The table is defined on the normalised text, as the parser reads it.
fn normalize_line_ends(body: &str) -> Cow<'_, str> {
    if !body.contains('\r') {
        return Cow::Borrowed(body);
    }
    Cow::Owned(body.replace("\r\n", "\n").replace('\r', "\n"))
}

/// The line on which byte `offset` of the normalised `text` sits, counted the slow way: the
/// `\n`s before it, plus one. Bytes, so an offset inside a multibyte character counts too.
fn line_by_counting(text: &str, offset: usize) -> usize {
    text.as_bytes()[..offset]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// The documents of the property tests, copied from `markdown.rs` (Rubrica's own): lines of
/// `pieces`, each after one of `prefixes` (containers, indentation, lazy lines, tabs a
/// container may half consume) and ended one of the three ways; a `⏎` inside a piece is a
/// line end inside it (a link's destination, title or label going on to the next line), and
/// the line it starts gets a prefix of its own.
fn documents(seed: u32, count: usize, pieces: &[&str]) -> Vec<String> {
    let prefixes = [
        "", "  ", "> ", "> > ", "- ", "  - ", "> - ", "1. ", "   ", ">  ", ">\t", "\t", "> 1. ",
    ];
    let ends = ["\n", "\r\n", "\r"];
    let mut seed = seed;
    let mut next = |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as usize % n
    };
    (0..count)
        .map(|_| {
            let mut md = String::new();
            for _ in 0..1 + next(5) {
                let first = prefixes[next(prefixes.len())];
                md.push_str(first);
                let piece = pieces[next(pieces.len())];
                for (i, part) in piece.split('⏎').enumerate() {
                    if i > 0 {
                        md.push_str(ends[next(ends.len())]);
                        if next(2) == 0 {
                            // The first line's containers, continued.
                            md.extend(first.chars().map(|c| match c {
                                '-' | '.' | '0'..='9' => ' ',
                                c => c,
                            }));
                        } else {
                            md.push_str(prefixes[next(prefixes.len())]);
                        }
                    }
                    md.push_str(part);
                }
                md.push_str(ends[next(ends.len())]);
            }
            md
        })
        .collect()
}

/// The pieces of `markdown.rs::text_after_reference_definitions_and_multi_line_links_sits_where_it_is_written`,
/// copied: single-line pieces and pieces whose syntax spans lines (links, images and
/// reference links with a destination, title or label on the next line; definitions whose
/// label or title does; footnote definitions and references; definitions then an underline
/// line then a table; a NUL; an underline line that is itself a table's header row).
const PIECES: &[&str] = &[
    "[y]: /w",
    "[z]: /v \"t\"",
    "[b c]: /u",
    "a ~b~ c",
    "[l](/u) d",
    "`c` e",
    "**s** f",
    "g  ",
    "\\* h ~i~",
    "[a](/u⏎\"t\") x ~b~",
    "[a](⏎/u) y [m](/v)",
    "![i](/p.png⏎\"t\") z ~c~",
    "see [a][b⏎c] w ~d~",
    "[l](/u \"t⏎u\") v ~e~",
    "x [a](/u⏎  \"t\") [n](/w)",
    "[y⏎z]: /w \"t⏎u\"",
    "[q]: /w⏎\"t\"",
    "[a] ~f~ [c](/u \"t⏎u\")",
    "| pb |⏎| - |",
    "[^1]: n [a](/u⏎\"t\") [m](/v)",
    "x [^1]",
    "=",
    "[ca](/u) | cb",
    "-|-",
    "[a](/u⏎\"t\") \0 [b](/v)",
    "[r]: /d⏎=⏎[ra](/u) | rb⏎-|-",
    "[r]: /d⏎=⏎[l](/v⏎\"t\") [m](/w)⏎[sa](/u) | sb⏎-|-",
    "|-",
    "[q]: /w⏎=⏎|-",
];

/// One generated document per proptest case; seed 0 is a fixed point of the generator's
/// xorshift and is left out.
fn generated_document() -> impl Strategy<Value = String> {
    (1u32..=u32::MAX).prop_map(|seed| documents(seed, 1, PIECES).swap_remove(0))
}

/// The table counts lines as today's `render` does: `markdown.rs:481`'s
/// `body.split('\n')` over the normalised body, so a document of `k` `\n`s has `k + 1`
/// lines and a trailing line end opens a last, empty line. Pinned against `render` itself
/// on a heading at the last line, with and without a trailing newline.
#[test]
fn line_table_counts_lines_as_render_does() {
    for md in documents(11, 200, PIECES) {
        let normalised = normalize_line_ends(&md);
        let table = LineTable::new(&normalised);
        assert_eq!(table.line_count(), normalised.split('\n').count(), "{md:?}");
    }
    let opts = RenderOptions::default();
    for (md, heading_line, lines) in [("x\n\n# h", 3, 3), ("x\n\n# h\n", 3, 4), ("# h", 1, 1)] {
        let out = render(md, &opts);
        assert_eq!(out.headings.len(), 1, "{md:?}");
        assert_eq!(out.headings[0].line, heading_line, "{md:?}: today's render");
        let table = LineTable::new(md);
        assert_eq!(table.line(md.find('#').unwrap()), heading_line, "{md:?}");
        assert_eq!(table.line_count(), lines, "{md:?}");
    }
}

#[test]
fn line_of_every_offset_is_the_line_its_byte_sits_on() {
    let text = prop::collection::vec(
        prop_oneof![
            Just("a"),
            Just("b "),
            Just("\n"),
            Just("\r\n"),
            Just("\r"),
            Just("é"),
            Just("日本"),
            Just("\u{1F600}"),
        ],
        0..40,
    )
    .prop_map(|parts| parts.concat());
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig::with_cases(2000));
    runner
        .run(&text, |raw| {
            let normalised = normalize_line_ends(&raw);
            let table = LineTable::new(&normalised);
            for o in 0..=normalised.len() {
                prop_assert_eq!(
                    table.line(o),
                    line_by_counting(&normalised, o),
                    "{:?} at {}",
                    normalised,
                    o
                );
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn line_col_counts_characters_not_bytes() {
    let text = "é\nab";
    let table = LineTable::new(text);
    assert_eq!(table.line_col(text, 0), (1, 1));
    // Offset 2 is the `\n`: the second character of line 1.
    assert_eq!(table.line_col(text, 2), (1, 2));
    assert_eq!(table.line_col(text, 3), (2, 1));
    assert_eq!(table.line_col(text, 4), (2, 2));
}

/// The normalised text keeps the original's lines and columns: `\r\n` is one byte shorter,
/// at a line end, and `\r` is as long as the `\n` it becomes.
#[test]
fn crlf_and_bare_cr_keep_the_same_lines_and_columns() {
    let raw = "a\r\nb\rc\n";
    let normalised = normalize_line_ends(raw);
    assert_eq!(normalised, "a\nb\nc\n");
    let table = LineTable::new(&normalised);
    for (letter, want) in [('a', (1, 1)), ('b', (2, 1)), ('c', (3, 1))] {
        let at = normalised.find(letter).unwrap();
        assert_eq!(table.line_col(&normalised, at), want, "{letter}");
        // The same byte in the original sits on the same line, at the same column: a
        // `\r\n`, a bare `\r` or a `\n` each end one line of the original.
        let raw_at = raw.find(letter).unwrap();
        let raw_line = normalize_line_ends(&raw[..raw_at]).matches('\n').count() + 1;
        let raw_col = raw[..raw_at]
            .rsplit(['\n', '\r'])
            .next()
            .unwrap()
            .chars()
            .count()
            + 1;
        assert_eq!((raw_line, raw_col), want, "{letter} in the original");
    }
    assert_eq!(table.line_count(), 4);
    assert_eq!(table.line_range(1), 0..1);
    assert_eq!(table.line_range(3), 4..5);
    assert_eq!(table.line_range(4), 6..6);
}

/// A guard on the merged-run offsets, green before the table exists: the merged run's `offset_of` lands every
/// text index on the line of the piece holding it.
#[test]
fn offset_of_a_merged_run_maps_to_the_same_line_as_its_piece() {
    let md = "a \\* www.x.y\n";
    let table = LineTable::new(md);
    let mut runs = 0;
    for merged in TextMerge::new(events(md, GFM)) {
        let Merged::Run(run) = merged else {
            continue;
        };
        runs += 1;
        for piece in &run.pieces {
            assert_eq!(table.line(piece.range.start), 1, "{run:?}");
            for i in piece.text_start..=piece.text_start + piece.text_len {
                assert_eq!(table.line(run.offset_of(i)), 1, "{run:?} at {i}");
            }
        }
        assert_eq!(table.line(run.offset_of(run.text.len())), 1, "{run:?}");
    }
    assert!(
        runs >= 2,
        "{runs} runs: the escape splits the text, the autolink splits it again"
    );
}

/// The position property, generalised from `markdown.rs::texts_sit_where_they_are_written`
/// and green before this package (the parser's ranges are right by construction): it is the
/// characterisation that guards the parser switch. For every `Text` free of escapes,
/// entities and tabs, the source bytes at its range are the text (or hold it, when the range
/// is longer: an escape or entity inside), and the table's line of the range's start is the
/// line the range begins on.
///
/// The property is over the parser's own events (`parse_with_offsets`), not `events()`:
/// the autolink pass merges every run of consecutive `Text` events by design, a
/// code block's lines included, so a merged run's range spans the stripped indentation,
/// container prefixes and escapes between its pieces and holds bytes that are not in its
/// text. What positions rely on is that a merged run still starts where its first piece
/// starts; the second loop asserts that over `events()`.
#[test]
fn every_text_event_sits_where_it_is_written() {
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig::with_cases(2000));
    runner
        .run(&generated_document(), |raw| {
            let md = normalize_line_ends(&raw);
            let table = LineTable::new(&md);
            // Every merged `Text` out of `events()` starts where one of the parser's `Text`
            // events starts: its first piece's start (the pieces are no autolink's gaps:
            // nothing in PIECES is an extended autolink).
            let piece_starts: Vec<usize> = parse_with_offsets(&md, GFM)
                .filter_map(|(event, range)| matches!(event, Event::Text(_)).then_some(range.start))
                .collect();
            for (event, range) in events(&md, GFM) {
                if let Event::Text(t) = event {
                    prop_assert!(
                        piece_starts.contains(&range.start),
                        "{:?}: merged {:?} at {:?} starts at no parser text",
                        md,
                        t,
                        range
                    );
                }
            }
            for (event, range) in parse_with_offsets(&md, GFM) {
                let Event::Text(t) = event else {
                    continue;
                };
                if t.is_empty() || t.contains(['\\', '&', '\t']) {
                    continue;
                }
                let Some(bytes) = md.get(range.clone()) else {
                    return Err(TestCaseError::fail(format!(
                        "{md:?}: {t:?} has range {range:?} outside the text"
                    )));
                };
                let placed = if range.len() > t.len() {
                    bytes.contains(t.as_ref())
                } else {
                    bytes.starts_with(t.as_ref())
                };
                prop_assert!(
                    placed,
                    "{:?}: {:?} at {:?}, the source has {:?}",
                    md,
                    t,
                    range,
                    bytes
                );
                prop_assert_eq!(
                    table.line(range.start),
                    line_by_counting(&md, range.start),
                    "{:?}: {:?} at {:?}",
                    md,
                    t,
                    range
                );
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn writer_positions_map_to_lines_once_per_line() {
    let md = "a **b**\n\nc `d`\n\ne\n";
    let table = LineTable::new(md);
    let out = write(events(md, GFM), &[], WriterOptions::NATIVE);
    assert_eq!(
        out.html,
        "<p>a <strong>b</strong></p>\n<p>c <code>d</code></p>\n<p>e</p>\n"
    );
    let lines = out.positions_as_lines(&table);
    // The paragraphs sit on lines 1, 3 and 5; the inline nodes inside each are on its
    // line and are dropped: one pair per line, at the line's first HTML offset.
    let second = out.html.find("<p>c").unwrap();
    let third = out.html.find("<p>e").unwrap();
    assert!(
        lines == [(0, 1), (second, 3), (third, 5)],
        "{lines:?} from {:?}",
        out.positions
    );
}

/// In a debug build an offset past the text is a bug, and the message names the offset;
/// release builds compile the assertion out and clamp to the last line, where this test
/// would fail for the wrong reason, so it is debug-only.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "offset")]
fn a_position_outside_the_text_is_a_bug_in_debug() {
    let text = "ab\nc";
    let table = LineTable::new(text);
    let _ = table.line(text.len() + 1);
}
