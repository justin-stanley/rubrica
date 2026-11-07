//! The GFM extended-autolink scanner through `events`: the 11 spec examples, Rubrica's
//! nested-candidate rule, what is never scanned, the delimiters, exact ranges, every byte
//! emitted once, and linearity of the scan on adversarial runs.

use std::ops::Range;
use std::time::{Duration, Instant};

use proptest::prelude::*;
use rubrica_markdown::autolink::scan_counting;
use rubrica_markdown::{events, Event, LinkType, Options, Tag, TagEnd, GFM};

// the data shape
#[derive(serde::Deserialize)]
struct Example {
    markdown: String,
    #[serde(default)]
    extension: String,
}

/// The markdown of the GFM spec's autolink examples (621–631), in order.
fn gfm_autolink_examples() -> Vec<String> {
    let all: Vec<Example> = serde_json::from_str(include_str!("spec/gfm-0.29.json")).unwrap();
    all.into_iter()
        .filter(|e| e.extension == "autolink")
        .map(|e| e.markdown)
        .collect()
}

/// What a document comes out as: text outside links, and `(dest_url, text)` per link.
#[derive(Debug, PartialEq, Eq)]
enum Item {
    Text(String),
    Link(String, String),
}

fn items(md: &str) -> Vec<Item> {
    let mut out = Vec::new();
    let mut link: Option<(String, String)> = None;
    for (e, _) in events(md, GFM) {
        match e {
            Event::Start(Tag::Link { dest_url, .. }) => {
                link = Some((dest_url.to_string(), String::new()));
            }
            Event::Text(t) => match &mut link {
                Some(l) => l.1.push_str(&t),
                None => out.push(Item::Text(t.to_string())),
            },
            Event::End(TagEnd::Link) => {
                let (dest, text) = link.take().expect("End(Link) without Start");
                out.push(Item::Link(dest, text));
            }
            _ => {}
        }
    }
    out
}

/// `(dest_url, text)` of every link, in order.
fn links(md: &str) -> Vec<(String, String)> {
    items(md)
        .into_iter()
        .filter_map(|i| match i {
            Item::Link(d, t) => Some((d, t)),
            Item::Text(_) => None,
        })
        .collect()
}

/// Every `Text` event's text, concatenated.
fn text(md: &str) -> String {
    events(md, GFM)
        .filter_map(|(e, _)| match e {
            Event::Text(t) => Some(t.to_string()),
            _ => None,
        })
        .collect()
}

/// `(text, range)` of every link: the range of its `Start`, which its `Text` and `End` share.
fn link_ranges(md: &str) -> Vec<(String, Range<usize>)> {
    let mut out = Vec::new();
    let mut cur: Option<(String, Range<usize>)> = None;
    for (e, r) in events(md, GFM) {
        match e {
            Event::Start(Tag::Link { .. }) => cur = Some((String::new(), r)),
            Event::Text(t) => {
                if let Some(c) = &mut cur {
                    assert_eq!(c.1, r, "{md:?}: the link's Text has another range");
                    c.1 = r;
                    c.0.push_str(&t);
                }
            }
            Event::End(TagEnd::Link) => {
                let c = cur.take().unwrap();
                assert_eq!(c.1, r, "{md:?}: the link's End has another range");
                out.push(c);
            }
            _ => {}
        }
    }
    out
}

fn pair(dest: &str, text: &str) -> (String, String) {
    (dest.to_string(), text.to_string())
}

