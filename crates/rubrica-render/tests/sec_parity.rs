//! `@sec` parity with `check-packages.cjs`, by construction: Rubrica runs
//! its harvest (`sec::check_packages_markers`, a port of its regex with
//! JavaScript's line ends), and every marker it harvests is either an anchor
//! on its heading or a finding. So a page with no `@sec` finding has exactly
//! check-packages' ids, all anchored, and `Rendered::secs` is always its set.
//!
//! The port is checked against the JavaScript: the harvest loop of
//! `check-packages.cjs` answered every document of
//! `fixtures/sec-harvest-differential.json`, which is frozen here.

use std::collections::HashMap;
use std::path::PathBuf;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use rubrica_render::sec::{check_packages_markers, HarvestedMarker};
use rubrica_render::{render, render_page, RenderFinding, RenderOptions, SecMarker};

/// Lines, line ends and markers of every shape the two readings can differ
/// on: markers alone, indented, tabbed, with no id, cut over two lines;
/// headings (ATX, setext, empty, invisible, NBSP); fences, HTML blocks,
/// containers; every line end JavaScript or the parser knows; a byte-order mark.
const LINES: &[&str] = &[
    "<!-- @sec id=a -->",
    "<!-- @sec id=b tags=x,y -->",
    "<!--@sec id=c-->",
    "<!--\t@sec\tid=a\t-->\t",
    "  <!-- @sec id=d -->",
    "    <!-- @sec id=e -->",
    "\t<!-- @sec id=f -->",
    "<!-- @sec tags=t -->",
    "<!-- @sec id= -->",
    "<!-- @sec id=g",
    "-->",
    "id=h -->",
    "<!-- @sec id=a> -->",
    "<!-- @sec id=\u{feff}a -->",
    "<!-- @sec id=a\u{85}b -->",
    "<!-- @sec id=i --> x",
    "x <!-- @sec id=j -->",
    "\u{feff}<!-- @sec id=k -->",
    "# H",
    "## Two",
    "#",
    "# \u{200b}",
    "   ### x",
    "#\u{a0}H",
    "H",
    "===",
    "```",
    "~~~",
    "<div>",
    "</div>",
    "<details>",
    "<!-- c -->",
    "> <!-- @sec id=l -->",
    "> # Q",
    "- <!-- @sec id=m -->",
    "- x",
    "  <!-- @sec id=n -->",
    "1. x",
    "[^1]: n",
    "x[^1]",
    "",
    "text",
    "| a |",
    "|---|",
];
const ENDS: &[&str] = &["\n", "\n", "\n", "\r\n", "\r", "\u{2028}", "\u{2029}"];

fn documents(seed: u64, count: usize) -> Vec<String> {
    let mut s = seed | 1;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s as usize
    };
    (0..count)
        .map(|_| {
            let mut doc = String::new();
            for _ in 0..1 + next() % 10 {
                doc.push_str(LINES[next() % LINES.len()]);
                doc.push_str(ENDS[next() % ENDS.len()]);
            }
            if next() % 4 == 0 {
                doc.pop();
            }
            doc
        })
        .collect()
}

/// A harvest as the JavaScript writes it: `[line, id or null, [tags]]`.
type Row = (usize, Option<String>, Vec<String>);

fn rows(harvest: &[HarvestedMarker]) -> Vec<Row> {
    harvest
        .iter()
        .map(|h| match &h.marker {
            Some(m) => (h.line, Some(m.id.clone()), m.tags.clone()),
            None => (h.line, None, Vec::new()),
        })
        .collect()
}

/// `sec-harvest-differential.json` is frozen: it is regenerated only by the private
/// conformance suite's node tool, never here.
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sec-harvest-differential.json")
}

