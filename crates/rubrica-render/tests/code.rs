//! Code blocks and Mermaid, and the render options.
//!
//! Highlighting is the site build's: `rubrica-site` hands the renderer a highlighter. With
//! none (these tests, and the browser preview), the renderer writes
//! `<pre><code class="language-x">` with the code escaped and no inline style.

use rubrica_render::markdown::render_strict;
use rubrica_render::{RawHtml, RenderOptions, Rendered};

fn r(md: &str) -> Rendered {
    render_strict(md, &RenderOptions::wikijs_compat())
}

#[test]
fn a_fence_keeps_its_language_class_for_the_browser_highlighter_and_no_inline_style() {
    let out = r("```bash\necho \"$HOME\" && ls <dir>\n```\n");
    assert_eq!(
        out.html,
        "<pre><code class=\"language-bash\">echo \"$HOME\" &amp;&amp; ls &lt;dir&gt;\n</code></pre>\n"
    );
    assert!(!out.html.contains("style="));
    // Only the first word of the info string; the rest is not an attribute.
    let out = r("```yaml title=\"x\" onclick=y\na: 1\n```\n");
    assert_eq!(
        out.html,
        "<pre><code class=\"language-yaml\">a: 1\n</code></pre>\n"
    );
}

#[test]
fn an_unknown_language_falls_back_to_plain_pre() {
    for lang in ["console", "cmd", "nosuchlang", "c++"] {
        let out = r(&format!("```{lang}\n<b>x</b>\n```\n"));
        assert_eq!(
            out.html,
            format!("<pre><code class=\"language-{lang}\">&lt;b&gt;x&lt;/b&gt;\n</code></pre>\n")
        );
    }
    // No language: no class (Wiki.js writes `class="language-"`).
    assert_eq!(r("```\nx\n```\n").html, "<pre><code>x\n</code></pre>\n");
    assert_eq!(r("    x\n").html, "<pre><code>x\n</code></pre>\n");
    // A language that would break out of its attribute stays inside it.
    let out = r("```a\"><script>alert(1)</script>\nx\n```\n");
    assert!(!out.html.contains("<script"), "{}", out.html);
    assert!(
        out.html.starts_with(
            "<pre><code class=\"language-a&quot;&gt;&lt;script&gt;alert(1)&lt;/script&gt;\">"
        ),
        "{}",
        out.html
    );
}

#[test]
fn a_mermaid_fence_is_language_mermaid_code_escaped() {
    let src = "graph LR\n  a[\"A<br/>b\"] -->|x & y| b\n  click a \"javascript:alert(1)\"\n";
    let out = r(&format!("```mermaid\n{src}```\n"));
    // No script draws the diagram: the fence is code like any other, its source
    // escaped as text, so it cannot be markup.
    assert_eq!(
        out.html,
        "<pre><code class=\"language-mermaid\">graph LR\n  a[\"A&lt;br/&gt;b\"] --&gt;|x &amp; y| b\n  click a \"javascript:alert(1)\"\n</code></pre>\n"
    );
    assert_eq!(out.mermaid_blocks, [src]);
    // The info string's other words are dropped, as for any fence.
    let out = r("```mermaid theme=dark\ngraph TD\n```\n");
    assert_eq!(
        out.html,
        "<pre><code class=\"language-mermaid\">graph TD\n</code></pre>\n"
    );
    // A `</pre>` in the source is text too.
    let out = r("```mermaid\n</pre><script>alert(1)</script>\n```\n");
    assert_eq!(
        out.html,
        "<pre><code class=\"language-mermaid\">&lt;/pre&gt;&lt;script&gt;alert(1)&lt;/script&gt;\n</code></pre>\n"
    );
    // Not Mermaid: another word that starts with it.
    assert!(r("```mermaidjs\nx\n```\n")
        .html
        .contains("language-mermaidjs"));
}

#[test]
fn a_fence_language_is_one_class() {
    // The previous renderer ends the language at a blank, tab or line end; a browser splits
    // a class at a form feed too, so `x⟨FF⟩mermaid` made the classes
    // `language-x` and `mermaid` (found by fuzzing). The language now ends at a
    // form feed, as markdown-it's does (JavaScript's `\s`); a vertical tab,
    // which a class keeps, stays in it.
    for (info, class) in [
        ("x\u{c}mermaid", "language-x"),
        ("rust\u{c}a b", "language-rust"),
        ("rust\u{b}a", "language-rust\u{b}a"),
    ] {
        let out = r(&format!("```{info}\ny\n```\n"));
        assert_eq!(
            out.html,
            format!("<pre><code class=\"{class}\">y\n</code></pre>\n"),
            "{info:?}"
        );
        assert_eq!(out.mermaid_blocks, Vec::<String>::new(), "{info:?}");
    }
    // A leading one is trimmed, as markdown-it trims the info string: a
    // Mermaid fence, as in Wiki.js.
    let out = r("```\u{c}mermaid\ny\n```\n");
    assert_eq!(
        out.html,
        "<pre><code class=\"language-mermaid\">y\n</code></pre>\n"
    );
}

#[test]
fn the_native_preset_is_the_default() {
    assert_eq!(RenderOptions::default(), RenderOptions::native());
    // Called directly, not through `r()`, which pins the compat profile.
    let out = render_strict("a\nb <sub>2</sub>\n", &RenderOptions::default());
    assert_eq!(out.html, "<p>a\nb <sub>2</sub></p>\n");
}

#[test]
fn the_builder_turns_hard_breaks_and_raw_html_off() {
    let opts = RenderOptions::wikijs_compat()
        .hard_breaks(false)
        .raw_html(RawHtml::Escape);
    assert_ne!(opts, RenderOptions::wikijs_compat());
    let out = render_strict("a\nb <sub>2</sub>\n", &opts);
    assert_eq!(out.html, "<p>a\nb &lt;sub&gt;2&lt;/sub&gt;</p>\n");
}
