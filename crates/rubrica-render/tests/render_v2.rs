//! The owned pipeline (`render`) on every Markdown literal the public
//! tests of `markdown`, `sec`, `sec_parity`, `footnotes`, `code`, `sanitize`,
//! `deep_nesting` and `frontmatter` pass to the renderer, and on the 652 CommonMark 0.31.2
//! and 672 GFM 0.29 examples: `render_strict` accepts every one under both profiles. Until
//! the switch to the owned pipeline this file also compared the pipeline with the previous one, field by field; the
//! oracle render-diff does that job now, and the differences it found are in
//! `docs/spec/deviations.md`.

use rubrica_render::markdown::{render, render_strict};
use rubrica_render::{Profile, RenderFinding, RenderOptions};

/// The literals the eight public test files pass to `r(`, `render(`, `render_strict(`,
/// `heading(` and `html(`, in `for md in [..]` arrays and `let md = ".."`, plus
/// `fixtures/md/gfm.md`; each once.
const INPUTS: &[&str] = &[
    "Two views:\n[Second](/views/second)\n",
    "one  \ntwo\\\nthree\n",
    "Intro.\n\n> ## ⚠️ Open item\n> Still open.\n",
    "## Use `gift.*` with [the *guide*](/x) ![logo](/l.png) **now** ~~not~~\n",
    "## Q&amp;A \\*not emphasis\\* FREE_POOL_SIZE &#x3A9;\n",
    "## a <b>b</b> <https://x.example>\n",
    "## Logo ![logo](/l.png)\n",
    "## Title ##\n",
    "## C# #\n",
    "### foo ###b\n",
    "## a \\#\n",
    "foo\nbar\n===\n",
    "# A\n\n## A\n\n> ## A\n\n- ### A\n\n## Open\n\n## 4. NAS config\n\n## a 1\n",
    "# A\n",
    "On example.io, see README.md and example-domain.com or github.com/a/b.\n",
    "www.example.com and someone@example.com\n",
    "a <b>bold</b> <script>alert(1)</script>\n\n<div>\nblock\n</div>\n",
    "[x](javascript:alert(1))\n",
    "# T\n\n[a](/x) [b](#t)\n[c](https://e.example) ![i](/img.png)\n\n| [d](rel) |\n|---|\n",
    "- [ ] before\n\n# T\n\nOne **two** `three`.\n\n## S\n\n- [x] done\n\n```mermaid\ngraph LR\n```\n\n```bash\necho hi\n```\n",
    "~~gone~~ and ~1 ~2, ~~also gone~~\n",
    "## about ~80 and ~90 lines\n",
    "x ~a~ y\n",
    "## x ~a~ y\n",
    "- [ ] **Add a key** *(High)*\n  Your DID has one.\n\n- [x] **Done** it\n\n  More.\n",
    "- [ ] a\n- [x] b\n- c\n",
    "- c\n",
    "[x](#caf%C3%A9)\n",
    "- [x] a\n- [ ]\n",
    "- [ ]  \n  hard\n",
    "| a | b |\n|---|---|\n| c | d |\n",
    "- [y]: /w\n  [ ]\n",
    "- [y]: /w\n  [ ]  \n  z\n",
    "[x]: /u\n\n- [x] foo\n",
    "- a\r- [x] b\n- [ ] c\n",
    "# A\r[l](/u)\r\n## B\n- [ ] t\r",
    "a\r~b~ c\n",
    "x\r~b~\n",
    "[y]: /w\n[l](/u) ~b~\n",
    "[y]: /w\nTitle [l](/u)\n===\n",
    "> [y]: /w\n>    b [l](/u)\n>  ~a~ [m](/v)\n",
    "- [y]: /w\n  [ ] x [l](/u)\n",
    "[y]: /w\n\\* ~b~\n",
    "[y]: /w\n   b ~c~\n",
    "[y]: /w\na ~~b~~\n",
    "`a\r\nb`\r\n",
    "```mermaid\r\ngraph\r\nA\r\n```\r\n",
    "```\ra\rb\r```\r",
    "- [ ] see [a](/u\n  \"title\")\n",
    "- [ ] x\n  [a](/u \"t\nu\")\n",
    "- [ ] see [a](\n  /u)\n",
    "- [ ] see ![i](/p.png\n  \"t\")\n",
    "- [ ] see [a][b\n  c]\n\n[b c]: /u\n",
    "[a] ~b~ [c](/u \"t\nu\")\n",
    "[a](/u \"t\nu\")\n===\n",
    "[a](/u\n\"t\") ~b~ [c](/v)\n",
    "> [a](/u\n>  \"t\") [c](/v) ~d~\n",
    "[a][b\nc] [d](/v)\n\n[b c]: /u\n",
    "x ![i](/p.png\n\"t\") [c](/v) ~e~\n",
    "> [a](/u\n   \"t\") [c](/v) ~d~\n",
    "[a](/u\n\"t\") [b](/v\n\"s\") [c](/w)\n[d](/x) ~e~\n",
    "[y]: /w\n[a](/u\n\"t\") [c](/v) ~b~\n",
    "- [y]: /w\n  [ ] see [a](/u\n  \"t\") [c](/v)\n",
    "[y\nz]: /w \"t\nu\"\n[c](/v) ~b~\n",
    "[a](/u\n\"t\") ~~b~~\n",
    "[l](/u\n\"t\") ~a~ [m](/v)\n|b|\n|-|\n",
    "x\n[y]: /w\n[m](/v\n\"t\") [n](/u) ~a~\n| b |\n| - |\n",
    "> 1.  [y]: /w\n>\t[ ] x\n",
    "- Item[^1]\n\n  [^1]: See [the docs](https://example.com/a\n  \"Docs\") and [this](/x).\n",
    "x[^1]\n\n> [^1]: [y]: /w\n>     h\n>     ===\n",
    "> - a[^n]\n>\n>   [^n]: [b](/u\n>   \"t\") [c](/v)\n",
    "[r]: /d\n=\n[a](/u) | b\n-|-\n",
    "> [r]: /d\n> =\n> [a](/u) | b\n> -|-\n",
    "[r]: /d\n=\n[l](/v\n\"t\") [m](/w)\n[a](/u) | b\n-|-\n[c](/x) | d\n",
    "[a](/u\n\"t\") \u{0} [b](/v)\n",
    "a \\| b [l](/u\n\"t\") [m](/v)\n|c|\n|-|\n",
    "[a](/u\n\"t\") [b](/v)\n",
    "# a\n\n- [ ] b\n",
    "[q]: /w\n=\n|-\n",
    "> [q]: /w\n> =\n> |-\n> [c](/u) | d\n",
    "Intro.\n\n<!-- @sec id=ops/deployments/golden-path tags=deploy,procedure -->\n## Golden path\n\nText.\n",
    "## Golden path\n",
    "<!-- @sec id=a tags= -->\n## Golden path\n",
    "<!-- @sec id=x -->\n## Open\n",
    "<!-- @sec id=ops/storage/backup-gap tags=a -->\n### Gap\n",
    "<!-- @sec id=a -->\n<!-- @sec id=b -->\n## H\n",
    "<!-- @sec id=a -->\r\n## H\r\n",
    "x\ry\n\n<!-- @sec id=a -->\n## H\n",
    "<!-- @sec id=\u{feff}a -->\n## H\n",
    "<!-- @sec id=a\u{85}b -->\n## H\n",
    "<!-- @sec id=a\u{a0}tags=x -->\n## H\n",
    "```\n<!-- @sec id=a -->\n```\n",
    "```\n<!-- @sec tags=x -->\n```\n",
    "\u{feff}<!-- @sec id=a -->\n## H\n",
    "x\n\n\u{feff}<!-- @sec id=a -->\n## H\n",
    "<!-- @sec id=a -->\n# a\u{200b}b\n",
    "<!-- @sec id=a -->\n## One\n\n<!-- @sec id=a -->\n## Two\n",
    "## Sec a\n\n<!-- @sec id=a -->\n## Other\n",
    "<!-- @sec id=a\"b&c -->\n## H\n",
    "```bash\necho \"$HOME\" && ls <dir>\n```\n",
    "```yaml title=\"x\" onclick=y\na: 1\n```\n",
    "```\nx\n```\n",
    "    x\n",
    "```a\"><script>alert(1)</script>\nx\n```\n",
    "```mermaid theme=dark\ngraph TD\n```\n",
    "```mermaid\n</pre><script>alert(1)</script>\n```\n",
    "```mermaidjs\nx\n```\n",
    "```\u{c}mermaid\ny\n```\n",
    "a\nb <sub>2</sub>\n",
    "a<br>b <br/> c\n",
    "a <script>alert(1)</script> b\n",
    "<script>\nalert(1)\n</script>\n",
    "<script>a && c > b</script>\n\nx\n",
    "<script>a<b && c</script>\n\nx\n",
    "use <name> here\n",
    "<foo>bar</foo> and <b>bold</b>\n",
    "<div class=\"x\">\n<table><tr><td>cell</td></tr></table>\n</div>\n",
    "a <!-- hidden --> b\n",
    "<!DOCTYPE html>\n\nx\n",
    "H<sub>2</sub>O x<sup>2</sup> <kbd>Ctrl</kbd>\n",
    "<sub id=\"x\" class=\"y\" style=\"color:red\" onclick=\"alert(1)\" title=\"t\">2</sub>\n",
    "<details open class=\"x\" ontoggle=\"alert(1)\"><summary onclick=\"x\">S</summary>body</details>\n",
    "<details>\n<summary>More</summary>\n\n*inner*\n\n</details>\n\nafter\n",
    "a </sub> b\n",
    "a <sub>b\n\nc\n",
    "<!-- @sec id=a/b -->\n## ⚠️ Open item\n\n### Q&A\n\nx[^n]\n\n[^n]: note\n",
    "- [x] a\n",
    "![x](data:image/png;base64,AA)\n",
    "[a](https://e.com) [b](http://e.com) [c](mailto:a@e.com) [d](/p#f) [e](#f) [f](x/y)\n",
    "[a](ftp://e.com) [b](tel:+1) [c](xmpp:a@e)\n",
    "[x](/u \"Some title\")\n",
    "[x]\n\n[x]: https://e.com/ 'T'\n",
    "<sub>2</sub> [x](javascript:alert(1))\n",
    "(+ fiber office-backhaul ~$420–580, separate page) · mini PC (~$500)\n",
    "x ~a~ y and (~1)~ z\n",
    "> about ~$1 and (~$2)\n",
    "- about ~$1 and (~$2)\n",
    "1. *about ~$1 and (~$2)* GbE\n",
    "| cost | range |\n|---|---|\n| ~$449 and (~$500) | ~a~ |\n",
    "- [ ] ok\n",
    "- [X] up\n",
    "1. [ ] num\n",
    "- [ ] `c`\n",
    "- [ ]   spaces\n",
    "- [ ] \n  soft\n",
    "- [ ] **st**\n",
    "- [y]: /w\n  [ ] x\n",
    "- [y]: /w\n  [x] x\n",
    "```\r\na\r\nb\r\n```\r\n",
    "    a\r\n    b\r\n",
    "> > [r]: /d\n> > =\n>\t[a](/u) | b\n> > -|-\n",
    "> 1. [y]: /d\n>    =\n>\t[a](/u) | b\n>    -|-\n",
    "  <!--@sec id=a -->\n## H\n",
    "<!--\t@sec\tid=a\ttags=x-->\t\n## H\n",
    "<!-- @sec id=a -->\n   ## H\n",
    "<!-- @sec tags=x id=b id=a -->\n## H\n",
    "<!-- sec id=a -->\n## H\n",
    "<!-- @section id=a -->\n## H\n",
    "<!-- @sec id=a> -->\n## H\n",
    "<!-- note --> <!-- @sec id=a -->\n## H\n",
    "<!-- @sec id=a -->\n\n## H\n",
    "<!-- @sec id=a -->\ntext\n",
    "x\n\n<!-- @sec id=a -->\n- **Cleanup**\n",
    "<!-- @sec id=a -->\nH\n===\n",
    "<!-- @sec id=a -->\n",
    "<!-- @sec id=a -->\r## H\r",
    "<!-- @sec id=a -->\n#\u{a0}H\n",
    "> <!-- @sec id=a -->\n> ## Quoted\n",
    "- <!-- @sec id=a -->\n  ## Listed\n",
    "1. x\n\n   <!-- @sec id=a -->\n   ## Listed\n",
    "- x\n\n  <!-- @sec id=a -->\n\n  text\n",
    "> <!-- @sec id=a -->\n\n## H\n",
    "x[^1]\n\n[^1]: y\n\n    <!-- @sec id=a -->\n    ## In a note\n",
    "<!-- @sec id=a -->\n#\n",
    "<!-- @sec id=a -->\n# #\n",
    "<!-- @sec id=a -->\n##   \n",
    "<!-- @sec id=a -->\n### ![](/i.png)\n",
    "```\n<!-- @sec id=a -->\n## H\n```\n",
    "~~~md\n<!-- @sec id=a -->\n~~~\n",
    "    <!-- @sec id=a -->\n\n## H\n",
    "\t<!-- @sec id=a -->\n## H\n",
    "x\n\n    <!-- @sec id=a -->\n",
    "A `<!-- @sec id=a -->` marker.\n## H\n",
    "See `@sec`, an off-vocabulary tag.\n",
    "x <!-- @sec id=a -->\n## H\n",
    "x\u{2028}<!-- @sec id=a -->\n## H\n",
    "x\u{2029}<!-- @sec id=a -->\n## H\n",
    "<!-- @sec id=a -->\u{2028}y\n## H\n",
    "<!-- @sec id=a\n-->\n## H\n",
    "<!-- @sec id=a \n -->\n## H\n",
    "<div>x</div>\n<!-- @sec id=a -->\n# H\n",
    "<details>\n<!-- @sec id=a -->\n## H\n",
    "<!-- @sec tags=a -->\n## H\n",
    "<!-- @sec id= -->\n## H\n",
    "<!-- @sec id=b -->\n# H\n",
    "# fn-1\n\nx[^1]\n\n[^1]: y\n",
    "x[^1]\n\n## fnref-1\n\n[^1]: y\n",
    "x[^a] y[^a]\n\n### fnref-a-2\n\n[^a]: z\n",
    "# fn-2\n\nx[^1]\n\n[^1]: y\n",
    "# fn-1\n\n[^1]: y\n",
    "# fn-1\n",
    "x[^1]\n\n[^1]: y\n",
    "x[^1]\n\n{}[^1]: y\n",
    "{}x[^1]\n\n[^1]: y\n",
    "x[^1]\n\n[^1]: {}y\n",
    "{}x[^1]\n\n{}[^1]: y\n",
    "x[^1] y[^2]\n\n[^1]: [^2]: b x[^2]\n",
    "x[^1]\n\n[^1]: a\n\n[^1]: b\n",
    "{}[^1]: a\n\nx[^1]\n\n[^1]: b\n",
    "x[^1]\n\n[^1]: a[^2]\n\n[^2]: b[^1]\n",
    "x[^z]\n\n{}y\n",
    "<h2 id=\"cookie\">x</h2>\n",
    "<span class=\"sec\" id=\"sec-a\"></span>\n",
    "a <span class=\"sec\" id=\"sec-a\">b</span>\n",
    "<a href=\"/x\" id=\"fnref-1\">x</a>\n",
    "<li id=\"fn-1\">x</li>\n",
    "<input type=\"checkbox\" class=\"task-list-item-checkbox\" checked>\n",
    "<img src=\"/x.png\" onerror=\"alert(1)\">\n",
    "[x]({p})\n",
    "[x](<{p}>)\n",
    "![x]({p})\n",
    "[x][r]\n\n[r]: {p}\n",
    "![x][r]\n\n[r]: <{p}>\n",
    "[x]( {p} \"t\")\n",
    "<{p}>\n",
    "<a href=\"{p}\">x</a>\n",
    "<img src=\"{p}\">\n",
    "<b onmouseover=alert(1)>x</b>\n",
    "<details ontoggle=alert(1) open>x</details>\n",
    "<svg><script>alert(1)</script></svg>\n",
    "<math><mi xlink:href=\"javascript:alert(1)\">x</mi></math>\n",
    "<iframe src=\"javascript:alert(1)\"></iframe>\n",
    "<p style=\"background:url(javascript:alert(1))\">x</p>\n",
    "[x](/u \"t\\\" onmouseover=\\\"alert(1)\")\n",
    "```js\" onmouseover=\"alert(1)\nx\n```\n",
    "<!--<p{}>-->\n",
    "<p title=\"{}\">\n",
    "| Host | Role |\n|---|---|\n| apps | ~~compute~~ edge |\n\n- [ ] open item\n- [x] done item\n\nSee https://example.com/path and <https://example.org/x>, or www.example.net.\n",
];
/// Whole page files (front matter and body) the tests render with `render_page`, and
/// `fixtures/md/block-scalars.md`.
const PAGES: &[&str] = &[
    "---\ntitle: T\ndescription: |\n  <!-- @sec id=fm -->\n---\n<!-- @sec id=b -->\n# H\n",
    "---\ntitle: T\nnote: |\n  <!-- @sec tags=x -->\n---\n# H\n",
    "---\ntitle: T\nnote: |\n  <!-- @sec id=fm\n---\n<!-- @sec id=b -->\n# H\n",
    "---\ntitle: T\nnote: |\n  <!-- @sec id=fm\n---\n<!-- @sec tags=x -->\n# H\n",
    "graph LR\n  a[\"A<br/>b\"] -->|x & y| b\n  click a \"javascript:alert(1)\"\n",
    "---\ntitle: T\rdescription: d\n---\n",
    "---\r\ntitle: T\r\nx: 1\r---\r\nbody\n",
    "---\ntitle: T\n---\rbody\n---\n",
    "---\ntitle: T\n---\r\r\nbody\n",
    "---\ntitle: T\r---\rbody\n",
    "---\ntitle: T\n---\r",
    "---\r\ntitle: T\r\n---\r",
    "---\ntitle: T\n---\rx",
    "---\ntitle: T\n---\r\r",
    "---\rtitle: T\n---\r",
    "---\ntitle: T\r",
    "---\r\ntitle: x\r\n---\r\n- a\r- [ ] b\r\n",
    "---\ntitle: Four YAML forms\ndescription: >\n  folded onto\n  one line\n\n  and a second paragraph\nsummary: |-\n  literal\n  keeps its newlines\nquoted: \"a quoted value\n  that spans lines\"\nplain: a plain value\n  that spans lines\npublished: true\n---\n\nBody.\n",
];

