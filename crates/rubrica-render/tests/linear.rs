//! Shapes that cost html5ever (pass 1's tokenizer, ammonia's tree builder
//! and serializer) or the previous renderer more than linear time, and the bounds that
//! keep a render linear. Each case is about 1 MB, which took seconds before
//! its bound, or less where 1 MB of a linear shape is slow and the shape's
//! finding is the test. The time allowed (2 s, unless a case says
//! otherwise) is far below a quadratic render's, and at least four times a
//! linear one's on a loaded runner. One measured estimate of one is this
//! machine with four suites running at once, about three times as slow as
//! one suite: no case took more than a quarter of its limit (html5ever and
//! the previous renderer optimised in tests as in release). Nesting depth is `deep_nesting.rs`'s and `sanitize.rs`'s.

use std::time::{Duration, Instant};

use rubrica_render::markdown::MAX_PAGE_BYTES;
use rubrica_render::{render, RenderFinding, RenderOptions, Rendered};

const MB: usize = 1 << 20;

fn timed(name: &str, md: &str) -> Rendered {
    timed_within(name, md, limits().ceiling)
}

fn timed_within(name: &str, md: &str, limit: Duration) -> Rendered {
    let start = Instant::now();
    let out = render(md, &RenderOptions::default());
    let took = start.elapsed();
    eprintln!("{name}: {} bytes in {took:?} (of {limit:?})", md.len());
    // A debug or coverage build only runs the shape; the release `linear` job holds the time.
    if cfg!(not(debug_assertions)) {
        assert!(took < limit, "{name}: {} bytes took {took:?}", md.len());
    }
    out
}

fn attrs(n: usize) -> String {
    (0..n).map(|i| format!(" a{i}")).collect()
}

#[test]
fn a_tag_with_240k_attributes_is_escaped_not_tokenized() {
    // 160,000 (1.17 MB) took 3.6 s in release through the tokenizer's
    // duplicate check; 240,000 (1.8 MB), about 8 s. Linear, at most about
    // 0.6 s loaded: 4 s allowed.
    let four = Duration::from_secs(4);
    let out = timed_within("block", &format!("<div{}>\n", attrs(240_000)), four);
    assert!(out.html.starts_with("&lt;div a0 a1"), "{}", &out.html[..40]);
    assert_eq!(
        out.findings,
        [RenderFinding::RawHtmlTooManyAttributes { line: 1 }]
    );
    let out = timed_within("inline", &format!("x <sub{}>\n", attrs(240_000)), four);
    assert!(
        out.html.starts_with("<p>x &lt;sub a0"),
        "{}",
        &out.html[..40]
    );
}

#[test]
fn tags_at_the_attribute_bound_stay_linear() {
    // 3,500 tags of 256 attributes each, 4.1 MB in one piece (under
    // MAX_PAGE_BYTES): about 0.23 s, 1.1 s loaded, so 8 s allowed.
    let tags: String = (0..3_500).map(|_| format!("<p{}>", attrs(256))).collect();
    let out = timed_within(
        "bounded tags",
        &format!("<div>\n{tags}\n"),
        Duration::from_secs(8),
    );
    assert_eq!(out.findings, []);
}

#[test]
fn other_raw_shapes_are_linear() {
    let x = "x".repeat(MB);
    for (name, md) in [
        ("long quoted value", format!("<div a=\"{x}\">\n")),
        ("long unquoted value", format!("<div a={x}>\n")),
        ("long attribute name", format!("<div {x}>\n")),
        ("long tag name", format!("<d{x}>\n")),
        ("many tags", format!("<div>\n{}\n", "<sub>".repeat(MB / 5))),
        (
            "many end tags",
            format!("<div>\n{}\n", "</sub>".repeat(MB / 6)),
        ),
        ("long comment", format!("<!-- {x} -->\n")),
        (
            "dashes in a comment",
            format!("<!-- {} -->\n", "-".repeat(MB)),
        ),
        (
            "nested comment openers",
            format!("<!-- {} -->\n", "<!-".repeat(MB / 3)),
        ),
        ("bogus comment", format!("<?{x}>\n")),
        ("doctype", format!("<!DOCTYPE {x}>\n")),
        ("references", format!("<div>\n{}\n", "&amp;".repeat(MB / 5))),
    ] {
        timed(name, &md);
    }
    // About 0.19 s: 4 s allowed.
    let md = format!("<div>\n{}\n", "&#x".repeat(MB / 3));
    timed_within("cut-off references", &md, Duration::from_secs(4));
}