/// Pages: a front matter whose block scalar holds lines of [`LINES`] (those
/// YAML takes as they are: no byte-order mark, U+0085, U+2028 or U+2029,
/// and `\n` ends), then a body of [`documents`]' kind. A marker left open
/// in the block scalar can run on into the body.
fn pages(seed: u64, count: usize) -> Vec<String> {
    let yaml_safe: Vec<&str> = LINES
        .iter()
        .copied()
        .filter(|l| !l.contains(['\u{feff}', '\u{85}']))
        .collect();
    let mut s = seed | 1;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s as usize
    };
    let bodies = documents(seed ^ 0x9e37_79b9, count);
    bodies
        .into_iter()
        .map(|body| {
            let mut src = String::from("---\ntitle: T\nnote: |2\n");
            for _ in 0..1 + next() % 4 {
                src.push_str("  ");
                src.push_str(yaml_safe[next() % yaml_safe.len()]);
                src.push('\n');
            }
            src.push_str("---\n");
            src.push_str(&body);
            src
        })
        .collect()
}

/// The claim: `secs` is the harvest's ids; each harvested id is an anchor on
/// a heading or named by a finding at its line; each harvested marker with
/// no id is a `SecWithoutId`, and each `SecWithoutId` is a harvested marker
/// with no id or a marker with no id that the oracle parser's parse has; each anchor
/// written is a harvested marker's. Returns the lines with more parsed
/// markers with no id than harvested ones (see [`assert_claim`]).
fn assert_parity(doc: &str) -> usize {
    let harvest = check_packages_markers(doc);
    let out = render(doc, &RenderOptions::default());
    assert_claim(doc, doc, &harvest, &out)
}