/// Inputs the public tests build with `format!` and `repeat`, at the sizes where their
/// bounds bite (named, since a known difference cannot quote them).
fn generated() -> Vec<(&'static str, String)> {
    let stars = "*".repeat(1000);
    let attrs = |n: usize| (0..n).map(|i| format!(" a{i}")).collect::<String>();
    let mut out = vec![
        (
            "gen-task-in-100-quotes",
            format!("{}- [ ] a\n", "> ".repeat(100)),
        ),
        (
            "gen-tasks-in-50-quote-lists",
            format!("{}x\n", "> - [ ] ".repeat(50)),
        ),
        ("gen-150-nested-lists", format!("{}x\n", "- ".repeat(150))),
        ("gen-150-quotes", format!("{}x\n", ">".repeat(150))),
        (
            "gen-heading-in-150-quotes",
            format!("{}## Deep\n", ">".repeat(150)),
        ),
        (
            "gen-heading-code-in-150-quotes",
            format!("{} # Deep `x`\n", ">".repeat(150)),
        ),
        (
            "gen-footnote-in-150-quotes",
            format!("{}[^d]: D\n\nx[^d]\n", "> ".repeat(150)),
        ),
        (
            "gen-deep-emphasis",
            format!("# {stars}deep{stars}\n\n> {stars}body{stars}\n"),
        ),
        (
            "gen-150-details",
            format!("{}x{}\n", "<details>".repeat(150), "</details>".repeat(150)),
        ),
        (
            "gen-100-sub",
            format!(
                "{}\n\na <kbd>K</kbd>\n\nb <sup>2</sup>\n",
                "<sub>".repeat(100)
            ),
        ),
        (
            "gen-99-sub",
            format!("{}\n\na <kbd>K</kbd>\n", "<sub>".repeat(99)),
        ),
        (
            "gen-100-footnote-refs",
            format!("{}\n\n[^1]: x\n", "[^1]".repeat(100)),
        ),
        (
            "gen-101-footnote-refs",
            format!("{}\n\n[^1]: x\n", "[^1]".repeat(101)),
        ),
        (
            "gen-1000-footnote-refs",
            format!("{}\n\n[^1]: x\n", "[^1]".repeat(1000)),
        ),
        (
            "gen-sub-at-attribute-bound",
            format!(
                "x <sub{}>2</sub>\n",
                attrs(rubrica_render::sanitize::MAX_RAW_ATTRIBUTES)
            ),
        ),
        (
            "gen-sub-past-attribute-bound",
            format!(
                "x <sub{}>2</sub>\n",
                attrs(rubrica_render::sanitize::MAX_RAW_ATTRIBUTES + 1)
            ),
        ),
        (
            "gen-div-past-attribute-bound",
            format!(
                "t\n\n<div{}>\n",
                attrs(rubrica_render::sanitize::MAX_RAW_ATTRIBUTES + 1)
            ),
        ),
        (
            "gen-end-tag-past-attribute-bound",
            format!(
                "</div{}>\n",
                attrs(rubrica_render::sanitize::MAX_RAW_ATTRIBUTES + 1)
            ),
        ),
        (
            "gen-costly-reference-title",
            format!("[x](/p \"{}\")\n", "&".repeat(5000)),
        ),
    ];
    for lang in ["console", "cmd", "nosuchlang", "c++"] {
        out.push(("gen-fence-language", format!("```{lang}\n<b>x</b>\n```\n")));
    }
    for info in [
        "x\u{c}mermaid",
        "rust\u{c}a b",
        "rust\u{b}a",
        "\u{c}mermaid",
    ] {
        out.push(("gen-fence-info-form-feed", format!("```{info}\ny\n```\n")));
    }
    out.push((
        "gen-mermaid-fence",
        "```mermaid\ngraph LR\n  a[\"A<br/>b\"] -->|x & y| b\n  click a \"javascript:alert(1)\"\n```\n".into(),
    ));
    for c in [
        '\u{200b}', '\u{200c}', '\u{200d}', '\u{2060}', '\u{feff}', '\u{ad}',
    ] {
        out.push((
            "gen-sec-on-invisible-heading",
            format!("<!-- @sec id=a -->\n# {c}\n"),
        ));
    }
    out
}