#[test]
fn the_eleven_gfm_examples_produce_exactly_these_links() {
    let examples = gfm_autolink_examples();
    assert_eq!(examples.len(), 11);
    let business = (
        "http://www.google.com/search?q=Markup+(business)",
        "www.google.com/search?q=Markup+(business)",
    );
    let table: [Vec<(&str, &str)>; 11] = [
        vec![("http://www.commonmark.org", "www.commonmark.org")],
        vec![("http://www.commonmark.org/help", "www.commonmark.org/help")],
        vec![
            ("http://www.commonmark.org", "www.commonmark.org"),
            ("http://www.commonmark.org/a.b", "www.commonmark.org/a.b"),
        ],
        vec![business, business, business, business],
        vec![(
            "http://www.google.com/search?q=(business))+ok",
            "www.google.com/search?q=(business))+ok",
        )],
        vec![
            (
                "http://www.google.com/search?q=commonmark&hl=en",
                "www.google.com/search?q=commonmark&hl=en",
            ),
            (
                "http://www.google.com/search?q=commonmark",
                "www.google.com/search?q=commonmark",
            ),
        ],
        vec![("http://www.commonmark.org/he", "www.commonmark.org/he")],
        vec![
            ("http://commonmark.org", "http://commonmark.org"),
            (
                "https://encrypted.google.com/search?q=Markup+(business)",
                "https://encrypted.google.com/search?q=Markup+(business)",
            ),
            ("ftp://foo.bar.baz", "ftp://foo.bar.baz"),
        ],
        vec![("mailto:foo@bar.baz", "foo@bar.baz")],
        vec![("mailto:hello+xyz@mail.example", "hello+xyz@mail.example")],
        vec![
            ("mailto:a.b-c_d@a.b", "a.b-c_d@a.b"),
            ("mailto:a.b-c_d@a.b", "a.b-c_d@a.b"),
        ],
    ];
    for (i, md) in examples.iter().enumerate() {
        let n = 621 + i;
        let links = links(md);
        let expected: Vec<(String, String)> = table[i].iter().map(|(d, t)| pair(d, t)).collect();
        assert!(
            links == expected,
            "assertion failed: links == expected: example {n}: got {links:?} expected {expected:?}"
        );
        // Every byte is emitted exactly once: none of the 11 has a trailing space before a
        // line end, a backslash or an entity, so the Text concatenation is the source minus
        // its line ends.
        assert_eq!(text(md), md.replace('\n', ""), "example {n}: text");
    }
    // Example 626's second paragraph keeps `&hl;` as text after the link.
    assert_eq!(
        items(&examples[5]),
        vec![
            Item::Link(
                "http://www.google.com/search?q=commonmark&hl=en".into(),
                "www.google.com/search?q=commonmark&hl=en".into()
            ),
            Item::Link(
                "http://www.google.com/search?q=commonmark".into(),
                "www.google.com/search?q=commonmark".into()
            ),
            Item::Text("&hl;".into()),
        ]
    );
    // Example 627: `<lp` stays text after the link.
    assert_eq!(
        items(&examples[6]),
        vec![
            Item::Link(
                "http://www.commonmark.org/he".into(),
                "www.commonmark.org/he".into()
            ),
            Item::Text("<lp".into()),
        ]
    );
}

#[test]
fn the_parsers_own_email_autolinks_carry_mailto() {
    // `<a@b.c>` is the parser's own email autolink; `events` gives every `LinkType::Email`
    // link the `mailto:` prefix, the scanner's and the parser's alike.
    let md = "a <a@b.c> b";
    let got = links(md);
    let want = vec![pair("mailto:a@b.c", "a@b.c")];
    assert!(got == want, "{md:?}: got {got:?} expected {want:?}");
    let types: Vec<LinkType> = events(md, GFM)
        .filter_map(|(e, _)| match e {
            Event::Start(Tag::Link { link_type, .. }) => Some(link_type),
            _ => None,
        })
        .collect();
    assert_eq!(types, vec![LinkType::Email]);
    // One that already has it is left alone.
    assert_eq!(
        links("<mailto:a@b.c>"),
        vec![pair("mailto:a@b.c", "mailto:a@b.c")]
    );
}

