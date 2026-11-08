//! The writer's spec gate: every CommonMark 0.31.2 example is written byte for
//! byte as the spec writes it, with no normalisation of either side, and so is every GFM
//! 0.29 base example except the ones whose HTML changed between CommonMark 0.29 and 0.31.2.
//!
//! The parser is Rubrica's with the GFM option set, **without** the extended-autolink
//! scanner (`parse_with_offsets`, not `events`): extended autolinks are a GFM extension,
//! and the CommonMark and GFM base examples are written without it (four in each suite
//! hold a bare URL or email the scanner would link); the extension's own examples are
//! the autolink scanner's tests. No CommonMark example and no GFM base example produces a table,
//! strikethrough, task-list or footnote event. The last test pins exactly which examples
//! the scanner changes.

use rubrica_markdown::{events, parse_with_offsets, GFM};
use rubrica_render::writer::{write, WriterOptions};

// the data shape
#[derive(serde::Deserialize)]
struct Example {
    markdown: String,
    html: String,
    #[serde(default)]
    example: usize,
    #[serde(default)]
    extension: String,
    #[serde(default)]
    section: String,
}

fn commonmark() -> Vec<Example> {
    serde_json::from_str(include_str!("fixtures/spec/commonmark-0.31.2.json")).unwrap()
}

fn gfm() -> Vec<Example> {
    serde_json::from_str(include_str!("fixtures/spec/gfm-0.29.json")).unwrap()
}

/// The example's name in a failure: `#<n> <section>`, where `n` is the suite's own number
/// (CommonMark) or the 1-based position in the file (GFM, which numbers nothing).
fn label(i: usize, ex: &Example) -> String {
    let n = if ex.example == 0 { i + 1 } else { ex.example };
    let what = if ex.section.is_empty() {
        if ex.extension.is_empty() {
            "base"
        } else {
            &ex.extension
        }
    } else {
        &ex.section
    };
    format!("#{n} {what}")
}

/// The source through Rubrica's parser (no extension pass) and writer, `NATIVE`, no
/// headings.
fn render(md: &str) -> String {
    write(parse_with_offsets(md, GFM), &[], WriterOptions::NATIVE).html
}

/// The same through `events`, the stream `render()` will consume: the scanner on.
fn render_with_scanner(md: &str) -> String {
    write(events(md, GFM), &[], WriterOptions::NATIVE).html
}

/// A byte-level mismatch: where the two strings first differ, and what each holds there.
#[derive(Debug, PartialEq, Eq)]
struct Mismatch {
    at: usize,
    expected_from_there: Vec<u8>,
    got_from_there: Vec<u8>,
}

/// Compares `expected` and `got` byte for byte. Nothing is trimmed, replaced, re-cased or
/// re-parsed on either side: equal means the same bytes.
fn compare(expected: &str, got: &str) -> Option<Mismatch> {
    let (e, g) = (expected.as_bytes(), got.as_bytes());
    if e == g {
        return None;
    }
    let at = e.iter().zip(g).take_while(|(a, b)| a == b).count();
    Some(Mismatch {
        at,
        expected_from_there: e[at..].to_vec(),
        got_from_there: g[at..].to_vec(),
    })
}

/// `s` in Rust's debug form, cut to about `max` characters.
fn quoted(s: &str, max: usize) -> String {
    let q = format!("{s:?}");
    if q.chars().count() <= max {
        q
    } else {
        let cut: String = q.chars().take(max).collect();
        format!("{cut}…")
    }
}

fn bytes_quoted(b: &[u8], max: usize) -> String {
    quoted(&String::from_utf8_lossy(b), max)
}

/// One failure's report: the example, both whole strings, and the byte-level diff.
fn report(label: &str, expected: &str, got: &str, m: &Mismatch) -> String {
    format!(
        "{label} expected {} got {}\n    bytes differ at {}: expected {} got {}",
        quoted(expected, 120),
        quoted(got, 120),
        m.at,
        bytes_quoted(&m.expected_from_there, 60),
        bytes_quoted(&m.got_from_there, 60),
    )
}