fn spec_examples() -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct Example {
        markdown: String,
    }
    let mut all: Vec<Example> =
        serde_json::from_str(include_str!("fixtures/spec/commonmark-0.31.2.json")).unwrap();
    all.extend(
        serde_json::from_str::<Vec<Example>>(include_str!("fixtures/spec/gfm-0.29.json")).unwrap(),
    );
    assert_eq!(all.len(), 1324);
    all.into_iter().map(|e| e.markdown).collect()
}

/// Every input, in a fixed order: the tests' literals, their pages (whole files, as a
/// body), the spec examples, the generated shapes.
fn all_inputs() -> Vec<String> {
    let mut v: Vec<String> = INPUTS
        .iter()
        .chain(PAGES)
        .map(|t| (*t).to_owned())
        .collect();
    v.extend(spec_examples());
    v.extend(generated().into_iter().map(|(_, t)| t));
    assert!(v.len() >= 1400, "{}", v.len());
    v
}

/// A guard: `render_strict` checks the position of every text and panics on a wrong
/// one (the check itself is `adapt::check_positions`, tested in the adapter's tests).
#[test]
fn render_strict_accepts_every_input() {
    for input in all_inputs() {
        for opts in [RenderOptions::wikijs_compat(), RenderOptions::native()] {
            let _ = render_strict(&input, &opts);
        }
    }
    assert_eq!(RenderOptions::native(), RenderOptions::native());
    assert_ne!(RenderOptions::native(), RenderOptions::wikijs_compat());
    assert_eq!(RenderOptions::default(), RenderOptions::native());
    let _ = Profile::Native;
}