#[test]
fn a_run_start_is_delimited_only_after_a_block_start_a_break_or_emphasis() {
    // A text run starts where the previous inline event ended, which is not a line start.
    // The byte before the run decides: after a code span, inline HTML, a footnote reference
    // or the end of a link or image, `www.` is not delimited; after a block start, a line
    // break, or an emphasis or strikethrough delimiter it is.
    let one = vec![pair("http://www.a.com", "www.a.com")];
    for md in [
        "<b>www.a.com</b>",
        "`x`www.a.com",
        "[a](b)www.a.com",
        "![i](u)www.a.com",
        "foo<br>www.a.com",
        "a[^1]www.a.com\n\n[^1]: n\n",
    ] {
        let got = links(md);
        let autolinks: Vec<_> = got
            .iter()
            .filter(|(d, _)| d.starts_with("http://www."))
            .collect();
        assert!(
            autolinks.is_empty(),
            "{md:?}: got {got:?} expected no autolink"
        );
    }
    for md in [
        "*x*www.a.com",
        "~~x~~www.a.com",
        "**x**www.a.com",
        "x\\\nwww.a.com",
        "x  \nwww.a.com",
    ] {
        let got = links(md);
        assert!(got == one, "{md:?}: got {got:?} expected one link");
    }
    // Superscript's `^` and subscript's `~` (options Rubrica does not enable) do not
    // delimit a run that follows their End event. The parser never closes either right
    // before a letter (its flanking rules), so here they stay literal text: `^` before
    // `www.` is no delimiter, `~` is one (it is in the spec's list).
    let opts = GFM | Options::ENABLE_SUPERSCRIPT | Options::ENABLE_SUBSCRIPT;
    let dests = |md: &str| -> Vec<String> {
        events(md, opts)
            .filter_map(|(e, _)| match e {
                Event::Start(Tag::Link { dest_url, .. }) => Some(dest_url.to_string()),
                _ => None,
            })
            .collect()
    };
    assert!(dests("x^a^www.a.com").is_empty());
    assert_eq!(dests("x~a~www.a.com"), vec!["http://www.a.com".to_string()]);
}

#[test]
fn the_scan_sees_source_bytes_not_decoded_text() {
    // The rules read the run's source bytes: an entity is its `&name;` spelling, not the
    // decoded character, so the `&name;` exclusion, the terminators and the delimiters see
    // what the author wrote. The link's text and destination are the decoded output.
    let t = |s: &str| Item::Text(s.into());
    let l = |d: &str, s: &str| Item::Link(d.into(), s.into());
    let rows: Vec<(&str, Vec<Item>)> = vec![
        // `&amp;` at the end is an entity: excluded, and left as text.
        (
            "www.a.com/&amp;",
            vec![l("http://www.a.com/", "www.a.com/"), t("&")],
        ),
        // `&nbsp;` is an entity too; decoded it would be a non-ASCII byte in the path.
        (
            "www.a.com&nbsp;",
            vec![l("http://www.a.com", "www.a.com"), t("\u{a0}")],
        ),
        // `&lt;` is not a `<` byte: the path runs through it; text and href are decoded.
        (
            "www.a.com/x&lt;y",
            vec![l("http://www.a.com/x<y", "www.a.com/x<y")],
        ),
        // `&#42;` decodes to `*`, a delimiter; the source byte before `www.` is `;`.
        ("&#42;www.a.com", vec![t("*www.a.com")]),
        // A numeric reference at the end is an entity too: dropped whole, never split.
        (
            "www.a.com&#46;",
            vec![l("http://www.a.com", "www.a.com"), t(".")],
        ),
        (
            "(www.a.com&#41;",
            vec![t("("), l("http://www.a.com", "www.a.com"), t(")")],
        ),
        // An entity that decodes to whitespace does not end the link: the rules see `&`,
        // and the destination then holds the character.
        (
            "www.a.com/x&#32;y",
            vec![l("http://www.a.com/x y", "www.a.com/x y")],
        ),
    ];
    for (md, expected) in rows {
        let got = items(md);
        assert!(got == expected, "{md:?}: got {got:?} expected {expected:?}");
    }
    // The link's range covers the entity's source bytes it spans.
    let md = "www.a.com/x&lt;y";
    let got = link_ranges(md);
    let want = vec![("www.a.com/x<y".to_string(), 0..16)];
    assert!(got == want, "{md:?}: got {got:?} expected {want:?}");
    let md = "www.a.com/&amp;";
    let got = link_ranges(md);
    let want = vec![("www.a.com/".to_string(), 0..10)];
    assert!(got == want, "{md:?}: got {got:?} expected {want:?}");
}

