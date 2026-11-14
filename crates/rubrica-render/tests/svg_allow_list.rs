//! The SVG allow-list's check: a diagram in the vocabulary passes, and every construct outside
//! it is refused with its reason.

use rubrica_render::sanitize::svg_allowed;

/// The rectangle of [`S`].
const RECT: &str = concat!(
    "<rect class=\"rd-shape\" x=\"1.00\" y=\"1.00\" width=\"5.00\" height=\"5.00\" ",
    "rx=\"5.00\" ry=\"5.00\"></rect>"
);

/// A diagram in the vocabulary: every element, every attribute, text that needs escaping.
const S: &str = concat!(
    "<svg role=\"img\" viewBox=\"0 0 10.00 10.00\" width=\"10.00\" height=\"10.00\" ",
    "class=\"rd rd-flowchart\" font-family=\"Inter, sans-serif\" font-size=\"14\">",
    "<title>t</title><desc>a --&gt; b</desc><g class=\"rd-nodes\"><g class=\"rd-node\">",
    "<rect class=\"rd-shape\" x=\"1.00\" y=\"1.00\" width=\"5.00\" height=\"5.00\" ",
    "rx=\"5.00\" ry=\"5.00\"></rect><path class=\"rd-shape-line\" d=\"M1.00 1.00 V5.00 ",
    "Z\"></path><text class=\"rd-text\" x=\"2.00\" y=\"3.00\" text-anchor=\"middle\">",
    "a &amp; b</text></g></g></svg>"
);

/// S with `from` replaced by `to`, once; `from` must be in S.
fn with(from: &str, to: &str) -> String {
    assert!(S.contains(from), "{from:?} is not in S");
    S.replacen(from, to, 1)
}

/// S with `x` inside its inner `g`, after the `text`.
fn inner(x: &str) -> String {
    with("</text></g>", &format!("</text>{x}</g>"))
}

/// S's rectangle changed.
fn rect(from: &str, to: &str) -> String {
    assert!(RECT.contains(from), "{from:?} is not in the rect");
    with(RECT, &RECT.replacen(from, to, 1))
}

#[test]
fn a_diagram_in_the_vocabulary_is_allowed() {
    assert_eq!(svg_allowed(S), Ok(()));
}