/// The shapes of `deep_nesting.rs` (100,000 deep) on a 2 MiB stack, the size of `serve`'s
/// worker threads: nothing in `render` recurses with the nesting.
#[test]
fn render_renders_100k_deep_nesting_on_a_2_mib_stack() {
    const DEPTH: usize = 100_000;
    let stars = "*".repeat(DEPTH / 2);
    let inputs = [
        format!("{}x\n", ">".repeat(DEPTH)),
        format!("{} # Deep `x`\n", ">".repeat(DEPTH)),
        format!("{}x\n", "> - [ ] ".repeat(DEPTH / 2)),
        format!("{}x\n", "- ".repeat(DEPTH)),
        format!("# {stars}deep{stars}\n\n> {stars}body{stars}\n"),
    ];
    for md in inputs {
        let out = std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(move || render(&md, &RenderOptions::wikijs_compat()))
            .unwrap()
            .join()
            .expect("render overflowed its stack or panicked");
        assert!(!out.plain_text.is_empty());
    }
}

/// A leading BOM is skipped as the previous renderer
/// skipped it, and the line numbers stay the body's. The expected values are the previous renderer's
/// render, pinned when the comparison with it went.
#[test]
fn a_leading_bom_is_skipped_and_lines_are_unchanged() {
    let o = RenderOptions::wikijs_compat();
    // The harvest is of the body as given, BOM and all, as check-packages reads it.
    let out = render("\u{feff}<!-- @sec id=a -->\n## H\n", &o);
    assert_eq!(out.html, "<h2 id=\"h\">H</h2>\n");
    assert_eq!(out.headings[0].line, 2);
    assert_eq!(
        out.findings,
        [RenderFinding::SecNotSeenByCheckPackages {
            id: "a".into(),
            line: 1
        }]
    );
    let md = "# A\n\n[x](/u)\n\n- [ ] t\n";
    let out = render(&format!("\u{feff}{md}"), &o);
    assert_eq!(out, render(md, &o));
    assert_eq!(
        (
            out.headings[0].line,
            out.links[0].line,
            out.task_items[0].line
        ),
        (1, 3, 5)
    );
    // One BOM only.
    assert_eq!(render("\u{feff}\u{feff}x\n", &o).html, "<p>\u{feff}x</p>\n");
}

