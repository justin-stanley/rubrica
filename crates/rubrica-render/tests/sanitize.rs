//! The sanitizer: raw HTML filtered to a short allowlist with no
//! attributes, then everything through ammonia, with Rubrica's own ids kept
//! and dangerous URL schemes removed.

use rubrica_render::markdown::render_strict;
use rubrica_render::sanitize::MAX_RAW_ATTRIBUTES;
use rubrica_render::{RawHtml, RenderFinding, RenderOptions, Rendered};

fn r(md: &str) -> Rendered {
    render_strict(md, &RenderOptions::wikijs_compat())
}

fn html(md: &str) -> String {
    r(md).html
}

/// Every `href` and `src` value in sanitized output (ammonia writes each
/// attribute as `name="value"`), with `&amp;`, `&quot;` and `&lt;`/`&gt;`
/// undone.
fn urls(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    for key in [" href=\"", " src=\""] {
        for (at, _) in html.match_indices(key) {
            let rest = &html[at + key.len()..];
            let value = &rest[..rest.find('"').unwrap()];
            out.push(
                value
                    .replace("&quot;", "\"")
                    .replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&amp;", "&"),
            );
        }
    }
    out
}

/// A URL as a browser reads its scheme: leading C0 controls and spaces
/// stripped, tabs and line ends removed anywhere, lowercased.
fn scheme_view(url: &str) -> String {
    url.trim_start_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect::<String>()
        .to_ascii_lowercase()
}

/// The attribute names of every tag in sanitized output (ammonia writes
/// `<tag name="value" …>`, quotes in values as `&quot;`).
fn attribute_names(html: &str) -> Vec<String> {
    let mut names = Vec::new();
    for tag in html.split('<').skip(1) {
        let tag = &tag[..tag.find('>').unwrap()];
        // Outside quotes only: drop each quoted value.
        let bare: String = tag.split('"').step_by(2).collect::<Vec<_>>().join(" ");
        names.extend(
            bare.split_whitespace()
                .skip(1)
                .map(|a| a.trim_end_matches('=').to_ascii_lowercase()),
        );
    }
    names
}

fn assert_no_dangerous_url(md: &str) {
    let out = html(md);
    for url in urls(&out) {
        let v = scheme_view(&url);
        for bad in ["javascript:", "vbscript:", "data:"] {
            assert!(!v.starts_with(bad), "{md:?}: {url:?} survives in {out}");
        }
    }
    let lower = out.to_ascii_lowercase();
    assert!(
        !lower.contains("javascript:alert") || !lower.contains("href=\"javascript"),
        "{md:?}: {out}"
    );
}

#[test]
fn raw_br_passes_and_script_is_stripped_with_its_content_kept() {
    assert_eq!(html("a<br>b <br/> c\n"), "<p>a<br>b <br> c</p>\n");
    assert_eq!(
        html("a <script>alert(1)</script> b\n"),
        "<p>a alert(1) b</p>\n"
    );
    assert_eq!(html("<script>\nalert(1)\n</script>\n"), "\nalert(1)\n\n");
    // Its text is text: escaped, never markup.
    assert_eq!(
        html("<script>a && c > b</script>\n\nx\n"),
        "a &amp;&amp; c &gt; b\n<p>x</p>\n"
    );
    // The filter tokenizes a piece on its own, so a script's content is not
    // raw text to it: `<b && c</script>` is a `<b>` tag, dropped with what it
    // swallowed. Text is lost, never markup gained.
    assert_eq!(html("<script>a<b && c</script>\n\nx\n"), "a\n<p>x</p>\n");
}

#[test]
fn unknown_tags_are_stripped_but_their_text_stays() {
    assert_eq!(html("use <name> here\n"), "<p>use  here</p>\n");
    assert_eq!(
        html("<foo>bar</foo> and <b>bold</b>\n"),
        "<p>bar and bold</p>\n"
    );
    assert_eq!(
        html("<div class=\"x\">\n<table><tr><td>cell</td></tr></table>\n</div>\n"),
        "\ncell\n\n"
    );
    // Comments and doctypes vanish.
    assert_eq!(html("a <!-- hidden --> b\n"), "<p>a  b</p>\n");
    assert_eq!(html("<!DOCTYPE html>\n\nx\n"), "\n<p>x</p>\n");
}

