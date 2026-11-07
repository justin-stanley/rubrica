//! A site renders its pages with the profile its configuration names.

use rubrica_render::{Profile, RenderFinding};
use rubrica_site::{Site, SiteConfig, SourceFile};

fn page_html(render: Profile) -> String {
    let files = vec![SourceFile::regular(
        "home.md".to_owned(),
        b"---\ntitle: A\n---\na\nb\n".to_vec(),
    )];
    let cfg = SiteConfig {
        render,
        ..Default::default()
    };
    let site = Site::from_sources(files, &cfg);
    let page = site.pages().values().next().expect("the one page");
    page.rendered.html.clone()
}

#[test]
fn a_site_renders_with_the_profile_its_config_names() {
    let html = page_html(Profile::Native);
    assert!(!html.contains("<br>"));
    assert!(html.contains("<p>a\nb</p>"));

    let html = page_html(Profile::WikijsCompat);
    assert!(html.contains("<br>"));
}

fn fence_page(lang: &str, code: &str) -> String {
    format!("```{lang}\n{code}\n```\n")
}

fn site_of(body: &str) -> Site {
    let files = vec![SourceFile::regular(
        "home.md".to_owned(),
        format!("---\ntitle: A\n---\n{body}").into_bytes(),
    )];
    let cfg = SiteConfig {
        render: Profile::Native,
        ..Default::default()
    };
    Site::from_sources(files, &cfg)
}

fn html_of(site: &Site) -> String {
    let page = site.pages().values().next().expect("the one page");
    page.rendered.html.clone()
}

fn highlight_findings(site: &Site) -> Vec<RenderFinding> {
    let page = site.pages().values().next().expect("the one page");
    page.rendered
        .findings
        .iter()
        .filter(|f| matches!(f, RenderFinding::CodeBlockNotHighlighted { .. }))
        .cloned()
        .collect()
}

#[test]
fn a_rust_fence_is_highlighted_with_hl_classes_and_no_style() {
    let site = site_of(&fence_page("rust", "fn main() {}"));
    let html = html_of(&site);
    eprintln!("{html}");
    assert!(
        html.contains("<span class=\"hl-storage\">fn</span>"),
        "{html}"
    );
    assert!(
        html.contains("<span class=\"hl-entity\">main</span>"),
        "{html}"
    );
    assert!(!html.contains("style="), "{html}");
    assert!(!html.contains("class=\"\""), "{html}");
    assert!(!html.contains("hl-source"), "{html}");
    assert!(!html.contains("hl-rust"), "{html}");
}

#[test]
fn an_unknown_language_is_plain_and_has_no_finding() {
    let site = site_of(&fence_page("nosuchlang", "a < b && c"));
    let html = html_of(&site);
    assert!(
        html.contains(
            "<pre><code class=\"language-nosuchlang\">a &lt; b &amp;&amp; c\n</code></pre>"
        ),
        "{html}"
    );
    assert!(!html.contains("<span"), "{html}");
    assert!(highlight_findings(&site).is_empty());
    assert!(!site
        .findings()
        .iter()
        .any(|f| format!("{f:?}").contains("CodeBlockNotHighlighted")));
}

#[test]
fn the_highlighter_never_emits_unescaped_input() {
    let site = site_of(&fence_page("html", "<script>alert(1)</script>"));
    let html = html_of(&site);
    assert!(html.contains("<span class=\"hl-"), "{html}");
    assert!(!html.contains("<script"), "{html}");
    // The spans split the text; without them it is the input, escaped.
    let text = html
        .replace("</span>", "")
        .split("<span class=\"hl-")
        .enumerate()
        .map(|(i, part)| {
            if i == 0 {
                part
            } else {
                part.split_once("\">").map_or(part, |(_, rest)| rest)
            }
        })
        .collect::<String>();
    assert!(
        text.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
        "{text}"
    );
    assert!(!text.contains("<span"), "{text}");
}

fn over_cap_page(code: &str) -> Site {
    site_of(&fence_page("rust", code))
}

fn assert_over_cap(
    site: &Site,
    bytes: usize,
    longest_line: usize,
    block_cap: usize,
    at_line: usize,
) {
    let html = html_of(site);
    assert!(!html.contains("<span class=\"hl-"), "{html}");
    let findings = highlight_findings(site);
    assert!(
        findings
            .iter()
            .any(|f| matches!(f, RenderFinding::CodeBlockNotHighlighted { .. })),
        "{findings:?}"
    );
    assert_eq!(
        findings,
        [RenderFinding::CodeBlockNotHighlighted {
            line: at_line,
            bytes,
            longest_line,
            block_cap,
            line_cap: 2000,
            page_budget: None,
        }]
    );
}

#[test]
fn a_code_block_over_the_line_cap_is_a_finding_and_rendered_plain() {
    let line = format!("// {}", "x".repeat(1998));
    assert_eq!(line.len(), 2001);
    let site = over_cap_page(&line);
    // A render finding's line is the body's: the fence is the body's first line.
    assert_over_cap(&site, 2001, 2001, 65536, 1);
}

#[test]
fn a_code_block_over_the_block_cap_is_a_finding_and_rendered_plain() {
    let long = format!("// {}\n", "x".repeat(997));
    let mut code = long.repeat(65);
    code.push_str(&format!("// {}", "x".repeat(469)));
    assert_eq!(code.len(), 65_537);
    let site = over_cap_page(&code);
    assert_over_cap(&site, 65_537, 1000, 65_536, 1);
}

#[test]
fn a_code_block_under_both_caps_has_no_finding() {
    let line = format!("// {}", "x".repeat(1996));
    assert_eq!(line.len(), 1999);
    let code = vec![line; 30].join("\n");
    assert_eq!(code.len(), 59_999);
    let site = over_cap_page(&code);
    let html = html_of(&site);
    assert!(html.contains("<span class=\"hl-comment\">"), "{html}");
    assert!(highlight_findings(&site).is_empty());
}

#[test]
fn a_page_of_dense_highlighted_blocks_stays_renderable_by_a_budget() {
    // `"a":` × 375 per line is the densest shape measured: about 34 bytes of HTML a byte.
    // 35 blocks of 40 lines (2.1 MB) would pass the writer's output cap if all were
    // highlighted, and the page would be replaced by a notice.
    let line = "\"a\":".repeat(375);
    let block = format!("```json\n{}```\n\n", format!("{line}\n").repeat(40));
    let site = site_of(&block.repeat(35));
    let html = html_of(&site);
    assert!(!html.contains("This page is not rendered"), "refused");
    assert!(
        html.contains("<span class=\"hl-"),
        "no block was highlighted"
    );
    assert!(html.len() <= 64 << 20, "{} bytes", html.len());
    let page = site.pages().values().next().expect("the one page");
    let over_budget = page
        .rendered
        .findings
        .iter()
        .filter(|f| {
            matches!(
                f,
                RenderFinding::CodeBlockNotHighlighted {
                    page_budget: Some(b),
                    ..
                } if *b == rubrica_render::writer::MAX_PAGE_HIGHLIGHT_BYTES
            )
        })
        .count();
    eprintln!("blocks written plain for the budget: {over_budget}");
    assert!(over_budget >= 1, "no finding names the page budget");
}