/// CommonMark 2.3: U+0000 is written as U+FFFD, in text, code and attributes.
#[test]
fn nul_is_written_as_the_replacement_character() {
    let o = RenderOptions::wikijs_compat();
    for md in [
        "a\0b\n",
        "`a\0b`\n",
        "```\na\0b\n```\n",
        "[x](/u \"a\0b\")\n",
        "[a](/u\n\"t\") \0 [b](/v)\n",
    ] {
        let new = render(md, &o);
        assert!(
            !new.html.contains('\0') && new.html.contains('\u{fffd}'),
            "{md:?}: {}",
            new.html
        );
    }
}

/// A task marker alone before a line break gets no space after its checkbox, as the previous renderer
/// wrote it (`<input …><br>`), in both markups.
#[test]
fn a_lone_task_marker_before_a_break_has_no_space() {
    let o = RenderOptions::wikijs_compat();
    // The previous renderer's HTML, pinned when the comparison with it went.
    let item = |text: &str| {
        format!("<ul class=\"contains-task-list\">\n<li class=\"task-list-item\"><input class=\"task-list-item-checkbox\" disabled=\"\" type=\"checkbox\"><br>\n{text}</li>\n</ul>\n")
    };
    for (md, text) in [
        ("- [ ]  \n  hard\n", "hard"),
        ("- [y]: /w\n  [ ]  \n  z\n", "z"),
        ("- [ ] \n  soft\n", "soft"),
    ] {
        assert_eq!(render(md, &o).html, item(text), "{md:?}");
    }
    // The GFM markup, from a hand-built event vector (pulldown itself never leaves a
    // marker alone before a break).
    use rubrica_markdown::{Event, Tag, TagEnd};
    use rubrica_render::writer::{write, WriterOptions};
    let events = vec![
        (Event::Start(Tag::List(None)), 0..0),
        (Event::Start(Tag::Item), 0..0),
        (Event::TaskListMarker(false), 2..5),
        (Event::HardBreak, 5..7),
        (Event::Text("x".into()), 8..9),
        (Event::End(TagEnd::Item), 0..0),
        (Event::End(TagEnd::List(false)), 0..0),
    ];
    let html = write(events, &[], WriterOptions::NATIVE).html;
    assert!(html.contains("type=\"checkbox\"><br"), "{html}");
}