#[test]
fn text_and_attributes_full_of_ampersands_are_linear() {
    // The serializer scanned from each `&` to the text's end: 1 MB of `&`
    // took 15 s in release.
    // 0.4 s linear (1.5 s loaded) and 15 s quadratic at 1 MB: 8 s allowed.
    let amp = "&".repeat(MB);
    let eight = Duration::from_secs(8);
    let out = timed_within("text of &", &format!("{amp}\n"), eight);
    assert_eq!(out.html.len(), 5 * MB + 8);
    timed("text of °", &format!("{}\n", "°".repeat(MB / 2)));
    timed("text of NBSP", &format!("{}\n", "\u{a0}".repeat(MB / 2)));
    timed_within("code of &", &format!("```\n{amp}\n```\n"), eight);
    // An attribute value cannot be split: one that costly is left out, and
    // the finding names it and the line that wrote it.
    let quarter = "&".repeat(MB / 4);
    for (name, md, element, attribute, line) in [
        (
            "title",
            format!("x\n\n[x](/p \"{quarter}\")\n"),
            "a",
            "title",
            3,
        ),
        (
            "href",
            format!("x\n\n> [x](/p?{quarter})\n"),
            "a",
            "href",
            3,
        ),
        (
            "alt",
            format!("# H\n![{quarter}](/i.png)\n"),
            "img",
            "alt",
            2,
        ),
        (
            "img title",
            format!("- a\n- ![i](/i.png \"{quarter}\")\n"),
            "img",
            "title",
            2,
        ),
        (
            "code class",
            format!("x\n\n```{quarter}\nx\n```\n"),
            "code",
            "class",
            3,
        ),
    ] {
        let out = timed(name, &md);
        assert_eq!(
            out.findings,
            [RenderFinding::AttributesTooCostly {
                element: element.into(),
                attribute: attribute.into(),
                line
            }],
            "{name}"
        );
    }
    // An `@sec` anchor too costly to write is not written: the heading has
    // no `sec`, the id is still check-packages' (in `secs`).
    let out = timed("sec id", &format!("x\n\n<!-- @sec id={quarter} -->\n# H\n"));
    assert_eq!(
        out.findings,
        [RenderFinding::AttributesTooCostly {
            element: "span".into(),
            attribute: "id".into(),
            line: 3
        }]
    );
    assert_eq!(out.headings[0].sec, None);
    assert_eq!(out.secs.len(), 1);
    assert!(!out.html.contains("class=\"sec\""), "{}", &out.html[..60]);
}

#[test]
fn a_tag_behind_a_comment_end_bang_dash_is_counted() {
    // `--!->` closes no comment (WHATWG: `--!` then `-` is comment end
    // dash), so the `<sub>` after the `-->` in the title is a tag of 160,000
    // attributes: 3.8 s through the tokenizer when the scan took `--!->`
    // for the comment's end and counted one attribute.
    for n in [160_000, 320_000] {
        let md = format!("<!-- --!-> <p title=\"--><sub{}>z\" >\n", attrs(n));
        let out = timed("comment end bang dash", &md);
        assert_eq!(
            out.findings,
            [RenderFinding::RawHtmlTooManyAttributes { line: 1 }]
        );
        assert!(
            out.html.starts_with("&lt;!-- --!-&gt;"),
            "{}",
            &out.html[..40]
        );
    }
}