#[test]
fn the_raw_allowlist_is_br_sub_sup_kbd_details_summary_without_attributes() {
    assert_eq!(
        html("H<sub>2</sub>O x<sup>2</sup> <kbd>Ctrl</kbd>\n"),
        "<p>H<sub>2</sub>O x<sup>2</sup> <kbd>Ctrl</kbd></p>\n"
    );
    assert_eq!(
        html("<sub id=\"x\" class=\"y\" style=\"color:red\" onclick=\"alert(1)\" title=\"t\">2</sub>\n"),
        "<p><sub>2</sub></p>\n"
    );
    assert_eq!(
        html("<details open class=\"x\" ontoggle=\"alert(1)\"><summary onclick=\"x\">S</summary>body</details>\n"),
        "<details open=\"\"><summary>S</summary>body</details>\n"
    );
    // Markdown inside a disclosure, the GitHub way: the tags are separate
    // blocks, and the final pass pairs them.
    assert_eq!(
        html("<details>\n<summary>More</summary>\n\n*inner*\n\n</details>\n\nafter\n"),
        "<details>\n<summary>More</summary>\n<p><em>inner</em></p>\n</details>\n<p>after</p>\n"
    );
    // An end tag that closes nothing, and a start tag never closed.
    assert_eq!(html("a </sub> b\n"), "<p>a  b</p>\n");
    assert_eq!(html("a <sub>b\n\nc\n"), "<p>a <sub>b</sub></p>\n<p>c</p>\n");
}

#[test]
fn raw_html_cannot_set_an_id_or_a_class() {
    // Ids are allowed on headings and the `@sec` span by the final pass,
    // which is safe only because no raw tag reaches it with an attribute.
    for md in [
        "<h2 id=\"cookie\">x</h2>\n",
        "<span class=\"sec\" id=\"sec-a\"></span>\n",
        "a <span class=\"sec\" id=\"sec-a\">b</span>\n",
        "<a href=\"/x\" id=\"fnref-1\">x</a>\n",
        "<li id=\"fn-1\">x</li>\n",
        "<input type=\"checkbox\" class=\"task-list-item-checkbox\" checked>\n",
        "<img src=\"/x.png\" onerror=\"alert(1)\">\n",
    ] {
        let out = html(md);
        for attr in [
            "id=", "class=", "href=", "src=", "on", "<input", "<img", "<h2", "<a", "<li",
        ] {
            assert!(!out.contains(attr), "{md:?}: {attr} in {out}");
        }
    }
}

#[test]
fn ammonia_keeps_the_ids_rubrica_emits() {
    let out = r("<!-- @sec id=a/b -->\n## ⚠️ Open item\n\n### Q&A\n\nx[^n]\n\n[^n]: note\n");
    let h = &out.html;
    assert!(
        h.contains("<h2 id=\"\u{FE0F}-open-item\"><span class=\"sec\" id=\"sec-a/b\"></span>⚠️ Open item</h2>"),
        "{h}"
    );
    assert!(h.contains("<h3 id=\"qa\">Q&amp;A</h3>"), "{h}");
    // Footnote ids are numbered (`fn-N`, `fnref-N`).
    assert!(h.contains("<a href=\"#fn-1\" id=\"fnref-1\""), "{h}");
    assert!(h.contains("<li id=\"fn-1\">"), "{h}");
    assert!(
        h.contains("<a href=\"#fnref-1\" class=\"footnote-backref\""),
        "{h}"
    );
    // And the classes of task lists.
    let h = html("- [x] a\n");
    assert_eq!(
        h,
        "<ul class=\"contains-task-list\">\n<li class=\"task-list-item\"><input class=\"task-list-item-checkbox\" checked=\"\" disabled=\"\" type=\"checkbox\"> a</li>\n</ul>\n"
    );
}

