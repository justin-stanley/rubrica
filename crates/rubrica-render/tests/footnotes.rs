//! Footnotes: their ids against the headings', every reference's link
//! reaching an id, and back references bounded.

use rubrica_render::markdown::{render_strict, MAX_FOOTNOTE_BACKREFS};
use rubrica_render::{RenderFinding, RenderOptions, Rendered};

fn r(md: &str) -> Rendered {
    render_strict(md, &RenderOptions::wikijs_compat())
}

/// The values of `key="…"` in sanitized output.
fn values<'h>(html: &'h str, key: &str) -> Vec<&'h str> {
    let key = format!(" {key}=\"");
    html.match_indices(&key)
        .map(|(at, _)| {
            let rest = &html[at + key.len()..];
            &rest[..rest.find('"').unwrap()]
        })
        .collect()
}

#[test]
fn a_heading_id_that_is_a_footnote_id_is_a_finding() {
    for (md, id, line) in [
        ("# fn-1\n\nx[^1]\n\n[^1]: y\n", "fn-1", 1),
        ("x[^1]\n\n## fnref-1\n\n[^1]: y\n", "fnref-1", 3),
        // The second reference's id (`fnref-N-K`, numbered).
        ("x[^a] y[^a]\n\n### fnref-1-2\n\n[^a]: z\n", "fnref-1-2", 3),
    ] {
        let out = r(md);
        assert_eq!(
            out.findings,
            [RenderFinding::HeadingIdIsAFootnoteId {
                id: id.into(),
                line
            }],
            "{md:?}: {}",
            out.html
        );
        assert_eq!(out.findings[0].line(), line);
        assert_eq!(
            values(&out.html, "id").iter().filter(|v| **v == id).count(),
            2,
            "{md:?}: {}",
            out.html
        );
    }
    // No footnote of that name, or one no reference makes: no finding.
    for md in [
        "# fn-2\n\nx[^1]\n\n[^1]: y\n",
        "# fn-1\n\n[^1]: y\n",
        "# fn-1\n",
    ] {
        assert_eq!(r(md).findings, [], "{md:?}");
    }
}

#[test]
fn every_footnote_link_reaches_an_id() {
    let deep = |n: usize| "> ".repeat(n);
    for md in [
        "x[^1]\n\n[^1]: y\n".to_owned(),
        // A definition deep in containers: the previous renderer moves it to the end.
        format!("x[^1]\n\n{}[^1]: y\n", deep(50)),
        format!("{}x[^1]\n\n[^1]: y\n", deep(150)),
        format!("x[^1]\n\n[^1]: {}y\n", deep(150)),
        format!("{}x[^1]\n\n{}[^1]: y\n", "- ".repeat(5), deep(98)),
        // Nested and duplicate definitions, and a reference in a note.
        "x[^1] y[^2]\n\n[^1]: [^2]: b x[^2]\n".to_owned(),
        "x[^1]\n\n[^1]: a\n\n[^1]: b\n".to_owned(),
        format!("{}[^1]: a\n\nx[^1]\n\n[^1]: b\n", deep(120)),
        "x[^1]\n\n[^1]: a[^2]\n\n[^2]: b[^1]\n".to_owned(),
        format!(
            "x[^z]\n\n{}y\n",
            (0..120).map(|i| format!("[^n{i}]: ")).collect::<String>()
        ),
    ] {
        let out = r(&md);
        let ids = values(&out.html, "id");
        for href in values(&out.html, "href") {
            let Some(target) = href.strip_prefix('#') else {
                continue;
            };
            assert!(
                ids.contains(&target),
                "{:?}…: #{target} has no id",
                &md[..md.len().min(60)]
            );
        }
    }
}

#[test]
fn back_references_are_bounded() {
    let n = 1_000;
    let out = r(&format!("{}\n\n[^1]: x\n", "[^1]".repeat(n)));
    let backrefs = out.html.matches("data-footnote-backref=").count();
    assert_eq!(backrefs, MAX_FOOTNOTE_BACKREFS as usize);
    // Every reference is still written, with its id.
    assert_eq!(out.html.matches("data-footnote-ref=").count(), n);
    assert_eq!(
        out.findings,
        [RenderFinding::FootnoteBackrefsBounded {
            name: "1".into(),
            references: n,
            line: 3
        }]
    );
    // At the bound, no finding.
    let at = MAX_FOOTNOTE_BACKREFS as usize;
    let out = r(&format!("{}\n\n[^1]: x\n", "[^1]".repeat(at)));
    assert_eq!(out.findings, []);
    assert_eq!(out.html.matches("data-footnote-backref=").count(), at);
}

#[test]
fn back_references_keep_the_output_linear() {
    // 400 KB of `[^1]` was 28.7 MB of HTML (the previous renderer's one back reference per
    // reference); about 100 bytes per reference now.
    let n = 100_000;
    let md = format!("{}\n\n[^1]: x\n", "[^1]".repeat(n));
    let out = r(&md);
    assert!(
        out.html.len() < 130 * n,
        "{} bytes of HTML from {} of Markdown",
        out.html.len(),
        md.len()
    );
}