#[test]
fn a_page_over_the_size_cap_is_refused_not_rendered() {
    let at = "word ".repeat(MAX_PAGE_BYTES / 5);
    let at = format!("{at}{}", "x".repeat(MAX_PAGE_BYTES - at.len()));
    assert_eq!(at.len(), MAX_PAGE_BYTES);
    // Linear: 0.15 s, 0.7 s loaded; 4 s allowed.
    let out = timed_within("at the cap", &at, Duration::from_secs(4));
    assert_eq!(out.findings, []);
    assert!(out.html.starts_with("<p>word word"));
    let over = format!("# Title\n\n{at}");
    let out = timed("over the cap", &over);
    assert_eq!(
        out.findings,
        [RenderFinding::PageTooLarge { bytes: over.len() }]
    );
    assert_eq!(out.findings[0].line(), 1);
    assert_eq!(
        out.html,
        format!(
            "<p>This page is not rendered: its body is {} bytes, more than Rubrica's limit of {MAX_PAGE_BYTES}.</p>\n",
            over.len()
        )
    );
    assert!(out.headings.is_empty() && out.links.is_empty() && out.secs.is_empty());
}

// The render-level matrix.

/// The public parser shape `name` at about `size` bytes (copied from rubrica-markdown's
/// `tests/linear.rs`).
fn public_shape(name: &str, size: usize) -> String {
    match name {
        // A long line of `:` with no `//`: cheap trigger check.
        "colons" => ":".repeat(size),
        // cmark's pathological_tests.py shapes.
        "nested-brackets" => format!("{}a{}", "[".repeat(size / 2), "]".repeat(size / 2)),
        "nested-strong-emph" => format!(
            "{}a{}",
            "*a **a ".repeat(size / 14),
            " a** a*".repeat(size / 14)
        ),
        "many-emph-closers" => "a_ ".repeat(size / 3),
        "many-emph-openers" => " _a".repeat(size / 3),
        "many-link-closers" => "a]".repeat(size / 2),
        "many-link-openers" => "[a".repeat(size / 2),
        "mismatched-openers-closers" => "*a_ ".repeat(size / 4),
        "link-openers-and-emph-closers" => "[ a_".repeat(size / 4),
        "pattern-brackets-emph" => "[a_".repeat(size / 3),
        "hard-link-emph" => "*a [a**".repeat(size / 7),
        "nested-blockquotes" => format!("{}a", "> ".repeat(size / 2)),
        "nested-lists" => format!("{}a", "- ".repeat(size / 2)),
        // `e` + n backticks + space for n up to k: about k²/2 bytes.
        "backticks" => {
            let k = ((2 * size) as f64).sqrt() as usize;
            (1..k).map(|n| format!("e{} ", "`".repeat(n))).collect()
        }
        "unclosed-links-a" => "[a](<b".repeat(size / 6),
        "unclosed-links-b" => "[a](b".repeat(size / 5),
        "unclosed-inline-html" => "<a href=\"".repeat(size / 9),
        "reference-collisions" => format!(
            "{}{}",
            "[a]: /url\n".repeat(size / 10),
            "[a][a]".repeat(size / 6)
        ),
        "many-references" => {
            (0..size / 12)
                .map(|i| format!("[{i}]: /u\n"))
                .collect::<String>()
                + "[0]"
        }
        "entities" => "&".repeat(size),
        "deeply-nested-emph" => format!("{}a{}", "*".repeat(size / 2), "*".repeat(size / 2)),
        "html-comments" => "<!--".repeat(size / 4),
        "code-spans" => "`a".repeat(size / 2),
        "tildes" => "~a".repeat(size / 2),
        "footnotes" => format!(
            "{}{}",
            (0..size / 16)
                .map(|i| format!("[^{i}]: n\n"))
                .collect::<String>(),
            (0..size / 16)
                .map(|i| format!("[^{i}]"))
                .collect::<String>()
        ),
        // GFM tables: cell count (markdown-rs issue 218), one row-pair of about size/6 cells.
        "table-wide" => {
            let n = size / 6;
            format!(
                "|{}\n|{}\n|{}\n",
                "a|".repeat(n),
                "-|".repeat(n),
                "b|".repeat(n)
            )
        }
        "table-tall" => format!("|a|b|\n|-|-|\n{}", "|c|d|\n".repeat(size / 6)),
        "table-escaped-pipes" => format!("|a|\n|-|\n|{}|\n", "\\|".repeat(size / 2)),
        "setext-underlines" => "a\n=\n".repeat(size / 4),
        "atx-empty" => "#\n".repeat(size / 2),
        "task-items" => "- [ ] a\n".repeat(size / 8),
        "plain-text" => "x".repeat(size),
        "paragraph-lines" => "a\n".repeat(size / 2),
        // Rubrica's autolink shapes.
        "at-signs" => "@".repeat(size),
        "emails" => "a@b.c ".repeat(size / 6),
        "www-fail" => "www.a ".repeat(size / 6),
        "parens" => "(www.a.com))))".repeat(size / 14),
        _ => panic!("unknown shape {name}"),
    }
}