#[test]
fn javascript_vbscript_and_data_hrefs_are_stripped() {
    // L9: with `unsafe_` on, the previous renderer passes every destination through, so
    // the final pass's scheme allowlist is what removes these, in links,
    // images, autolinks and reference definitions; any case; entities;
    // leading blanks and control characters; tabs and line ends inside.
    let payloads = [
        "javascript:alert(1)",
        "JaVaScRiPt:alert(1)",
        "JAVASCRIPT:alert(1)",
        "&#106;avascript:alert(1)",
        "&#x6A;avascript:alert(1)",
        "&#0000106;avascript:alert(1)",
        "javascript&colon;alert(1)",
        "javascript&#58;alert(1)",
        "java&#9;script:alert(1)",
        "java&#10;script:alert(1)",
        "java&Tab;script:alert(1)",
        "&#1;javascript:alert(1)",
        "&#x20;javascript:alert(1)",
        "&#14;javascript:alert(1)",
        "vbscript:msgbox(1)",
        "VBScript:msgbox(1)",
        "data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==",
        "DATA:text/html,<script>alert(1)</script>",
        "data:image/svg+xml,<svg onload=alert(1)>",
        "data:image/png;base64,iVBORw0KGgo=",
    ];
    for p in payloads {
        for md in [
            format!("[x]({p})\n"),
            format!("[x](<{p}>)\n"),
            format!("![x]({p})\n"),
            format!("[x][r]\n\n[r]: {p}\n"),
            format!("![x][r]\n\n[r]: <{p}>\n"),
            format!("[x]( {p} \"t\")\n"),
            format!("<{p}>\n"),
            format!("<a href=\"{p}\">x</a>\n"),
            format!("<img src=\"{p}\">\n"),
        ] {
            assert_no_dangerous_url(&md);
        }
    }
    // The link text stays; only the attribute goes.
    assert_eq!(html("[x](javascript:alert(1))\n"), "<p><a>x</a></p>\n");
    assert_eq!(
        html("![x](data:image/png;base64,AA)\n"),
        "<p><img alt=\"x\"></p>\n"
    );
    // Allowed schemes and relative URLs pass.
    let out =
        r("[a](https://e.com) [b](http://e.com) [c](mailto:a@e.com) [d](/p#f) [e](#f) [f](x/y)\n");
    assert_eq!(
        urls(&out.html),
        [
            "https://e.com",
            "http://e.com",
            "mailto:a@e.com",
            "/p#f",
            "#f",
            "x/y"
        ]
    );
    // Schemes Wiki.js's DOMPurify allows and Rubrica does not.
    assert_eq!(
        urls(&html("[a](ftp://e.com) [b](tel:+1) [c](xmpp:a@e)\n")),
        Vec::<String>::new()
    );
}

#[test]
fn event_handlers_and_styles_never_survive() {
    for md in [
        "<b onmouseover=alert(1)>x</b>\n",
        "<details ontoggle=alert(1) open>x</details>\n",
        "<svg><script>alert(1)</script></svg>\n",
        "<math><mi xlink:href=\"javascript:alert(1)\">x</mi></math>\n",
        "<iframe src=\"javascript:alert(1)\"></iframe>\n",
        "<p style=\"background:url(javascript:alert(1))\">x</p>\n",
        "[x](/u \"t\\\" onmouseover=\\\"alert(1)\")\n",
        "```js\" onmouseover=\"alert(1)\nx\n```\n",
    ] {
        let out = html(md);
        let lower = out.to_ascii_lowercase();
        for bad in [
            "<script",
            "<svg",
            "<math",
            "<iframe",
            "style=",
            "javascript:",
        ] {
            assert!(!lower.contains(bad), "{md:?}: {bad} in {out}");
        }
        let names = attribute_names(&out);
        assert!(
            !names.iter().any(|n| n.starts_with("on") || n == "style"),
            "{md:?}: {names:?} in {out}"
        );
    }
}