/// Runs every example of `suite` that `included` admits through the writer and panics with
/// every mismatch counted and the first three reported with a byte-level diff.
fn assert_suite_exact(suite: &[Example], included: impl Fn(usize, &Example) -> bool) {
    let mut failures = Vec::new();
    let mut total = 0;
    for (i, ex) in suite.iter().enumerate() {
        if !included(i, ex) {
            continue;
        }
        total += 1;
        let got = render(&ex.markdown);
        if let Some(m) = compare(&ex.html, &got) {
            failures.push(report(&label(i, ex), &ex.html, &got, &m));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {total} examples differ; first: {}",
        failures.len(),
        failures
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  next: ")
    );
}

#[test]
fn commonmark_652_exact() {
    let suite = commonmark();
    assert_eq!(suite.len(), 652);
    assert_suite_exact(&suite, |_, _| true);
}

/// The GFM 0.29 base examples whose HTML changed between CommonMark 0.29 and 0.31.2
/// (1-based position in `gfm-0.29.json`, and what changed). The parser follows 0.31.2, so
/// these are not compared against 0.29's HTML; the test checks that each still differs, so
/// a stale entry is a failure too. Any other base example that differs is a writer bug.
///
/// The red commit started from the ten candidates measured against upstream's writer; the
/// tenth, #144 (`- <div>` in a tight item), is exact with Rubrica's writer (CommonMark's
/// example 175, the same source and HTML) and left the list in the green step. The nine
/// that remain are the emphasis examples whose source CommonMark 0.31.2 writes with nested
/// `<strong>` where GFM 0.29's HTML has one: each names its 0.31.2 twin.
const SPEC_VERSION: &[(usize, &str)] = &[
    (
        398,
        "spec version: `__foo, __bar__, baz__`: 0.29 writes `<strong>foo, bar, baz</strong>`, 0.31.2 (example 389) nests the inner pair, `<strong>foo, <strong>bar</strong>, baz</strong>`",
    ),
    (
        426,
        "spec version: `foo******bar*********baz`: 0.29 writes one `<strong>bar</strong>`, 0.31.2 (example 417) three nested `<strong>` from the six-delimiter run",
    ),
    (
        434,
        "spec version: `__foo __bar__ baz__`: 0.29 writes `<strong>foo bar baz</strong>`, 0.31.2 (example 425) nests the inner pair",
    ),
    (
        435,
        "spec version: `____foo__ bar__`: 0.29 writes `<strong>foo bar</strong>`, 0.31.2 (example 426) `<strong><strong>foo</strong> bar</strong>`",
    ),
    (
        436,
        "spec version: `**foo **bar****`: 0.29 writes `<strong>foo bar</strong>`, 0.31.2 (example 427) `<strong>foo <strong>bar</strong></strong>`",
    ),
    (
        473,
        "spec version: `****foo****`: 0.29 writes one `<strong>`, 0.31.2 (example 464) two nested (rule 13 on a four-delimiter run)",
    ),
    (
        474,
        "spec version: `____foo____`: 0.29 writes one `<strong>`, 0.31.2 (example 465) two nested",
    ),
    (
        475,
        "spec version: `******foo******`: 0.29 writes one `<strong>`, 0.31.2 (example 466) three nested",
    ),
    (
        477,
        "spec version: `_____foo_____`: 0.29 writes `<em><strong>foo</strong></em>`, 0.31.2 (example 468) `<em><strong><strong>foo</strong></strong></em>`",
    ),
];

#[test]
fn gfm_base_examples_exact_where_commonmark_agrees() {
    let suite = gfm();
    assert_eq!(suite.len(), 672);
    let base = |ex: &Example| ex.extension.is_empty();
    assert_eq!(suite.iter().filter(|ex| base(ex)).count(), 648);
    // Every excluded example is a base example that really differs.
    for &(n, reason) in SPEC_VERSION {
        let ex = &suite[n - 1];
        assert!(base(ex), "#{n} is an extension example, not a base one");
        assert!(
            compare(&ex.html, &render(&ex.markdown)).is_some(),
            "#{n} is written as GFM 0.29 writes it; its entry is stale ({reason})"
        );
    }
    assert_suite_exact(&suite, |i, ex| {
        base(ex) && !SPEC_VERSION.iter().any(|&(n, _)| n == i + 1)
    });
}

/// A meta-test, green before and after the writer: the comparison is byte-exact, so a
/// deliberately mis-spelled `<br>` fails it, and the helper itself normalises nothing.
#[test]
fn no_normalisation_in_the_gate() {
    assert_eq!(compare("<p>a<br />\nb</p>\n", "<p>a<br />\nb</p>\n"), None);
    let m =
        compare("<p>a<br />\nb</p>\n", "<p>a<br>\nb</p>\n").expect("a mis-spelled <br> differs");
    assert_eq!(m.at, 7);
    assert_eq!(m.expected_from_there, b" />\nb</p>\n");
    assert_eq!(m.got_from_there, b">\nb</p>\n");
    // Whitespace, case and trailing newlines count too.
    assert!(compare("<p>a</p>\n", "<p>a</p>").is_some());
    assert!(compare("<p>a</p>\n", "<P>a</P>\n").is_some());
    assert!(compare("<p>a</p>\n", " <p>a</p>\n").is_some());
    // This file's helpers do not normalise: no replace, trim or re-casing anywhere in it.
    // Never `include_str!("../…")`: a test reads within its crate.
    let me = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/writer_spec.rs"));
    // (Spelled in pieces so this list is not itself a hit.)
    for forbidden in [
        concat!(".", "replace", "("),
        concat!(".", "trim", "("),
        concat!(".", "trim_end", "("),
        concat!(".", "to_lowercase", "("),
        concat!(".", "to_ascii_lowercase", "("),
    ] {
        assert!(
            !me.contains(forbidden),
            "the gate normalises: {forbidden} is in writer_spec.rs"
        );
    }
}

/// The GFM 0.29 extension examples, written through `events` (the scanner on, since the
/// autolink examples are the scanner's): tables, task lists (`extension == "disabled"`),
/// strikethrough and autolinks, 23 in all, byte-exact. The one `tagfilter` example (#652)
/// is excluded: filtering `<title>`, `<xmp>` and the other disallowed raw tags is the
/// sanitizer's job, not the writer's (the writer passes raw HTML through under
/// `RawHtmlMode::Raw`). Each example runs under `catch_unwind`, so a writer that panics on
/// an event counts as a mismatch, not as an abort of the whole gate.
#[test]
fn gfm_extensions_exact_except_tagfilter() {
    let suite = gfm();
    assert_eq!(suite.len(), 672);
    let included = |ex: &Example| {
        matches!(
            ex.extension.as_str(),
            "autolink" | "table" | "disabled" | "strikethrough"
        )
    };
    let counts = |name: &str| suite.iter().filter(|ex| ex.extension == name).count();
    assert_eq!(
        (
            counts("autolink"),
            counts("table"),
            counts("disabled"),
            counts("strikethrough")
        ),
        (11, 8, 2, 2)
    );
    assert_eq!(counts("tagfilter"), 1);
    assert_eq!(suite[652 - 1].extension, "tagfilter");
    let mut failures = Vec::new();
    let mut total = 0;
    for (i, ex) in suite.iter().enumerate() {
        if !included(ex) {
            continue;
        }
        total += 1;
        let md = ex.markdown.as_str();
        let got = std::panic::catch_unwind(|| render_with_scanner(md));
        match got {
            Ok(got) => {
                if let Some(m) = compare(&ex.html, &got) {
                    failures.push(report(&label(i, ex), &ex.html, &got, &m));
                }
            }
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| payload.downcast_ref::<&str>().copied())
                    .unwrap_or("(non-string panic)");
                failures.push(format!("{} panicked: {}", label(i, ex), quoted(msg, 120)));
            }
        }
    }
    assert_eq!(total, 23);
    assert!(
        failures.is_empty(),
        "{} of {total} examples differ; first: {}",
        failures.len(),
        failures
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  next: ")
    );
}