#[test]
fn nested_candidates_follow_rubricas_rule() {
    let t = |s: &str| Item::Text(s.into());
    let l = |d: &str, s: &str| Item::Link(d.into(), s.into());
    // The fourteen rows of the package's table, in order.
    let rows: Vec<(&str, Vec<Item>)> = vec![
        ("www.foo/http://bar.com", vec![t("www.foo/http://bar.com")]),
        ("http://@b.com", vec![t("http://@b.com")]),
        ("mailto:www.a.com", vec![t("mailto:www.a.com")]),
        (
            "foo@bar.baz@qux.com",
            vec![l("mailto:foo@bar.baz", "foo@bar.baz"), t("@qux.com")],
        ),
        (
            "(www.a.com)",
            vec![t("("), l("http://www.a.com", "www.a.com"), t(")")],
        ),
        ("foowww.a.com", vec![t("foowww.a.com")]),
        ("www.a_b.c", vec![t("www.a_b.c")]),
        ("www.a_b.c.d", vec![l("http://www.a_b.c.d", "www.a_b.c.d")]),
        ("http://a", vec![l("http://a", "http://a")]),
        ("www.a", vec![t("www.a")]),
        ("a@b.c1", vec![l("mailto:a@b.c1", "a@b.c1")]),
        ("www.a.com'", vec![l("http://www.a.com'", "www.a.com'")]),
        ("www.a.com\"", vec![l("http://www.a.com\"", "www.a.com\"")]),
        ("[www.a.com]", vec![t("[www.a.com]")]),
        ("1http://x", vec![t("1http://x")]),
        // Review rows. Decided: no empty domain segment anywhere.
        ("www.a..", vec![t("www.a..")]),
        // A trailing `;` that is not an entity's is stripped, as cmark-gfm strips it.
        (
            "www.a.com;",
            vec![l("http://www.a.com", "www.a.com"), t(";")],
        ),
        // A failed `www.` candidate's bytes may be an email's local part, as in cmark-gfm.
        (
            "www.afoo@b.c",
            vec![l("mailto:www.afoo@b.c", "www.afoo@b.c")],
        ),
        // Matched to cmark-gfm: `www.` is lower-case; a URL's first host byte is
        // alphanumeric; an email domain's `.` counts only when an alphanumeric follows.
        ("WWW.a.com", vec![t("WWW.a.com")]),
        ("http://-", vec![t("http://-")]),
        ("-@o.-h", vec![t("-@o.-h")]),
        // Divergences from cmark-gfm kept and documented (docs/spec/autolinks.md).
        ("+@c.f@", vec![l("mailto:+@c.f", "+@c.f"), t("@")]),
        ("a@b@c.com", vec![t("a@b@c.com")]),
        ("www.a.", vec![t("www.a.")]),
        ("www.a_b.c.", vec![t("www.a_b.c.")]),
        ("x@.p", vec![t("x@.p")]),
        ("_www.a.com_", vec![l("http://www.a.com", "www.a.com")]),
        // `]` is a path byte and not in the trailing set: the link keeps it, as in cmark-gfm.
        (
            "[ www.a.com]",
            vec![t("[ "), l("http://www.a.com]", "www.a.com]")],
        ),
        (
            "[x www.a.com",
            vec![t("[x "), l("http://www.a.com", "www.a.com")],
        ),
        (
            "x [a@b.c]",
            vec![t("x ["), l("mailto:a@b.c", "a@b.c"), t("]")],
        ),
        (
            "mailto:a@b.c",
            vec![t("mailto:"), l("mailto:a@b.c", "a@b.c")],
        ),
        // The scan reads source bytes for emails too; the domain's bytes are ASCII.
        ("a&#64;b.c", vec![t("a@b.c")]),
        ("http://ünïcode.example", vec![t("http://ünïcode.example")]),
        // Escapes keep their backslash in the source view: `\@` is no `@` after a local
        // byte, and `www\.` is not `www.`.
        ("a\\@b.c", vec![t("a@b.c")]),
        ("www\\.a.com", vec![t("www.a.com")]),
    ];
    // `www.a.com'` and `www.a.com"` are one row of the package's table, so its fourteen rows
    // are fifteen inputs; the reviews added twenty.
    assert_eq!(rows.len(), 35);
    for (md, expected) in rows {
        let got = items(md);
        assert!(got == expected, "{md:?}: got {got:?} expected {expected:?}");
    }
}