#[test]
fn raw_nesting_is_bounded() {
    // The final pass is O(depth) per block start tag; raw elements deeper
    // than 100 are dropped with their end tags, their text kept.
    let md = format!("{}x{}\n", "<details>".repeat(150), "</details>".repeat(150));
    let out = html(&md);
    assert_eq!(out.matches("<details>").count(), 100);
    assert_eq!(out.matches("</details>").count(), 100);
    assert!(out.contains('x'));
    assert_eq!(r(&md).findings, [RenderFinding::RawHtmlTooDeep { line: 1 }]);
}

#[test]
fn unclosed_raw_tags_that_silence_the_rest_of_the_page_are_a_finding() {
    // 100 `<sub>`s never closed hold the depth: every later raw start tag is
    // dropped. Reported once, at the first.
    let md = format!(
        "{}\n\na <kbd>K</kbd>\n\nb <sup>2</sup>\n",
        "<sub>".repeat(100)
    );
    let out = r(&md);
    assert!(!out.html.contains("<kbd>"), "{}", out.html);
    assert_eq!(out.findings, [RenderFinding::RawHtmlTooDeep { line: 3 }]);
    // 99 leave room for one more: no finding.
    let md = format!("{}\n\na <kbd>K</kbd>\n", "<sub>".repeat(99));
    let out = r(&md);
    assert!(out.html.contains("<kbd>K</kbd>"), "{}", out.html);
    assert_eq!(out.findings, []);
}

#[test]
fn a_raw_piece_with_a_tag_of_too_many_attributes_is_text() {
    let attrs = |n: usize| (0..n).map(|i| format!(" a{i}")).collect::<String>();
    // At the bound the filter runs as ever: the tag kept, its attributes not.
    let out = r(&format!("x <sub{}>2</sub>\n", attrs(MAX_RAW_ATTRIBUTES)));
    assert_eq!(out.html, "<p>x <sub>2</sub></p>\n");
    assert_eq!(out.findings, []);
    // Past it, the piece is escaped text, block or inline, start or end tag.
    for (md, line) in [
        (
            format!("x <sub{}>2</sub>\n", attrs(MAX_RAW_ATTRIBUTES + 1)),
            1,
        ),
        (format!("t\n\n<div{}>\n", attrs(MAX_RAW_ATTRIBUTES + 1)), 3),
        (format!("</div{}>\n", attrs(MAX_RAW_ATTRIBUTES + 1)), 1),
        // Attributes after a slash, or after a quoted value with no blank.
        (
            format!("<div>\n<p{}>\n", "/a".repeat(MAX_RAW_ATTRIBUTES + 1)),
            1,
        ),
        (
            format!("<p{}>\n", " a=\"\"".repeat(MAX_RAW_ATTRIBUTES + 1)),
            1,
        ),
    ] {
        let out = r(&md);
        assert!(out.html.contains("&lt;"), "{}", &out.html[..60]);
        assert!(!out.html.contains("<sub>"), "{}", &out.html[..60]);
        assert_eq!(
            out.findings,
            [RenderFinding::RawHtmlTooManyAttributes { line }],
            "{}",
            &md[..30]
        );
    }
    // Inside a comment or a quoted value they are no attributes.
    for md in [
        format!("<!--<p{}>-->\n", attrs(MAX_RAW_ATTRIBUTES + 1)),
        format!("<p title=\"{}\">\n", attrs(MAX_RAW_ATTRIBUTES + 1)),
    ] {
        assert_eq!(r(&md).findings, [], "{}", &md[..30]);
    }
}