/// The 37 public shapes.
const PUBLIC_SHAPES: [&str; 37] = [
    "colons",
    "nested-brackets",
    "nested-strong-emph",
    "many-emph-closers",
    "many-emph-openers",
    "many-link-closers",
    "many-link-openers",
    "mismatched-openers-closers",
    "link-openers-and-emph-closers",
    "pattern-brackets-emph",
    "hard-link-emph",
    "nested-blockquotes",
    "nested-lists",
    "backticks",
    "unclosed-links-a",
    "unclosed-links-b",
    "unclosed-inline-html",
    "reference-collisions",
    "many-references",
    "entities",
    "deeply-nested-emph",
    "html-comments",
    "code-spans",
    "tildes",
    "footnotes",
    "table-wide",
    "table-tall",
    "table-escaped-pipes",
    "setext-underlines",
    "atx-empty",
    "task-items",
    "plain-text",
    "paragraph-lines",
    "at-signs",
    "emails",
    "www-fail",
    "parens",
];

/// Rubrica's own shapes, which only the renderer sees.
const OWN_SHAPES: [&str; 5] = [
    "sec-markers",
    "footnote-back-references",
    "heading-runs",
    "table-rows",
    "raw-html-tags",
];

fn own_shape(name: &str, size: usize) -> String {
    match name {
        // `@sec` markers, each on its own heading.
        "sec-markers" => "<!-- @sec id=a -->\n# h\n".repeat(size / 24),
        // One definition, n references to it: n back-references.
        "footnote-back-references" => format!("[^1]: n\n\n{}", "[^1]".repeat(size / 4)),
        "heading-runs" => "# h\n".repeat(size / 4),
        "table-rows" => format!("|a|b|\n|-|-|\n{}", "|a|b|\n".repeat(size / 6)),
        "raw-html-tags" => "<br>".repeat(size / 4),
        _ => panic!("unknown shape {name}"),
    }
}

const FLOOR: Duration = Duration::from_millis(50);
/// How strictly the linear-time rule is applied. Locally (and in the private gate) a
/// doubling may cost at most 2.5x, timed as the best of three runs. The hosted public CI
/// runners are shared and noisy, so there (`RUBRICA_LINEAR_CI=1`) the best of five runs may
/// cost up to 3.0x per doubling. Quadratic growth shows about 4x either way. The absolute
/// ceiling is a budget for known hardware: 2 s locally, 6 s on hosted CI, whose arm64 runner
/// is about 3.7x slower than the development machine.
struct Limits {
    ratio: f64,
    repeats: usize,
    ceiling: Duration,
}

/// The release-build limits with the hosted-CI switch `ci` on or off.
fn limits_for(ci: bool) -> Limits {
    if ci {
        Limits {
            ratio: 3.0,
            repeats: 5,
            ceiling: Duration::from_secs(6),
        }
    } else {
        Limits {
            ratio: 2.5,
            repeats: 3,
            ceiling: Duration::from_secs(2),
        }
    }
}