#[test]
fn text_inside_links_images_and_code_is_not_scanned() {
    let fenced = "```\nwww.a.com\nfoo@bar.baz\n```\n";
    let indented = "    www.a.com\n    foo@bar.baz\n";
    let docs = [
        "[www.a.com](/x)",
        "![www.a.com](/i.png)",
        "`www.a.com`",
        fenced,
        indented,
        "<div>\nwww.a.com foo@bar.baz\n</div>\n",
    ];
    for md in docs {
        for (e, _) in events(md, GFM) {
            if let Event::Start(Tag::Link { link_type, .. } | Tag::Image { link_type, .. }) = &e {
                assert!(
                    !matches!(link_type, LinkType::Autolink | LinkType::Email),
                    "{md:?}: {e:?}"
                );
            }
        }
    }
    // The inline link and the image keep their one destination.
    assert_eq!(links("[www.a.com](/x)"), vec![pair("/x", "www.a.com")]);
    assert_eq!(links("![www.a.com](/i.png)"), vec![]);
    // A code span is one `Code` event, no `Text`.
    assert_eq!(text("`www.a.com`"), "");
    // A code block's content comes out as one `Text`, unscanned.
    for md in [fenced, indented] {
        let texts: Vec<String> = events(md, GFM)
            .filter_map(|(e, _)| match e {
                Event::Text(t) => Some(t.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(
            texts,
            vec!["www.a.com\nfoo@bar.baz\n".to_string()],
            "{md:?}"
        );
    }
}

#[test]
fn delimiters_before_a_candidate() {
    let one = vec![pair("http://www.a.com", "www.a.com")];
    // After an `x`, so a leading tab is not an indented code block and a leading space is
    // not stripped: the byte before `www.` is the delimiter under test.
    for d in [" ", "\t", "*", "_", "~", "("] {
        let md = format!("x{d}www.a.com");
        let got = links(&md);
        assert!(got == one, "{md:?}: got {got:?} expected one link");
    }
    for d in ["a", "1", ".", "/", ":"] {
        let md = format!("{d}www.a.com");
        let got = links(&md);
        assert!(got.is_empty(), "{md:?}: got {got:?} expected text");
    }
    // At the run start, and after a soft break (a new run).
    for md in ["www.a.com", "x\nwww.a.com"] {
        let got = links(md);
        assert!(got == one, "{md:?}: got {got:?} expected one link");
    }
}

/// The alphabet of the property tests: candidate starts, delimiters, path and trim bytes,
/// `<`, an entity's bytes, line ends and whitespace.
const ALPHABET: &[char] = &[
    'a', '.', 'w', 'W', ':', '/', '@', '(', ')', '<', '&', ';', '+', '-', '_', 'h', 't', 'p', 'f',
    ' ', '\t', '\n', '*',
];

fn docs() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(ALPHABET.to_vec()), 0..200)
        .prop_map(|v| v.into_iter().collect())
}

#[test]
fn ranges_are_exact_in_verbatim_pieces_and_stay_on_one_line() {
    // Plain 1: pieces `ab `@0..3 and `* www.a.com x`@4..17, both verbatim.
    let md = "ab \\* www.a.com x";
    let got = link_ranges(md);
    let want = vec![("www.a.com".to_string(), 6..15)];
    assert!(got == want, "{md:?}: got {got:?} expected {want:?}");
    assert_eq!(&md[6..15], "www.a.com");
    // Plain 2: pieces `a`@0..1 and `*www.a.com`@2..12; `w` is preceded by `*`.
    let md = "a\\*www.a.com";
    let got = link_ranges(md);
    let want = vec![("www.a.com".to_string(), 3..12)];
    assert!(got == want, "{md:?}: got {got:?} expected {want:?}");
    assert_eq!(&md[3..12], "www.a.com");

    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig::with_cases(1000));
    runner
        .run(&docs(), |src| {
            for (t, r) in link_ranges(&src) {
                prop_assert!(
                    r.start <= r.end && r.end <= src.len(),
                    "{src:?}: range {r:?} out of bounds"
                );
                prop_assert!(src.is_char_boundary(r.start) && src.is_char_boundary(r.end));
                let bytes = &src[r.clone()];
                prop_assert!(!bytes.contains('\n'), "{src:?}: {r:?} spans a line end");
                // Only an entity makes a piece non-verbatim in this alphabet.
                if !bytes.contains('&') {
                    prop_assert_eq!(bytes, t.as_str(), "{:?}: link {:?} at {:?}", src, t, r);
                }
            }
            Ok(())
        })
        .unwrap();
}