#[test]
fn text_runs_split_for_the_serializer_render_as_one() {
    // The final pass splits long text with empty comments, which ammonia
    // drops; the text is written back to back, entities whole.
    for text in [
        "&".repeat(5_000),
        "a&b°\u{a0}".repeat(2_000),
        "é".repeat(3_000),
    ] {
        let out = r(&format!("{text}\n"));
        let want = text.replace('&', "&amp;").replace('\u{a0}', "&nbsp;");
        assert_eq!(out.html, format!("<p>{want}</p>\n"));
        assert_eq!(out.findings, []);
    }
    let code = "&<>\"".repeat(2_000);
    let out = r(&format!("```\n{code}\n```\n"));
    assert_eq!(
        out.html,
        format!(
            "<pre><code>{}\n</code></pre>\n",
            code.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        )
    );
}

#[test]
fn an_attribute_too_costly_to_serialize_is_dropped() {
    // A title of 4,096 `&`s costs 4,096² > 2²²: left out, the link kept.
    let out = r(&format!("[x](/p \"{}\")\n", "&".repeat(4_096)));
    assert_eq!(out.html, "<p><a href=\"/p\">x</a></p>\n");
    assert_eq!(
        out.findings,
        [RenderFinding::AttributesTooCostly {
            element: "a".into(),
            attribute: "title".into(),
            line: 1
        }]
    );
    // 2,048 cost exactly 2²²: kept.
    let out = r(&format!("[x](/p \"{}\")\n", "&".repeat(2_048)));
    assert!(out.html.contains(" title=\""), "{}", &out.html[..40]);
    assert_eq!(out.findings, []);
    // A 15 KB URL with 256 `&`s, longer than real ones, is kept.
    let url = format!("/q?{}", "a=1&b=2&".repeat(128) + &"x".repeat(14_000));
    let out = r(&format!("[x]({url})\n"));
    assert!(out.html.contains(&url.replace('&', "&amp;")));
    assert_eq!(out.findings, []);
}

#[test]
fn a_link_title_passes() {
    // Wiki.js keeps a link's title (markdown-it writes it, DOMPurify allows
    // it); the corpus has none, so only this test sees the allowlist's.
    assert_eq!(
        html("[x](/u \"Some title\")\n"),
        "<p><a href=\"/u\" title=\"Some title\">x</a></p>\n"
    );
    assert_eq!(
        html("[x]\n\n[x]: https://e.com/ 'T'\n"),
        "<p><a href=\"https://e.com/\" title=\"T\">x</a></p>\n"
    );
}

#[test]
fn markdown_nesting_is_written_at_most_100_containers_deep() {
    let out = r(&format!("{}## Deep\n", ">".repeat(150)));
    assert_eq!(out.html.matches("<blockquote>").count(), 100);
    assert_eq!(out.html.matches("</blockquote>").count(), 100);
    assert!(
        out.html.contains("<h2 id=\"deep\">Deep</h2>"),
        "{}",
        out.html
    );
}

#[test]
fn escape_mode_writes_raw_html_as_text_and_still_runs_the_final_pass() {
    let opts = RenderOptions::wikijs_compat().raw_html(RawHtml::Escape);
    let out = render_strict("<sub>2</sub> [x](javascript:alert(1))\n", &opts);
    // The dangerous href is dropped, as in the default mode (`<a>x</a>`; the previous renderer blanked it
    // to `href=""` before, `docs/spec/deviations.md`).
    assert_eq!(out.html, "<p>&lt;sub&gt;2&lt;/sub&gt; <a>x</a></p>\n");
    assert!(!out.html.contains("javascript:"), "{}", out.html);
}

#[test]
fn raw_html_cannot_set_an_hl_class() {
    // The final pass allows the highlighter's classes on a `span`; a page author's
    // own span never reaches it with a class (the raw-HTML pass drops every attribute).
    for md in [
        "<span class=\"hl-keyword\">x</span>\n",
        "a <span class=\"hl-keyword hl-string\">b</span>\n",
    ] {
        let out = html(md);
        assert!(!out.contains("class=\"hl-"), "{md:?}: {out}");
        assert!(!out.contains("<span class="), "{md:?}: {out}");
    }
    // In a fence it is text, escaped: no span at all.
    let out = html("```rust\n<span class=\"hl-keyword\">x</span>\n```\n");
    assert!(!out.contains("<span"), "{out}");
}