/// The examples whose HTML the extended-autolink scanner changes: each holds a bare URL or
/// email address the GFM extension links and the spec, without the extension, writes as
/// text. CommonMark's are numbered by the suite, GFM's by position in `gfm-0.29.json`.
const SCANNER_CHANGES_COMMONMARK: &[usize] = &[606, 608, 611, 612];
const SCANNER_CHANGES_GFM_BASE: &[usize] = &[614, 616, 619, 620];

/// A characterisation test: the scanner changes the HTML of exactly the listed examples
/// and of no other. Every CommonMark example and every GFM base example goes through both
/// `parse_with_offsets` and `events`; the spec-version examples are included (the scanner
/// has no business with them either).
#[test]
fn the_scanner_changes_exactly_the_extended_autolink_examples() {
    fn changed(suite: &[Example], base_only: bool) -> Vec<usize> {
        suite
            .iter()
            .enumerate()
            .filter(|(_, ex)| !base_only || ex.extension.is_empty())
            .filter(|(_, ex)| render(&ex.markdown) != render_with_scanner(&ex.markdown))
            .map(|(i, ex)| if ex.example == 0 { i + 1 } else { ex.example })
            .collect()
    }
    let cm = commonmark();
    assert_eq!(cm.len(), 652);
    assert_eq!(changed(&cm, false), SCANNER_CHANGES_COMMONMARK);
    let gfm = gfm();
    assert_eq!(gfm.iter().filter(|ex| ex.extension.is_empty()).count(), 648);
    assert_eq!(changed(&gfm, true), SCANNER_CHANGES_GFM_BASE);
}