/// Whether `RUBRICA_LINEAR_CI=1` selects the hosted-CI limits.
fn hosted_ci() -> bool {
    std::env::var("RUBRICA_LINEAR_CI").is_ok_and(|v| v == "1")
}

/// The limits in force; debug builds, whose timing asserts are compiled out, run each
/// timing once.
fn limits() -> Limits {
    let ci = hosted_ci();
    Limits {
        repeats: if cfg!(debug_assertions) {
            1
        } else {
            limits_for(ci).repeats
        },
        ..limits_for(ci)
    }
}

#[test]
fn the_limits_follow_the_hosted_ci_switch() {
    let local = limits_for(false);
    assert_eq!((local.ratio, local.repeats), (2.5, 3));
    assert_eq!(local.ceiling, Duration::from_secs(2));
    let hosted = limits_for(true);
    assert_eq!((hosted.ratio, hosted.repeats), (3.0, 5));
    assert_eq!(hosted.ceiling, Duration::from_secs(6));
}

/// The best of `limits().repeats` timings of `f`.
fn best_of<T>(mut f: impl FnMut() -> T) -> Duration {
    (0..limits().repeats)
        .map(|_| {
            let start = Instant::now();
            std::hint::black_box(f());
            start.elapsed()
        })
        .min()
        .unwrap()
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

/// The linear-time rule on a `(bytes, time)` series: per doubling `t(2x) <= 2.5 t(x)` (3.0 on hosted CI) unless
/// `t(x)` is under the floor; the 2 MB run under the ceiling (2 s; 6 s on hosted CI).
#[cfg(not(debug_assertions))]
fn check_linear(what: &str, series: &[(usize, Duration)], failures: &mut Vec<String>) {
    for w in series.windows(2) {
        let (n1, t1) = w[0];
        let (n2, t2) = w[1];
        if t1 >= floor_at(n1) {
            let ratio = t2.as_secs_f64() / t1.as_secs_f64();
            if t2 > t1.mul_f64(limits().ratio) {
                failures.push(format!(
                    "assertion failed: t2 <= {} * t1 ({what} {}→{} MB: ratio {ratio:.2})",
                    limits().ratio,
                    n1 as f64 / MB as f64,
                    n2 as f64 / MB as f64
                ));
            }
        }
    }
    for &(n, t) in series {
        if n == 2 * MB {
            if t > limits().ceiling {
                failures.push(format!("{what} at 2 MB took {:.3} s", t.as_secs_f64()));
            }
        }
    }
}

/// Every shape through `render` under both profiles (a characterisation, held
/// against the owned pipeline): the public parser shapes and Rubrica's own, at 0.5,
/// 1 and 2 MB, the linear-time rule, a shape under the floor at 2 MB re-run at 4 and 8 MB. The
/// two shapes that hold `<` are not here; they are in the private conformance suite.
#[test]
fn every_shape_is_linear_through_render_under_both_profiles() {
    #[cfg_attr(debug_assertions, allow(unused_mut))]
    let mut failures: Vec<String> = Vec::new();
    for (profile, opts) in [
        ("native", RenderOptions::native()),
        ("wikijs_compat", RenderOptions::wikijs_compat()),
    ] {
        for name in PUBLIC_SHAPES.iter().chain(&OWN_SHAPES) {
            let mut series = Vec::new();
            let mut sizes = vec![MB / 2, MB, 2 * MB];
            while let Some(size) = sizes.first().copied() {
                sizes.remove(0);
                let md = if PUBLIC_SHAPES.contains(name) {
                    public_shape(name, size)
                } else {
                    own_shape(name, size)
                };
                let took = best_of(|| render(&md, &opts));
                eprintln!(
                    "{profile} {name} {} B {:.3} s",
                    md.len(),
                    took.as_secs_f64()
                );
                if size == 2 * MB && took < FLOOR {
                    sizes.extend([4 * MB, 8 * MB]);
                }
                series.push((size, took));
            }
            #[cfg(not(debug_assertions))]
            check_linear(&format!("{profile} shape {name}"), &series, &mut failures);
            #[cfg(debug_assertions)]
            let _ = series;
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
