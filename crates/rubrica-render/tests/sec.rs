//! `@sec` markers: parsed from HTML blocks, put on the heading after them as
//! `<span class="sec" id="sec-<id>">`, and reported when they cannot be.

use rubrica_render::markdown::render_strict;
use rubrica_render::{RenderFinding, RenderOptions, Rendered, SecHidden, SecMarker};

fn r(md: &str) -> Rendered {
    render_strict(md, &RenderOptions::wikijs_compat())
}

fn marker(id: &str, tags: &[&str], line: usize) -> SecMarker {
    SecMarker {
        id: id.into(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        line,
    }
}

#[test]
fn a_sec_marker_is_invisible_and_emits_an_anchor_on_its_heading() {
    let out = r("Intro.\n\n<!-- @sec id=ops/deployments/golden-path tags=deploy,procedure -->\n## Golden path\n\nText.\n");
    assert_eq!(
        out.html,
        "<p>Intro.</p>\n<h2 id=\"golden-path\"><span class=\"sec\" id=\"sec-ops/deployments/golden-path\"></span>Golden path</h2>\n<p>Text.</p>\n"
    );
    let m = marker("ops/deployments/golden-path", &["deploy", "procedure"], 3);
    assert_eq!(out.secs, std::slice::from_ref(&m));
    assert_eq!(out.headings[0].sec, Some(m));
    assert_eq!(out.headings[0].line, 4);
    assert_eq!(out.findings, []);
    assert!(!out.html.contains("@sec"), "{}", out.html);
    assert!(!out.plain_text.contains("@sec"), "{}", out.plain_text);
}

#[test]
fn heading_text_and_id_exclude_the_sec_span() {
    let plain = r("## Golden path\n");
    let marked = r("<!-- @sec id=a tags= -->\n## Golden path\n");
    assert_eq!(plain.headings[0].text, marked.headings[0].text);
    assert_eq!(plain.headings[0].id, marked.headings[0].id);
    assert_eq!(marked.secs, [marker("a", &[], 1)]);
    // A dropped id (DOM clobbering) keeps its anchor.
    let out = r("<!-- @sec id=x -->\n## Open\n");
    assert_eq!(out.headings[0].id, None);
    assert_eq!(
        out.html,
        "<h2><span class=\"sec\" id=\"sec-x\"></span>Open</h2>\n"
    );
}

#[test]
fn sec_ids_may_contain_slashes() {
    let out = r("<!-- @sec id=ops/storage/backup-gap tags=a -->\n### Gap\n");
    assert!(
        out.html
            .contains("<span class=\"sec\" id=\"sec-ops/storage/backup-gap\"></span>"),
        "{}",
        out.html
    );
}

#[test]
fn the_marker_rule_is_check_packages_s() {
    // Leading blanks, tabs and blanks inside, no blank before `-->`, an
    // indented heading: still markers on their headings.
    for md in [
        "  <!--@sec id=a -->\n## H\n",
        "<!--\t@sec\tid=a\ttags=x-->\t\n## H\n",
        "<!-- @sec id=a -->\n   ## H\n",
        "<!-- @sec tags=x id=b id=a -->\n## H\n",
    ] {
        let out = r(md);
        assert_eq!(out.findings, [], "{md:?}");
        assert_eq!(
            out.headings[0].sec.as_ref().map(|s| s.id.as_str()),
            Some("a"),
            "{md:?}"
        );
    }
    // Not markers: other comments, `@sec` glued to a word, `>` inside.
    for md in [
        "<!-- sec id=a -->\n## H\n",
        "<!-- @section id=a -->\n## H\n",
        "<!-- @sec id=a> -->\n## H\n",
        "<!-- note --> <!-- @sec id=a -->\n## H\n",
    ] {
        let out = r(md);
        assert_eq!((out.secs.len(), out.findings.len()), (0, 0), "{md:?}");
        assert_eq!(out.headings[0].sec, None, "{md:?}");
        assert!(!out.html.contains("<!--"), "{md:?}: {}", out.html);
    }
}

#[test]
fn a_sec_marker_not_followed_by_a_heading_is_a_finding() {
    for (md, line) in [
        // A blank line between: check-packages's "immediately".
        ("<!-- @sec id=a -->\n\n## H\n", 1),
        // A paragraph, a list (a real page shape).
        ("<!-- @sec id=a -->\ntext\n", 1),
        ("x\n\n<!-- @sec id=a -->\n- **Cleanup**\n", 3),
        // A setext heading: not `^ {0,3}#{1,6}\s`.
        ("<!-- @sec id=a -->\nH\n===\n", 1),
        // The end of the page.
        ("<!-- @sec id=a -->\n", 1),
        // `check-packages` finds the next line after a `\n`: none after a
        // bare `\r` (a CR-only file).
        ("<!-- @sec id=a -->\r## H\r", 1),
        // `#⟨NBSP⟩H` is a heading to `check-packages` (JavaScript's `\s`), a
        // paragraph to CommonMark: stricter, on purpose.
        ("<!-- @sec id=a -->\n#\u{a0}H\n", 1),
    ] {
        let out = r(md);
        assert_eq!(
            out.findings,
            [RenderFinding::SecWithoutHeading {
                id: "a".into(),
                line
            }],
            "{md:?}"
        );
        assert_eq!(out.secs.len(), 1, "{md:?}");
        assert!(out.headings.iter().all(|h| h.sec.is_none()), "{md:?}");
        assert!(!out.html.contains("sec-a"), "{md:?}: {}", out.html);
        assert_eq!(out.findings[0].line(), line);
    }
    // Two markers in a row: the first names no heading.
    let out = r("<!-- @sec id=a -->\n<!-- @sec id=b -->\n## H\n");
    assert_eq!(
        out.findings,
        [RenderFinding::SecWithoutHeading {
            id: "a".into(),
            line: 1
        }]
    );
    assert_eq!(out.headings[0].sec.as_ref().unwrap().id, "b");
}

#[test]
fn a_marker_inside_a_container_is_a_finding_and_gets_no_anchor() {
    // (markdown, line, check-packages sees it): it reads lines, so only a
    // marker with nothing but blanks before it on its line.
    for (md, line, seen) in [
        ("> <!-- @sec id=a -->\n> ## Quoted\n", 1, false),
        ("- <!-- @sec id=a -->\n  ## Listed\n", 1, false),
        ("1. x\n\n   <!-- @sec id=a -->\n   ## Listed\n", 3, true),
        ("- x\n\n  <!-- @sec id=a -->\n\n  text\n", 3, true),
        ("> <!-- @sec id=a -->\n\n## H\n", 1, false),
        (
            "x[^1]\n\n[^1]: y\n\n    <!-- @sec id=a -->\n    ## In a note\n",
            5,
            true,
        ),
    ] {
        let out = r(md);
        assert_eq!(
            out.findings,
            [RenderFinding::SecInContainer {
                id: "a".into(),
                line
            }],
            "{md:?}"
        );
        assert_eq!(out.secs.len(), usize::from(seen), "{md:?}");
        assert!(out.headings.iter().all(|h| h.sec.is_none()), "{md:?}");
        assert!(!out.html.contains("sec-a"), "{md:?}: {}", out.html);
        assert!(!out.html.contains("@sec"), "{md:?}: {}", out.html);
    }
}

#[test]
fn a_marker_on_an_empty_heading_is_a_finding_and_gets_no_anchor() {
    for md in [
        "<!-- @sec id=a -->\n#\n",
        "<!-- @sec id=a -->\n# #\n",
        "<!-- @sec id=a -->\n##   \n",
        "<!-- @sec id=a -->\n### ![](/i.png)\n",
    ] {
        let out = r(md);
        assert_eq!(
            out.findings,
            [RenderFinding::SecOnEmptyHeading {
                id: "a".into(),
                line: 1
            }],
            "{md:?}"
        );
        assert_eq!(out.secs.len(), 1, "{md:?}");
        assert_eq!(out.headings[0].sec, None, "{md:?}");
        assert!(!out.html.contains("sec-a"), "{md:?}: {}", out.html);
    }
}

#[test]
fn a_crlf_marker_lands_and_a_bare_cr_one_does_not() {
    let out = r("<!-- @sec id=a -->\r\n## H\r\n");
    assert_eq!(out.findings, []);
    assert_eq!(
        out.headings[0].sec.as_ref().map(|s| s.id.as_str()),
        Some("a")
    );
    // A bare `\r` elsewhere in the page moves nothing.
    let out = r("x\ry\n\n<!-- @sec id=a -->\n## H\n");
    assert_eq!(out.findings, []);
    assert_eq!(out.headings[0].line, 5);
    assert!(out.headings[0].sec.is_some());
}

#[test]
fn marker_attributes_split_on_javascript_whitespace() {
    // `split(/\s+/)` splits on U+FEFF: `id=` and `a`, so no id.
    let out = r("<!-- @sec id=\u{feff}a -->\n## H\n");
    assert_eq!(out.findings, [RenderFinding::SecWithoutId { line: 1 }]);
    assert_eq!(out.secs, []);
    // …and not on U+0085, which Rust's whitespace has.
    let out = r("<!-- @sec id=a\u{85}b -->\n## H\n");
    assert_eq!(out.findings, []);
    assert_eq!(out.secs, [marker("a\u{85}b", &[], 1)]);
    // NBSP is whitespace to both.
    let out = r("<!-- @sec id=a\u{a0}tags=x -->\n## H\n");
    assert_eq!(out.secs, [marker("a", &["x"], 1)]);
}

#[test]
fn a_marker_in_code_is_not_rendered_and_is_reported() {
    // check-packages.cjs reads lines, so it harvests these; Rubrica's parse
    // reads code. Each is in `secs` (check-packages' set) and reported.
    for (md, line) in [
        ("```\n<!-- @sec id=a -->\n## H\n```\n", 2),
        ("~~~md\n<!-- @sec id=a -->\n~~~\n", 2),
        ("    <!-- @sec id=a -->\n\n## H\n", 1),
        ("\t<!-- @sec id=a -->\n## H\n", 1),
        ("x\n\n    <!-- @sec id=a -->\n", 3),
    ] {
        let out = r(md);
        assert_eq!(
            out.findings,
            [RenderFinding::SecNotRendered {
                id: "a".into(),
                line,
                why: SecHidden::Code
            }],
            "{md:?}"
        );
        assert_eq!(out.secs, [marker("a", &[], line)], "{md:?}");
        assert!(!out.html.contains("class=\"sec\""), "{md:?}: {}", out.html);
        assert!(out.headings.iter().all(|h| h.sec.is_none()), "{md:?}");
    }
    // In code it is shown as written.
    assert!(r("```\n<!-- @sec id=a -->\n```\n")
        .html
        .contains("&lt;!-- @sec id=a --&gt;"));
    // Code spans and prose are no lines of their own: neither harvests them.
    for md in [
        "A `<!-- @sec id=a -->` marker.\n## H\n",
        "See `@sec`, an off-vocabulary tag.\n",
        "x <!-- @sec id=a -->\n## H\n",
    ] {
        let out = r(md);
        assert_eq!((out.secs.len(), out.findings.len()), (0, 0), "{md:?}");
        assert!(!out.html.contains("class=\"sec\""), "{md:?}: {}", out.html);
    }
}

#[test]
fn a_marker_check_packages_harvests_is_rendered_or_reported() {
    for (md, line, why) in [
        // JavaScript's `m` flag ends a line at U+2028 and U+2029; the previous renderer
        // does not, so the marker is in a paragraph.
        ("x\u{2028}<!-- @sec id=a -->\n## H\n", 1, SecHidden::Text),
        ("x\u{2029}<!-- @sec id=a -->\n## H\n", 1, SecHidden::Text),
        // …or the comment goes on, after U+2028, on the previous renderer's line.
        (
            "<!-- @sec id=a -->\u{2028}y\n## H\n",
            1,
            SecHidden::HtmlBlock,
        ),
        // `[^>]*?` crosses line ends: a comment over two lines.
        ("<!-- @sec id=a\n-->\n## H\n", 1, SecHidden::HtmlBlock),
        ("<!-- @sec id=a \n -->\n## H\n", 1, SecHidden::HtmlBlock),
        // Swallowed by the HTML block before it (no blank line between).
        (
            "<div>x</div>\n<!-- @sec id=a -->\n# H\n",
            2,
            SecHidden::HtmlBlock,
        ),
        (
            "<details>\n<!-- @sec id=a -->\n## H\n",
            2,
            SecHidden::HtmlBlock,
        ),
    ] {
        let out = r(md);
        assert_eq!(
            out.findings,
            [RenderFinding::SecNotRendered {
                id: "a".into(),
                line,
                why
            }],
            "{md:?}"
        );
        assert_eq!(out.secs, [marker("a", &[], line)], "{md:?}");
        assert!(!out.html.contains("class=\"sec\""), "{md:?}: {}", out.html);
    }
    // A marker with no id that only check-packages harvests is its error.
    let out = r("```\n<!-- @sec tags=x -->\n```\n");
    assert_eq!(out.findings, [RenderFinding::SecWithoutId { line: 2 }]);
    assert_eq!(out.secs, []);
}

#[test]
fn a_marker_only_rubrica_sees_is_reported_and_gets_no_anchor() {
    // The previous renderer drops a leading byte-order mark; JavaScript keeps it, and `^`
    // then `[ \t]*<!--` does not match before it.
    let out = r("\u{feff}<!-- @sec id=a -->\n## H\n");
    assert_eq!(
        out.findings,
        [RenderFinding::SecNotSeenByCheckPackages {
            id: "a".into(),
            line: 1
        }]
    );
    assert_eq!(out.secs, []);
    assert_eq!(out.headings[0].sec, None);
    assert!(!out.html.contains("class=\"sec\""), "{}", out.html);
    // Elsewhere a byte-order mark is text to both: no marker at all.
    let out = r("x\n\n\u{feff}<!-- @sec id=a -->\n## H\n");
    assert_eq!((out.secs.len(), out.findings.len()), (0, 0));
}

#[test]
fn a_marker_on_an_invisible_heading_is_on_an_empty_one() {
    for c in [
        '\u{200b}', '\u{200c}', '\u{200d}', '\u{2060}', '\u{feff}', '\u{ad}',
    ] {
        let md = format!("<!-- @sec id=a -->\n# {c}\n");
        let out = r(&md);
        assert_eq!(
            out.findings,
            [
                RenderFinding::SecOnEmptyHeading {
                    id: "a".into(),
                    line: 1
                },
                RenderFinding::InvisibleHeading { line: 2 },
            ],
            "{md:?}"
        );
        assert_eq!(out.headings[0].sec, None, "{md:?}");
        // Without a marker, the heading is still reported.
        let out = r(&format!("# {c}{c}\n\n## Seen\n"));
        assert_eq!(out.findings, [RenderFinding::InvisibleHeading { line: 1 }]);
    }
    // Visible text with an invisible character in it is a heading.
    let out = r("<!-- @sec id=a -->\n# a\u{200b}b\n");
    assert_eq!(out.findings, []);
    assert!(out.headings[0].sec.is_some());
}

#[test]
fn a_marker_without_an_id_is_a_finding() {
    for md in ["<!-- @sec tags=a -->\n## H\n", "<!-- @sec id= -->\n## H\n"] {
        let out = r(md);
        assert_eq!(
            out.findings,
            [RenderFinding::SecWithoutId { line: 1 }],
            "{md:?}"
        );
        assert_eq!(out.secs, [], "{md:?}");
        assert_eq!(out.headings[0].sec, None);
    }
}

#[test]
fn a_duplicate_sec_id_is_a_finding_and_gets_one_anchor() {
    let out = r("<!-- @sec id=a -->\n## One\n\n<!-- @sec id=a -->\n## Two\n");
    assert_eq!(
        out.findings,
        [RenderFinding::DuplicateSecId {
            id: "a".into(),
            line: 4,
            first: 1
        }]
    );
    assert_eq!(out.html.matches("id=\"sec-a\"").count(), 1, "{}", out.html);
    assert!(out.headings[0].sec.is_some() && out.headings[1].sec.is_none());
    assert_eq!(out.secs.len(), 2);
}

#[test]
fn a_sec_anchor_that_is_a_heading_id_is_a_finding() {
    // `## Sec a` has the id `sec-a`, the anchor `id=a` would have.
    let out = r("## Sec a\n\n<!-- @sec id=a -->\n## Other\n");
    assert_eq!(
        out.findings,
        [RenderFinding::SecAnchorIsAHeadingId {
            id: "a".into(),
            line: 3
        }]
    );
    assert_eq!(out.html.matches("id=\"sec-a\"").count(), 1, "{}", out.html);
    assert_eq!(out.headings[1].sec, None);
}

#[test]
fn a_sec_id_is_escaped_in_its_attribute() {
    let out = r("<!-- @sec id=a\"b&c -->\n## H\n");
    assert!(
        out.html
            .contains("<span class=\"sec\" id=\"sec-a&quot;b&amp;c\"></span>"),
        "{}",
        out.html
    );
}

fn page(src: &str) -> (Rendered, usize) {
    let (_, out, offset) = rubrica_render::render_page(src, &RenderOptions::wikijs_compat())
        .unwrap_or_else(|e| panic!("{src:?}: {e:?}"));
    (out, offset)
}

#[test]
fn a_marker_in_front_matter_is_reported_at_its_line_in_the_file() {
    // check-packages.cjs harvests the whole file, front matter included;
    // Rubrica's markers are the body's. A marker in a block scalar is
    // check-packages' and not Rubrica's: a finding at its file line.
    let (out, offset) = page(
        "---\ntitle: T\ndescription: |\n  <!-- @sec id=fm -->\n---\n<!-- @sec id=b -->\n# H\n",
    );
    assert_eq!(offset, 5);
    assert_eq!(
        out.findings,
        [RenderFinding::SecInFrontMatter {
            id: Some("fm".into()),
            line: 4
        }]
    );
    assert_eq!(out.secs, [marker("b", &[], 1)]);
    assert_eq!(out.headings[0].sec, Some(marker("b", &[], 1)));
    // One with no id is reported too.
    let (out, _) = page("---\ntitle: T\nnote: |\n  <!-- @sec tags=x -->\n---\n# H\n");
    assert_eq!(
        out.findings,
        [RenderFinding::SecInFrontMatter { id: None, line: 4 }]
    );
    assert_eq!(out.secs, []);
    // With none in the front matter, a page is its body's render.
    let body = "<!-- @sec id=b -->\n# H\n";
    let (out, offset) = page(&format!("---\ntitle: T\n---\n{body}"));
    assert_eq!(
        out,
        rubrica_render::render(body, &RenderOptions::wikijs_compat())
    );
    assert_eq!(offset, 3);
}

#[test]
fn an_unclosed_marker_in_front_matter_takes_the_body_s_marker() {
    // The regex's group crosses line ends: `<!-- @sec id=fm` in the front
    // matter runs to the body marker's `-->`, and check-packages reads one
    // marker at line 4 whose last `id=` is the body's (node, the vendored
    // harvest: `[[4,"b",[]]]`). The body marker is no match of its own, so
    // it gets no anchor and is not in `secs`.
    let (out, _) =
        page("---\ntitle: T\nnote: |\n  <!-- @sec id=fm\n---\n<!-- @sec id=b -->\n# H\n");
    assert_eq!(
        out.findings,
        [
            RenderFinding::SecInFrontMatter {
                id: Some("b".into()),
                line: 4
            },
            RenderFinding::SecNotSeenByCheckPackages {
                id: "b".into(),
                line: 1
            },
        ]
    );
    assert_eq!(out.secs, []);
    assert_eq!(out.headings[0].sec, None);
    assert!(!out.html.contains("class=\"sec\""), "{}", out.html);
    // A body marker with no id leaves the front matter's (`[[4,"fm",["x"]]]`).
    let (out, _) =
        page("---\ntitle: T\nnote: |\n  <!-- @sec id=fm\n---\n<!-- @sec tags=x -->\n# H\n");
    assert_eq!(
        out.findings,
        [
            RenderFinding::SecInFrontMatter {
                id: Some("fm".into()),
                line: 4
            },
            RenderFinding::SecWithoutId { line: 1 },
        ]
    );
    assert_eq!(out.secs, []);
}