fn upstream_options() -> pulldown_cmark::Options {
    use pulldown_cmark::Options;
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
}

#[test]
fn every_byte_is_emitted_once_and_ranges_are_monotone() {
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig::with_cases(2000));
    runner
        .run(&docs(), |src| {
            let ours: Vec<(Event, Range<usize>)> = events(&src, GFM).collect();
            let ours_text: String = ours
                .iter()
                .filter_map(|(e, _)| match e {
                    Event::Text(t) => Some(t.as_ref()),
                    _ => None,
                })
                .collect();
            let theirs_text: String = pulldown_cmark::Parser::new_ext(&src, upstream_options())
                .filter_map(|e| match e {
                    pulldown_cmark::Event::Text(t) => Some(t.to_string()),
                    _ => None,
                })
                .collect();
            prop_assert_eq!(&ours_text, &theirs_text, "{:?}: text differs", src);

            // Within one run: the scanner's events (Text gaps and the Link triples) have
            // non-decreasing start and end. Nothing else in this alphabet makes a Link
            // (no `[`, `!` or `>`), so a maximal sequence of such events is one run.
            let mut prev: Option<Range<usize>> = None;
            let mut link_text: Option<String> = None;
            for (e, r) in &ours {
                prop_assert!(r.start <= r.end, "{:?}: {:?} has range {:?}", src, e, r);
                let scanner_shaped = matches!(
                    e,
                    Event::Text(_)
                        | Event::Start(Tag::Link {
                            link_type: LinkType::Autolink | LinkType::Email,
                            ..
                        })
                        | Event::End(TagEnd::Link)
                );
                if scanner_shaped {
                    if let Some(p) = &prev {
                        prop_assert!(
                            p.start <= r.start && p.end <= r.end,
                            "{:?}: {:?} at {:?} after {:?}",
                            src,
                            e,
                            r,
                            p
                        );
                    }
                    prev = Some(r.clone());
                } else {
                    prev = None;
                }
                match e {
                    Event::Start(Tag::Link { dest_url, .. }) => {
                        prop_assert!(
                            !dest_url.contains(|c: char| c.is_ascii_whitespace() || c == '<'),
                            "{:?}: dest {:?}",
                            src,
                            dest_url
                        );
                        link_text = Some(String::new());
                    }
                    Event::Text(t) => {
                        if let Some(l) = &mut link_text {
                            l.push_str(t);
                        }
                    }
                    Event::End(TagEnd::Link) => {
                        let t = link_text.take().unwrap();
                        prop_assert!(!t.is_empty(), "{:?}: an empty link text", src);
                    }
                    _ => {}
                }
            }
            Ok(())
        })
        .unwrap();
}

const MB: usize = 1 << 20;
const FLOOR: Duration = Duration::from_millis(50);
#[cfg(not(debug_assertions))]
const CEILING: Duration = Duration::from_secs(2);