#[test]
fn every_construct_outside_the_vocabulary_is_refused() {
    let cases: Vec<(&str, String, &str)> = vec![
        (
            "foreignobject",
            inner("<foreignObject></foreignObject>"),
            "`<foreignobject>` is not allowed",
        ),
        ("a", inner("<a href=\"/x\">x</a>"), "`<a>` is not allowed"),
        (
            "script",
            inner("<script>x</script>"),
            "`<script>` is not allowed",
        ),
        (
            "style element",
            inner("<style>*{}</style>"),
            "`<style>` is not allowed",
        ),
        (
            "use",
            inner("<use href=\"#a\"></use>"),
            "`<use>` is not allowed",
        ),
        (
            "image",
            inner("<image href=\"x\"></image>"),
            "`<image>` is not allowed",
        ),
        (
            "animate",
            inner("<animate attributeName=\"x\"></animate>"),
            "`<animate>` is not allowed",
        ),
        (
            "set",
            inner("<set to=\"x\"></set>"),
            "`<set>` is not allowed",
        ),
        (
            "circle",
            inner("<circle r=\"1\"></circle>"),
            "`<circle>` is not allowed",
        ),
        (
            "tspan",
            with("a &amp; b</text>", "a &amp; b<tspan>x</tspan></text>"),
            "`<tspan>` is not allowed",
        ),
        (
            "html in title",
            with("<title>t</title>", "<title><b>t</b></title>"),
            "`<b>` is not allowed",
        ),
        ("nested svg", inner(S), "`<svg>` inside `<g>`"),
        (
            "rect under svg",
            with("</desc>", &format!("</desc>{RECT}")),
            "`<rect>` inside `<svg>`",
        ),
        (
            "no title",
            with("<title>t</title>", ""),
            "`<title>` and `<desc>` must open the `<svg>`",
        ),
        (
            "self-closing",
            with(RECT, &RECT.replace("></rect>", " />")),
            "a self-closing `<rect>`",
        ),
        (
            "style attribute",
            rect("ry=\"5.00\"", "ry=\"5.00\" style=\"fill:red\""),
            "`style` on `<rect>` is not allowed",
        ),
        (
            "onload",
            with("<svg role", "<svg onload=\"x\" role"),
            "`onload` on `<svg>` is not allowed",
        ),
        (
            "href",
            with("<g class=\"rd-node\">", "<g class=\"rd-node\" href=\"/x\">"),
            "`href` on `<g>` is not allowed",
        ),
        (
            "xlink",
            with("d=\"M1.00", "xlink:href=\"/x\" d=\"M1.00"),
            "`xlink:href` on `<path>` is not allowed",
        ),
        (
            "fill",
            rect("ry=\"5.00\"", "ry=\"5.00\" fill=\"url(#x)\""),
            "`fill` on `<rect>` is not allowed",
        ),
        (
            "transform",
            with(
                "<g class=\"rd-node\">",
                "<g class=\"rd-node\" transform=\"scale(2)\">",
            ),
            "`transform` on `<g>` is not allowed",
        ),
        (
            "exponent",
            rect("x=\"1.00\"", "x=\"1e9\""),
            "a bad `x` value on `<rect>`",
        ),
        (
            "negative",
            rect("x=\"1.00\"", "x=\"-1.00\""),
            "a bad `x` value on `<rect>`",
        ),
        (
            "one decimal",
            rect("x=\"1.00\"", "x=\"1.0\""),
            "a bad `x` value on `<rect>`",
        ),
        (
            "curve",
            with("d=\"M1.00 1.00 V5.00 Z\"", "d=\"M1.00 1.00 C2.00 2.00\""),
            "a bad `d` value on `<path>`",
        ),
        (
            "script in d",
            with(
                "d=\"M1.00 1.00 V5.00 Z\"",
                "d=\"M1.00 1.00 &#106;avascript:x\"",
            ),
            "a bad `d` value on `<path>`",
        ),
        (
            "view box",
            with("viewBox=\"0 0 10.00 10.00\"", "viewBox=\"0 0 10 10\""),
            "a bad `viewbox` value on `<svg>`",
        ),
        (
            "role",
            with("role=\"img\"", "role=\"presentation\""),
            "a bad `role` value on `<svg>`",
        ),
        (
            "font",
            with("font-family=\"Inter, sans-serif\"", "font-family=\"x\""),
            "a bad `font-family` value on `<svg>`",
        ),
        (
            "empty class",
            with("<g class=\"rd-node\">", "<g class=\"\">"),
            "a bad `class` value on `<g>`",
        ),
        (
            "evil class",
            rect("class=\"rd-shape\"", "class=\"rd-shape evil\""),
            "the class `evil` is not allowed on `<rect>`",
        ),
        (
            "svg class",
            with("class=\"rd rd-flowchart\"", "class=\"rd-flowchart rd\""),
            "a bad `class` value on `<svg>`",
        ),
        (
            "rx alone",
            rect(" ry=\"5.00\"", ""),
            "`rx` without `ry` on `<rect>`",
        ),
        (
            "missing height",
            rect(" height=\"5.00\"", ""),
            "`<rect>` lacks `height`",
        ),
        (
            "text in g",
            inner("x"),
            "text outside `<text>`, `<title>` and `<desc>`",
        ),
        (
            "space between tags",
            with("</desc>", "</desc> "),
            "text outside `<text>`, `<title>` and `<desc>`",
        ),
        (
            "comment",
            inner("<!-- x -->"),
            "a comment, doctype or processing instruction",
        ),
        (
            "cdata",
            inner("<![CDATA[x]]>"),
            "a comment, doctype or processing instruction",
        ),
        (
            "trailing",
            format!("{S}<p>x</p>"),
            "not one `<svg>` element",
        ),
        ("leading", format!("<p>x</p>{S}"), "not one `<svg>` element"),
        (
            "unclosed",
            S.strip_suffix("</svg>").unwrap().to_string(),
            "not one `<svg>` element",
        ),
        (
            "wrong close",
            with("</text></g>", "</text></text>"),
            "`</text>` does not close `<g>`",
        ),
    ];
    let mut bad = Vec::new();
    for (name, case, want) in &cases {
        let got = svg_allowed(case);
        let want = Err((*want).to_string());
        if got != want {
            bad.push(format!("{name}: got {got:?}, want {want:?}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