/// The body lines (the oracle's, upstream pulldown-cmark) of the markers with no id that its
/// parse of `body` has: an HTML block, in any container, that is one line
/// matching check-packages' regex, with no id. Found here from the oracle parser and
/// the public harvest, not from Rubrica's parse, so the reverse direction is
/// checked against something Rubrica does not compute.
fn parsed_no_id_lines(body: &str) -> Vec<usize> {
    let body = body.replace("\r\n", "\n").replace('\r', "\n");
    // The oracle drops a leading byte-order mark, as the pipeline does (upstream
    // pulldown-cmark keeps it as text, which would hide the first line's block).
    let body = body.strip_prefix('\u{feff}').unwrap_or(&body).to_owned();
    let options =
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES;
    let mut out = Vec::new();
    // The open HTML block: its first line (1-based) and its text so far.
    let mut open: Option<(usize, String)> = None;
    for (event, range) in Parser::new_ext(&body, options).into_offset_iter() {
        match event {
            Event::Start(Tag::HtmlBlock) => {
                let line = 1 + body.as_bytes()[..range.start]
                    .iter()
                    .filter(|&&b| b == b'\n')
                    .count();
                open = Some((line, String::new()));
            }
            Event::Html(text) => {
                if let Some((_, literal)) = open.as_mut() {
                    literal.push_str(&text);
                }
            }
            Event::End(TagEnd::HtmlBlock) => {
                let Some((line, literal)) = open.take() else {
                    continue;
                };
                let text = literal.strip_suffix('\n').unwrap_or(&literal);
                // One line, by the parser's and by JavaScript's line ends, so
                // a match of the harvest's regex is the whole literal.
                if text.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
                    continue;
                }
                if let [h] = check_packages_markers(text).as_slice() {
                    if h.marker.is_none() {
                        out.push(line);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The claim for a page file: check-packages harvests the whole file, so
/// each match that starts in the front matter is a `SecInFrontMatter` at its
/// file line, and the body's matches (by body line) are the claim above.
fn assert_page_parity(src: &str) -> usize {
    let Ok((_, out, offset)) = render_page(src, &RenderOptions::default()) else {
        return 0;
    };
    let (front, body): (Vec<_>, Vec<_>) = check_packages_markers(src)
        .into_iter()
        .partition(|h| h.line <= offset);
    let reported: Vec<_> = out
        .findings
        .iter()
        .filter(|f| matches!(f, RenderFinding::SecInFrontMatter { .. }))
        .cloned()
        .collect();
    let want: Vec<_> = front
        .iter()
        .map(|h| RenderFinding::SecInFrontMatter {
            id: h.marker.as_ref().map(|m| m.id.clone()),
            line: h.line,
        })
        .collect();
    assert_eq!(reported, want, "{src:?}");
    let body: Vec<HarvestedMarker> = body
        .into_iter()
        .map(|h| HarvestedMarker {
            line: h.line - offset,
            marker: h.marker.map(|m| SecMarker {
                line: m.line - offset,
                ..m
            }),
        })
        .collect();
    let Ok((_, text, _)) = rubrica_render::split_front_matter(src) else {
        unreachable!("render_page read it");
    };
    assert_claim(src, text, &body, &out)
}

/// The claim on `out`, the render of `body` (the page file `doc`'s, or `doc`
/// itself), given check-packages' harvest of the body. Returns the number of
/// lines with more parsed markers with no id than harvested ones: where the
/// reverse direction is what holds a `SecWithoutId` to account.
fn assert_claim(
    doc: &str,
    body: &str,
    harvest: &[HarvestedMarker],
    out: &rubrica_render::Rendered,
) -> usize {
    let want: Vec<SecMarker> = harvest.iter().filter_map(|h| h.marker.clone()).collect();
    assert_eq!(out.secs, want, "{doc:?}");
    let reported = |id: &str, at: usize| {
        out.findings.iter().any(|f| match f {
            RenderFinding::SecWithoutHeading { id: i, line }
            | RenderFinding::DuplicateSecId { id: i, line, .. }
            | RenderFinding::SecAnchorIsAHeadingId { id: i, line }
            | RenderFinding::SecInContainer { id: i, line }
            | RenderFinding::SecOnEmptyHeading { id: i, line }
            | RenderFinding::SecNotRendered { id: i, line, .. } => i == id && *line == at,
            RenderFinding::AttributesTooCostly {
                element,
                attribute,
                line,
            } => element == "span" && attribute == "id" && *line == at,
            _ => false,
        })
    };
    for h in harvest {
        match &h.marker {
            Some(m) => {
                let anchored = out.headings.iter().any(|x| x.sec.as_ref() == Some(m));
                assert!(
                    anchored || reported(&m.id, h.line),
                    "{doc:?}: {m:?} is neither anchored nor reported: {:?}",
                    out.findings
                );
            }
            None => assert!(
                out.findings
                    .contains(&RenderFinding::SecWithoutId { line: h.line }),
                "{doc:?}: no SecWithoutId at {}: {:?}",
                h.line,
                out.findings
            ),
        }
    }
    // The reverse: a `SecWithoutId` is a harvested marker with no id, or a
    // marker with no id in the parse, whether or not check-packages sees it
    // (a front-matter match can take a body marker with no
    // id, and a byte-order mark hides one from the harvest). A line has as
    // many as the more of the two: a parsed one pairs with a harvested one.
    let mut without_id: HashMap<usize, (usize, usize)> = HashMap::new();
    for h in harvest.iter().filter(|h| h.marker.is_none()) {
        without_id.entry(h.line).or_default().0 += 1;
    }
    for line in parsed_no_id_lines(body) {
        without_id.entry(line).or_default().1 += 1;
    }
    let mut reported: HashMap<usize, usize> = HashMap::new();
    for f in &out.findings {
        if let RenderFinding::SecWithoutId { line } = f {
            *reported.entry(*line).or_default() += 1;
        }
    }
    for (line, n) in &reported {
        let (harvested, parsed) = without_id.get(line).copied().unwrap_or_default();
        assert_eq!(
            *n,
            harvested.max(parsed),
            "{doc:?}: {n} SecWithoutId at {line}, {harvested} harvested, {parsed} parsed: {:?}",
            out.findings
        );
    }
    let anchors: Vec<&SecMarker> = out.headings.iter().filter_map(|x| x.sec.as_ref()).collect();
    for m in &anchors {
        assert!(want.contains(m), "{doc:?}: {m:?} anchored, not harvested");
    }
    assert_eq!(
        out.html.matches("<span class=\"sec\"").count(),
        anchors.len(),
        "{doc:?}: {}",
        out.html
    );
    without_id.values().filter(|(h, p)| p > h).count()
}

/// A marker with no id is `SecWithoutId` whether or not
/// check-packages sees it. Here it does not, for each of its two causes: a
/// marker left open in the front matter runs on into the body's, and a
/// byte-order mark before the body's (the parser drops it, JavaScript keeps it).
#[test]
fn a_marker_with_no_id_is_sec_without_id_where_check_packages_misses_it() {
    let cases: [(&str, Vec<RenderFinding>); 2] = [
        (
            "---\ntitle: t\nd: |\n  <!-- @sec id=fm\n---\n<!-- @sec tags=t -->\n# H\n",
            vec![
                RenderFinding::SecInFrontMatter {
                    id: Some("fm".into()),
                    line: 4,
                },
                RenderFinding::SecWithoutId { line: 1 },
            ],
        ),
        (
            "---\ntitle: t\n---\n\u{feff}<!-- @sec tags=t -->\n# H\n",
            vec![RenderFinding::SecWithoutId { line: 1 }],
        ),
    ];
    for (src, want) in cases {
        let (_, out, offset) = render_page(src, &RenderOptions::default()).unwrap();
        assert_eq!(out.findings, want, "{src:?}");
        // check-packages has no match in the body: the finding is the parse's.
        assert!(
            check_packages_markers(src).iter().all(|h| h.line <= offset),
            "{src:?}"
        );
        assert_eq!(assert_page_parity(src), 1, "{src:?}");
    }
}

#[derive(serde::Deserialize)]
struct Case {
    doc: String,
    harvest: Vec<Row>,
}

#[test]
fn the_port_harvests_what_check_packages_harvests() {
    let cases: Vec<Case> = serde_json::from_str(
        &std::fs::read_to_string(fixture()).expect("fixtures/sec-harvest-differential.json"),
    )
    .unwrap();
    assert!(cases.len() >= 5_000, "{}", cases.len());
    for c in &cases {
        assert_eq!(
            rows(&check_packages_markers(&c.doc)),
            c.harvest,
            "{:?}",
            c.doc
        );
    }
}

#[test]
fn every_marker_check_packages_harvests_is_anchored_or_reported() {
    let cases: Vec<Case> =
        serde_json::from_str(&std::fs::read_to_string(fixture()).unwrap()).unwrap();
    let mut parse_only = 0;
    for c in &cases {
        parse_only += assert_parity(&c.doc);
    }
    for doc in documents(0x5ec0_0002, 50_000) {
        parse_only += assert_parity(&doc);
    }
    // Never vacuous: the reverse direction meets markers with no id the
    // harvest misses (after a byte-order mark).
    assert!(parse_only > 20, "{parse_only}");
}

#[test]
fn every_marker_check_packages_harvests_from_a_page_file_is_anchored_or_reported() {
    let docs = pages(0x5ec0_0004, 20_000);
    let (mut in_front, mut read, mut parse_only) = (0, 0, 0);
    for src in &docs {
        let Ok((_, _, offset)) = rubrica_render::split_front_matter(src) else {
            continue;
        };
        read += 1;
        in_front += check_packages_markers(src)
            .iter()
            .filter(|h| h.line <= offset)
            .count();
        parse_only += assert_page_parity(src);
    }
    // Never vacuous: nearly every page reads, many matches start in the
    // front matter, and the reverse direction meets body markers with no id
    // that a front-matter match took or a byte-order mark hid.
    assert!(read > 19_000, "{read}");
    assert!(in_front > 5_000, "{in_front}");
    assert!(parse_only > 30, "{parse_only}");
}