/// Timings are the best of three in release; debug builds, whose timing asserts are
/// compiled out, run each once.
const REPEATS: usize = if cfg!(debug_assertions) { 1 } else { 3 };

/// The best of `REPEATS` timings of `f`.
fn best_of<T>(mut f: impl FnMut() -> T) -> (T, Duration) {
    let mut best: Option<(T, Duration)> = None;
    for _ in 0..REPEATS {
        let start = Instant::now();
        let out = f();
        let took = start.elapsed();
        if best.as_ref().is_none_or(|(_, t)| took < *t) {
            best = Some((out, took));
        }
    }
    best.unwrap()
}

/// The floor under which a doubling's smaller run is timer noise: 50 ms, and 100 ms for
/// the 4 and 8 MB re-runs.
#[cfg(not(debug_assertions))]
fn floor_at(n: usize) -> Duration {
    if n >= 2 * MB {
        Duration::from_millis(100)
    } else {
        FLOOR
    }
}

/// The linear-time rule on a shape's `(bytes, time)` series: per doubling `t(2x) <= 2.5 t(x)`
/// unless `t(x)` is under the floor (50 ms; 100 ms for the 4 and 8 MB re-runs); the 2 MB
/// run under 2 s.
#[cfg(not(debug_assertions))]
fn assert_linear(name: &str, series: &[(usize, Duration)]) {
    for w in series.windows(2) {
        let (n1, t1) = w[0];
        let (n2, t2) = w[1];
        if t1 >= floor_at(n1) {
            let ratio = t2.as_secs_f64() / t1.as_secs_f64();
            assert!(
                t2 <= t1.mul_f64(2.5),
                "assertion failed: t2 <= 2.5 * t1 (shape {name} {}→{} MB: ratio {ratio:.1})",
                n1 as f64 / MB as f64,
                n2 as f64 / MB as f64
            );
        }
    }
    for &(n, t) in series {
        if n == 2 * MB {
            assert!(
                t <= CEILING,
                "shape {name} at 2 MB took {:.3} s",
                t.as_secs_f64()
            );
        }
    }
}

#[test]
fn scan_is_linear_on_adversarial_runs() {
    type Shape = (&'static str, fn(usize) -> String);
    let shapes: [Shape; 8] = [
        ("colons", |k| ":".repeat(k)),
        ("at-signs", |k| "@".repeat(k)),
        ("a-at", |k| "a@".repeat(k / 2)),
        ("www-fail", |k| "www.a ".repeat(k / 6)),
        ("parens-open", |k| {
            format!("{}www.a.com", "(((".repeat(k / 3))
        }),
        ("parens-close", |k| format!("www.a.com{}", ")".repeat(k))),
        ("parens-close-short", |k| {
            format!("www.a.b{}", ")".repeat(k))
        }),
        ("entity-tail", |k| {
            format!("{};", "www.a.com&a".repeat(k / 11))
        }),
    ];
    for (name, make) in shapes {
        let mut series = Vec::new();
        let mut sizes = vec![MB / 2, MB, 2 * MB];
        while let Some(size) = sizes.first().copied() {
            sizes.remove(0);
            let run = make(size);
            let n = run.len();
            let ((found, steps), took) = best_of(|| scan_counting(&run, true));
            std::hint::black_box(found);
            eprintln!(
                "scan {name} {n} B {:.3} s {steps} steps",
                took.as_secs_f64()
            );
            assert!(
                steps <= 7 * n + 16,
                "shape {name} at {n} B: {steps} steps > 7n + 16"
            );
            // A shape under the floor at 2 MB is re-run at 4 and 8 MB for one real ratio.
            if size == 2 * MB && took < FLOOR {
                sizes.extend([4 * MB, 8 * MB]);
            }
            series.push((n, took));
        }
        #[cfg(not(debug_assertions))]
        assert_linear(name, &series);
        #[cfg(debug_assertions)]
        let _ = series;
    }
}