/// Whatever the writer does not write produces
/// nothing in `Rendered`. A footnote definition the writer drops (a duplicate, one nothing
/// written references, one past the container bound) yields no heading, link, task item or text; every heading id is in the HTML.
#[test]
fn nothing_from_an_unwritten_footnote_definition_reaches_rendered() {
    let dead = "## Dead [d](/dead)\n\n    - [ ] dead\n";
    let deep = format!("x[^1]\n\n{}[^1]: ## Dead [d](/dead)\n", "> ".repeat(101));
    let inputs = [
        "Text[^1]\n\n[^1]: ## A\n\nText[^1]\n\n[^1]: ## B\n".to_owned(),
        format!("x[^1]\n\n[^1]: a\n\n[^1]: {dead}"),
        format!("y\n\n[^1]: {dead}"),
        format!("y\n\n[^1]: a[^2]\n\n[^2]: {dead}"),
        deep,
    ];
    for opts in [RenderOptions::wikijs_compat(), RenderOptions::native()] {
        for md in &inputs {
            let out = render(md, &opts);
            for h in &out.headings {
                if let Some(id) = &h.id {
                    assert!(
                        out.html.contains(&format!("id=\"{id}\"")),
                        "{md:?}: heading id {id:?} is not in the HTML: {}",
                        out.html
                    );
                }
            }
            assert!(
                out.links.iter().all(|l| l.href != "/dead"),
                "{md:?}: {:?}",
                out.links
            );
            assert_eq!(out.task_items, [], "{md:?}");
            assert!(
                !out.plain_text.contains("Dead"),
                "{md:?}: {:?}",
                out.plain_text
            );
            assert!(
                !out.plain_text.contains("dead"),
                "{md:?}: {:?}",
                out.plain_text
            );
        }
    }
    // A definition a written one references is written, and collected.
    let out = render(
        "x[^1]\n\n[^1]: a[^2]\n\n[^2]: ## Live [l](/live)\n",
        &RenderOptions::wikijs_compat(),
    );
    assert_eq!(out.headings.len(), 1, "{:?}", out.headings);
    assert!(out.html.contains("id=\"live-l\""), "{}", out.html);
    assert_eq!(out.links.len(), 1);
}

/// A `%` that starts no `%XX` escape is
/// written `%25`, as the previous renderer and markdown-it write it; a `%XX` is kept.
#[test]
fn a_lone_percent_in_an_href_is_encoded() {
    for opts in [RenderOptions::wikijs_compat(), RenderOptions::native()] {
        for (md, href) in [
            ("[x](a%2)\n", "a%252"),
            ("[x](a%20b%)\n", "a%20b%25"),
            ("[x](/%zz%41)\n", "/%25zz%41"),
        ] {
            let html = render(md, &opts).html;
            assert!(html.contains(&format!("href=\"{href}\"")), "{md:?}: {html}");
        }
    }
}
