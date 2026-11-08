//! `rubrica build`'s output, from small in-memory sites: the files it writes, the
//! theme pass, escaping, the feed, the budgets and what it refuses. The git side (reading
//! the tree, history, writing `out/`) is the binary's, tested in
//! `crates/rubrica/tests/build.rs`.

use std::cell::RefCell;
use std::collections::BTreeMap;

use html5ever::tokenizer::{
    BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use rubrica_site::build::theme::theme_body;
use rubrica_site::build::{
    self, refusals, BuildConfig, BuildError, Commit, FeedConfig, History, RecentCommit,
};
use rubrica_site::{Finding, Site, SiteConfig, SourceFile};

// ---------- helpers ----------

fn commit(id_byte: char, time: i64, summary: &str) -> Commit {
    Commit {
        id: id_byte.to_string().repeat(40),
        time,
        offset: -7 * 3600,
        summary: summary.into(),
    }
}

fn no_history() -> History {
    History {
        commit: commit('c', 1_792_981_257, "the commit built"),
        last: BTreeMap::new(),
        recent: Vec::new(),
        feed_versions: Vec::new(),
        complete: true,
    }
}

/// A neutral configuration with every optional link set, so the tests
/// see edit, history, canonical and feed output.
fn synthetic_cfg() -> BuildConfig {
    BuildConfig {
        base_url: Some("https://wiki.example.org".into()),
        edit_url: Some("https://forge.example/edit/main/".into()),
        history_url: Some("https://forge.example/history/main/".into()),
        ..BuildConfig::default()
    }
}

/// The files a build writes, each once.
fn build_files(
    site: &Site,
    cfg: &BuildConfig,
    history: &History,
    sources: &BTreeMap<String, Vec<u8>>,
) -> Result<BTreeMap<String, Vec<u8>>, BuildError> {
    let mut out: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut order = Vec::new();
    build::build(
        site,
        cfg,
        history,
        &mut |page| Ok(sources[&*page.file].clone()),
        &mut |path, bytes| {
            order.push(path.to_owned());
            assert!(
                out.insert(path.to_owned(), bytes.to_vec()).is_none(),
                "{path} written twice"
            );
            Ok(())
        },
    )?;
    Ok(out)
}

fn sources_of(files: &[SourceFile]) -> BTreeMap<String, Vec<u8>> {
    files
        .iter()
        .map(|f| (f.path.clone(), f.bytes.clone()))
        .collect()
}

fn text(out: &BTreeMap<String, Vec<u8>>, path: &str) -> String {
    String::from_utf8(
        out.get(path)
            .unwrap_or_else(|| panic!("no {path} in the output"))
            .clone(),
    )
    .unwrap()
}

/// One start tag, with its attributes, or text.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Start(String, Vec<(String, String)>),
    End(String),
    Text(String),
    Comment,
}

#[derive(Default)]
struct Sink {
    toks: RefCell<Vec<Tok>>,
}

impl TokenSink for Sink {
    type Handle = ();
    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut t = self.toks.borrow_mut();
        match token {
            Token::TagToken(tag) => {
                let name = tag.name.to_string();
                match tag.kind {
                    TagKind::StartTag => t.push(Tok::Start(
                        name,
                        tag.attrs
                            .iter()
                            .map(|a| (a.name.local.to_string(), a.value.to_string()))
                            .collect(),
                    )),
                    TagKind::EndTag => t.push(Tok::End(name)),
                }
            }
            Token::CharacterTokens(s) => match t.last_mut() {
                Some(Tok::Text(x)) => x.push_str(&s),
                _ => t.push(Tok::Text(s.to_string())),
            },
            Token::CommentToken(_) => t.push(Tok::Comment),
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

fn tokens(html: &str) -> Vec<Tok> {
    let tokenizer = Tokenizer::new(Sink::default(), TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(html.into());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.toks.into_inner()
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn has_class(attrs: &[(String, String)], class: &str) -> bool {
    attr(attrs, "class").is_some_and(|c| c.split_ascii_whitespace().any(|x| x == class))
}

/// The page's body: what the template put between its prose markers.
fn prose(page_html: &str) -> &str {
    let start = "<div class=\"prose\">\n";
    let end = "\n</div><!--/prose-->";
    let a = page_html.find(start).expect("a prose block") + start.len();
    let b = page_html.rfind(end).expect("the prose block's end");
    &page_html[a..b]
}

/// A themed body with its wrappers taken away, and how many of each:
/// the inverse of the theme pass, by the exact markup it writes.
fn unwrap_theme(html: &str) -> (String, usize, usize) {
    let mut out = String::with_capacity(html.len());
    let (mut headings, mut tables) = (0, 0);
    let mut rest = html;
    loop {
        let next_heading = rest.find("<div class=\"heading heading--");
        let next_table = rest.find("<div class=\"table-scroll\">");
        match (next_heading, next_table) {
            (None, None) => {
                out.push_str(rest);
                break;
            }
            (Some(h), t) if t.is_none_or(|t| h < t) => {
                out.push_str(&rest[..h]);
                let after = &rest[h + "<div class=\"heading heading--".len()..];
                let level = &after[..1];
                let after = after
                    .strip_prefix(&format!("{level}\">"))
                    .expect("the wrapper's level");
                let close = format!("</h{level}><a class=\"anchor\" href=\"#");
                let at = after.find(&close).expect("the heading's anchor");
                out.push_str(&after[..at]);
                out.push_str(&format!("</h{level}>"));
                let tail = &after[at + close.len()..];
                let end = tail.find("</a></div>").expect("the anchor's end");
                rest = &tail[end + "</a></div>".len()..];
                headings += 1;
            }
            (_, Some(t)) => {
                out.push_str(&rest[..t]);
                let after = &rest[t + "<div class=\"table-scroll\">".len()..];
                let end = after.find("</table></div>").expect("the table's end");
                out.push_str(&after[..end + "</table>".len()]);
                rest = &after[end + "</table></div>".len()..];
                tables += 1;
            }
            _ => unreachable!(),
        }
    }
    (out, headings, tables)
}

fn md(path: &str, body: &str) -> SourceFile {
    SourceFile::regular(path, body.as_bytes().to_vec())
}

fn page(path: &str, title: &str, body: &str) -> SourceFile {
    md(path, &format!("---\ntitle: {title}\n---\n{body}"))
}

fn small_site(files: Vec<SourceFile>) -> (Site, BTreeMap<String, Vec<u8>>) {
    let sources = sources_of(&files);
    (Site::from_sources(files, &SiteConfig::default()), sources)
}

fn build_small(files: Vec<SourceFile>) -> BTreeMap<String, Vec<u8>> {
    let (site, sources) = small_site(files);
    build_files(&site, &synthetic_cfg(), &no_history(), &sources).expect("the site builds")
}

#[test]
fn edit_link_text_is_forge_neutral() {
    let (site, sources) = small_site(vec![home(), page("notes.md", "Notes", "# Notes\n")]);
    let cfg = BuildConfig {
        edit_url: Some("https://forge.example/edit/main/".into()),
        ..BuildConfig::default()
    };
    let out = build_files(&site, &cfg, &no_history(), &sources).expect("the site builds");
    let html = text(&out, "notes.html");
    assert!(html.contains(">Edit<"));
    assert!(!html.contains("Edit in "));
}

// ---------- the layout ----------

// ---------- the body: the render, the theme pass, Wiki.js's skeleton ----------

#[test]
fn a_raw_div_from_markdown_never_survives() {
    let body = "<div class=\"table-scroll\">x</div>\n\n<div class=\"heading heading--2\"><h2 id=\"y\">Y</h2><a class=\"anchor\" href=\"#y\">¶</a></div>\n\n\
                inline <div class=\"heading\">z</div> and <span class=\"anchor\">w</span>\n\n| a |\n|---|\n| b |\n\n## Real\n";
    let out = build_small(vec![page("home.md", "Home", body)]);
    let html = text(&out, "index.html");
    let toks = tokens(prose(&html));
    let divs: Vec<&Vec<(String, String)>> = toks
        .iter()
        .filter_map(|t| match t {
            Tok::Start(n, a) if n == "div" => Some(a),
            _ => None,
        })
        .collect();
    // Exactly the theme's two: the real table's and the real heading's.
    assert_eq!(divs.len(), 2, "{html}");
    assert!(divs.iter().any(|a| has_class(a, "table-scroll")));
    assert!(divs.iter().any(|a| has_class(a, "heading")));
    let anchors = toks
        .iter()
        .filter(|t| matches!(t, Tok::Start(n, a) if n == "a" && has_class(a, "anchor")))
        .count();
    assert_eq!(anchors, 1, "only Real's anchor");
    assert!(!toks
        .iter()
        .any(|t| matches!(t, Tok::Start(n, a) if n == "h2" && attr(a, "id") == Some("y"))));
}

/// Pages that put every heading and table shape through the pass: in lists,
/// quotes and footnotes, with `@sec` markers, dropped ids, FE0F ids, code,
/// links whose titles and hrefs look like tags, raw HTML.
fn wrapper_corpus() -> Vec<String> {
    let pieces = [
        "# Title\n",
        "## Open\n",
        "## ⚠️ Warning\n",
        "### a <b>&amp;</b> \"q\" 'x'\n",
        "<!-- @sec id=a/b tags=x -->\n## Marked\n",
        "> ## In a quote\n",
        "- ## In a list\n",
        "1. | a |\n   |---|\n   | b |\n",
        "| x | y |\n|---|:-:|\n| `<h2 id=z>` | <div>t</div> |\n",
        "[l](/u \"<h2 id=x>\") and [m](</h3><table>)\n",
        "`<table>` and ```<h4>```\n",
        "```\n<h2 id=\"c\">code</h2>\n<table></table>\n```\n",
        "<details><summary>s</summary>\n\n## Inside details\n\n</details>\n",
        "Text[^1]\n\n[^1]: ## Not a heading in a footnote\n",
        "Setext\n===\n",
        "###### Six\n",
        "##\n",
        "<h2 id=\"raw\">raw heading</h2>\n",
        "<table><tr><td>raw table</td></tr></table>\n",
    ];
    let mut docs = Vec::new();
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    for _ in 0..400 {
        let mut d = String::new();
        for _ in 0..6 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            d.push_str(pieces[(seed % pieces.len() as u64) as usize]);
            d.push('\n');
        }
        docs.push(d);
    }
    docs
}

#[test]
fn every_wrapper_encloses_exactly_one_rubrica_table_or_heading() {
    for doc in wrapper_corpus() {
        let r = rubrica_render::render(&doc, &rubrica_render::RenderOptions::default());
        let themed = theme_body(&r.html, &r.headings);
        let (back, h, t) = unwrap_theme(&themed.html);
        assert_eq!(back, r.html, "{doc:?}");
        assert_eq!((h, t), (themed.headings, themed.tables), "{doc:?}");
        let with_id = r.headings.iter().filter(|h| h.id.is_some()).count();
        assert_eq!(h, with_id, "{doc:?}: every heading with an id, once");
        let toks = tokens(&themed.html);
        let tables_in = toks
            .iter()
            .filter(|t| matches!(t, Tok::Start(n, _) if n == "table"))
            .count();
        assert_eq!(t, tables_in, "{doc:?}: every table, once");
        // Each wrapper's first child is its element, and a heading's
        // anchor names the heading's own id.
        for (i, tok) in toks.iter().enumerate() {
            let Tok::Start(n, a) = tok else { continue };
            if n != "div" {
                continue;
            }
            if has_class(a, "table-scroll") {
                assert!(
                    matches!(&toks[i + 1], Tok::Start(m, _) if m == "table"),
                    "{doc:?}"
                );
            } else {
                let level = attr(a, "class")
                    .unwrap()
                    .rsplit("heading--")
                    .next()
                    .unwrap();
                let Tok::Start(m, ha) = &toks[i + 1] else {
                    panic!("{doc:?}: a wrapper without its heading")
                };
                assert_eq!(*m, format!("h{level}"), "{doc:?}");
                let id = attr(ha, "id").expect("a wrapped heading has an id");
                let close = toks[i + 1..]
                    .iter()
                    .position(|t| *t == Tok::End(m.clone()))
                    .unwrap()
                    + i
                    + 1;
                let Tok::Start(an, aa) = &toks[close + 1] else {
                    panic!("{doc:?}: no anchor after the heading")
                };
                assert_eq!(an, "a");
                assert!(has_class(aa, "anchor"));
                assert_eq!(attr(aa, "href"), Some(format!("#{id}").as_str()), "{doc:?}");
                let text = &r
                    .headings
                    .iter()
                    .find(|h| h.id.as_deref() == Some(id))
                    .unwrap()
                    .text;
                assert_eq!(
                    attr(aa, "aria-label"),
                    Some(format!("Section link: {text}").as_str())
                );
            }
        }
    }
}

#[test]
fn a_dropped_heading_is_not_wrapped_and_a_sec_span_stays_inside() {
    let doc = "## Open\n\n<!-- @sec id=x/y -->\n## Marked\n";
    let r = rubrica_render::render(doc, &rubrica_render::RenderOptions::default());
    let t = theme_body(&r.html, &r.headings);
    assert!(t.html.contains("<h2>Open</h2>"), "{}", t.html);
    assert!(!t.html.contains("href=\"#open\""));
    assert!(
        t.html.contains("<div class=\"heading heading--2\"><h2 id=\"marked\"><span class=\"sec\" id=\"sec-x/y\"></span>Marked</h2><a class=\"anchor\" href=\"#marked\" aria-label=\"Section link: Marked\">¶</a></div>"),
        "{}",
        t.html
    );
    assert_eq!((t.headings, t.tables), (1, 0));
}

#[test]
fn an_fe0f_id_is_its_anchors_fragment_as_written() {
    let r = rubrica_render::render(
        "## ⚠️ Open item\n",
        &rubrica_render::RenderOptions::wikijs_compat(),
    );
    let t = theme_body(&r.html, &r.headings);
    assert!(
        t.html.contains("<a class=\"anchor\" href=\"#\u{FE0F}-open-item\" aria-label=\"Section link: ⚠\u{FE0F} Open item\">¶</a>"),
        "{}",
        t.html
    );
}

// ---------- links: a static file server over the output ----------

// ---------- search ----------

// ---------- page chrome: edit, history, last updated, recent ----------

#[test]
fn edit_and_history_links_follow_the_configured_prefixes() {
    // A characterisation: the build already composes both links from the
    // configured prefixes; this pins it on a site of two synthetic pages.
    let (site, sources) = small_site(vec![home(), page("notes/today.md", "Today", "# Today\n")]);
    let out =
        build_files(&site, &synthetic_cfg(), &no_history(), &sources).expect("the site builds");
    let html = text(&out, "notes/today.html");
    assert!(
        html.contains("href=\"https://forge.example/edit/main/notes/today.md\">Edit<"),
        "{html}"
    );
    assert!(
        html.contains("href=\"https://forge.example/history/main/notes/today.md\">History<"),
        "{html}"
    );
    // With no prefix configured, neither link is written, nor the paragraph
    // that holds them.
    let out = build_files(&site, &BuildConfig::default(), &no_history(), &sources)
        .expect("the site builds");
    let html = text(&out, "notes/today.html");
    let meta = &html[html
        .find("<div class=\"doc-meta\">")
        .expect("the meta block")..];
    let meta = &meta[..meta.find("<div class=\"prose\">").expect("the prose block")];
    assert!(
        !html.contains(">Edit<") && !html.contains(">History<"),
        "{meta}"
    );
    assert!(!meta.contains("<p>"), "{meta}");
}

#[test]
fn last_updated_is_the_last_commit_touching_the_file_not_front_matter_date() {
    let (site, sources) = small_site(vec![md(
        "home.md",
        "---\ntitle: Home\ndate: 2020-01-01T00:00:00Z\n---\n# Home\n",
    )]);
    let mut h = no_history();
    // 2026-10-20T10:00:00-07:00.
    let touched = commit('a', 1_792_515_600, "home: edit");
    h.last.insert("home.md".into(), touched);
    let out = build_files(&site, &synthetic_cfg(), &h, &sources).unwrap();
    let html = text(&out, "index.html");
    assert!(
        html.contains("<time datetime=\"2026-10-20T10:00:00-07:00\">2026-10-20</time>"),
        "{html}"
    );
    assert!(!html.contains("2020-01-01"));
    // Without a commit for the file, there is no last-updated line.
    let out = build_files(&site, &synthetic_cfg(), &no_history(), &sources).unwrap();
    assert!(!text(&out, "index.html").contains("<time"));
}

#[test]
fn recent_lists_commits_newest_first_with_their_pages() {
    let (site, sources) = small_site(vec![
        page("home.md", "Home", "# Home\n"),
        page("alpha.md", "Alpha page", "# Alpha\n"),
        page("beta.md", "Beta page", "# Beta\n"),
    ]);
    let mut h = no_history();
    h.recent = vec![
        RecentCommit {
            commit: commit('3', 1_792_515_603, "third: beta"),
            files: vec!["beta.md".into()],
            more: 0,
        },
        RecentCommit {
            commit: commit('2', 1_792_515_602, "second <b>&</b>"),
            files: vec!["alpha.md".into(), "home.md".into()],
            more: 0,
        },
        RecentCommit {
            commit: commit('1', 1_792_515_601, "first"),
            files: vec!["home.md".into()],
            more: 0,
        },
    ];
    let out = build_files(&site, &synthetic_cfg(), &h, &sources).unwrap();
    let html = text(&out, "_recent.html");
    let third = html.find("third: beta").unwrap();
    // askama writes numeric character references.
    let second = html
        .find("second &#60;b&#62;&#38;&#60;/b&#62;")
        .expect(&html);
    let first = html.find(">first<").unwrap();
    assert!(third < second && second < first, "{html}");
    let pages_of_second = &html[second..first];
    assert!(pages_of_second.contains("<a href=\"/alpha\">Alpha page</a>"));
    assert!(pages_of_second.contains("<a href=\"/\">Home</a>"));
    assert!(html.contains("3333333"), "short ids");
}

// ---------- escaping ----------

const HOSTILE: &str =
    "\"'><script>alert(1)</script><!-- </script> &amp; \u{2028}\u{2029}\u{FEFF}e\u{301} \u{202E}x";

/// `s` as the inside of a YAML double-quoted scalar: every character not
/// printable ASCII, and `"` and `\\`, escaped.
fn yaml_dq(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '"' => "\\\"".to_owned(),
            '\\' => "\\\\".to_owned(),
            ' '..='~' => c.to_string(),
            c => format!("\\u{:04X}", c as u32),
        })
        .collect()
}

#[test]
fn hostile_strings_are_escaped_in_every_context() {
    let yaml = yaml_dq(HOSTILE);
    let src = format!(
        "---\ntitle: \"{yaml}\"\ndescription: \"{yaml}\"\ntags: \"{yaml}\"\n---\n# {h}\n\n## {h}\n\ntext\n",
        h = HOSTILE.replace('<', "&lt;")
    );
    let (site, sources) = small_site(vec![md("home.md", &src)]);
    let page = site.pages().values().next().expect("the page");
    assert_eq!(page.front.title, HOSTILE);
    let out = build_files(&site, &synthetic_cfg(), &no_history(), &sources).unwrap();
    for file in ["index.html", "_search.html", "_recent.html", "404.html"] {
        let html = text(&out, file);
        let toks = tokens(&html);
        for t in &toks {
            if let Tok::Start(n, a) = t {
                assert!(
                    n != "script" || attr(a, "src").is_some(),
                    "{file}: an inline script"
                );
                assert!(!a.iter().any(|(k, _)| k.starts_with("on")), "{file}");
            }
        }
        // The title reads back as written, in the <title> and in the
        // description's attribute.
        if file == "index.html" {
            let i = toks
                .iter()
                .position(|t| matches!(t, Tok::Start(n, _) if n == "title"))
                .unwrap();
            assert_eq!(toks[i + 1], Tok::Text(format!("{HOSTILE} · Wiki")));
            let desc = toks.iter().find_map(|t| match t {
                Tok::Start(n, a) if n == "meta" && attr(a, "name") == Some("description") => {
                    attr(a, "content").map(str::to_owned)
                }
                _ => None,
            });
            assert_eq!(desc.as_deref(), Some(HOSTILE));
        }
    }
    // The search index: no `<`, `>`, `&`, U+2028 or U+2029 as such (so it is
    // safe even inside a <script>), and every string reads back.
    let json = &out["_search.json"];
    let raw = String::from_utf8(json.clone()).unwrap();
    for c in ['<', '>', '&', '\u{2028}', '\u{2029}'] {
        assert!(!raw.contains(c), "{c:?} in _search.json");
    }
    let v: serde_json::Value = serde_json::from_slice(json).unwrap();
    assert_eq!(v[0]["title"], HOSTILE);
    assert_eq!(v[0]["description"], HOSTILE);
}

#[test]
fn the_feed_escapes_titles_and_content_as_xml() {
    let src =
        "---\ntitle: Changelog\n---\n## 0.2 <\u{1}>\n\nA & B <kbd>c</kbd>\n\n## 0.1\n\nFirst.\n";
    let (site, sources) = small_site(vec![
        page("home.md", "Home", "# Home\n"),
        md("changelog.md", src),
    ]);
    let mut cfg = synthetic_cfg();
    cfg.feed = Some(FeedConfig {
        url: "/changelog.xml".into(),
        source: "changelog.md".into(),
    });
    let mut h = no_history();
    h.feed_versions = vec![(commit('2', 1_792_515_602, "0.2"), src.as_bytes().to_vec())];
    let out = build_files(&site, &cfg, &h, &sources).unwrap();
    let xml = text(&out, "changelog.xml");
    // No character XML 1.0 refuses, and markup only as escaped text.
    assert!(
        !xml.chars()
            .any(|c| (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r')),
        "{xml:?}"
    );
    assert!(
        xml.contains("<title>Wiki 0.2 &lt;&gt;</title>")
            || xml.contains("<title>Wiki 0.2 &lt;\u{FFFD}&gt;</title>"),
        "{xml}"
    );
    assert!(
        xml.contains("A &amp;amp; B &lt;kbd&gt;c&lt;/kbd&gt;"),
        "{xml}"
    );
    assert_eq!(xml.matches("<entry>").count(), 2);
}

// ---------- the changelog feed ----------

#[test]
fn the_feed_has_one_entry_per_h2_dated_by_the_oldest_version_with_it() {
    let v1 = "---\ntitle: Changelog\n---\n## 0.1\n\nFirst.\n";
    let v2 = "---\ntitle: Changelog\n---\n## 0.2\n\nSecond.\n\n## 0.1\n\nFirst.\n";
    let v3 = "---\ntitle: Changelog\n---\n## 0.2\n\nSecond, fixed.\n\n## 0.1\n\nFirst.\n";
    let files = vec![page("home.md", "Home", "# Home\n"), md("changelog.md", v3)];
    let sources = sources_of(&files);
    // The compat slugger numbers `0.2` as `h-02`.
    let compat = SiteConfig {
        render: rubrica_render::Profile::WikijsCompat,
        ..SiteConfig::default()
    };
    let site = Site::from_sources(files, &compat);
    let mut cfg = synthetic_cfg();
    cfg.feed = Some(FeedConfig {
        url: "/changelog.xml".into(),
        source: "changelog.md".into(),
    });
    let c1 = commit('1', 1_792_000_000, "0.1");
    let c2 = commit('2', 1_792_100_000, "0.2");
    let c3 = commit('3', 1_792_200_000, "fix");
    let mut h = no_history();
    h.feed_versions = vec![
        (c3.clone(), v3.as_bytes().to_vec()),
        (c2.clone(), v2.as_bytes().to_vec()),
        (c1.clone(), v1.as_bytes().to_vec()),
    ];
    let out = build_files(&site, &cfg, &h, &sources).unwrap();
    let xml = text(&out, "changelog.xml");
    let e02 = xml
        .find("<id>https://wiki.example.org/changelog#h-02</id>")
        .expect(&xml);
    let e01 = xml
        .find("<id>https://wiki.example.org/changelog#h-01</id>")
        .unwrap();
    assert!(e02 < e01, "newest first, as written");
    assert!(
        xml[e02..e01].contains(&format!("<updated>{}</updated>", c2.iso())),
        "{xml}"
    );
    assert!(xml[e01..].contains(&format!("<updated>{}</updated>", c1.iso())));
    // The feed's own date is its newest entry's.
    let head = &xml[..xml.find("<entry>").unwrap()];
    assert!(
        head.contains(&format!("<updated>{}</updated>", c2.iso())),
        "{head}"
    );
    assert!(xml.contains("<title>Wiki 0.2</title>"));
    assert!(
        xml.contains("Second, fixed."),
        "the content is the current version's"
    );
    // Every page links it.
    assert!(text(&out, "index.html").contains(
        "<link rel=\"alternate\" type=\"application/atom+xml\" title=\"Wiki releases\" href=\"/changelog.xml\">"
    ));
}

#[test]
fn a_feed_whose_source_is_no_page_or_whose_url_is_a_page_is_refused() {
    let mut cfg = synthetic_cfg();
    cfg.feed = Some(FeedConfig {
        url: "/changelog.xml".into(),
        source: "missing.md".into(),
    });
    let (site, sources) = small_site(vec![page("home.md", "Home", "# Home\n")]);
    assert!(matches!(
        build_files(&site, &cfg, &no_history(), &sources),
        Err(BuildError::Config(_))
    ));
    cfg.feed = Some(FeedConfig {
        url: "/changelog.xml".into(),
        source: "home.md".into(),
    });
    let (site, sources) = small_site(vec![
        page("home.md", "Home", "# Home\n"),
        page("changelog.xml.md", "Shadow", "# Shadow\n"),
    ]);
    assert!(matches!(
        build_files(&site, &cfg, &no_history(), &sources),
        Err(BuildError::Config(_)) | Err(BuildError::Refused)
    ));
}

// ---------- refusing to build ----------

fn refused(files: Vec<SourceFile>) -> Vec<Finding> {
    let (site, sources) = small_site(files);
    let mut wrote = 0;
    let r = build::build(
        &site,
        &synthetic_cfg(),
        &no_history(),
        &mut |page| Ok(sources[&*page.file].clone()),
        &mut |_, _| {
            wrote += 1;
            Ok(())
        },
    );
    assert!(matches!(r, Err(BuildError::Refused)), "{r:?}");
    assert_eq!(wrote, 0, "a refused build writes nothing");
    refusals(&site).into_iter().cloned().collect()
}

fn home() -> SourceFile {
    page("home.md", "Home", "# Home\n")
}

fn kind(kind: rubrica_site::FileKind, path: &str) -> SourceFile {
    SourceFile {
        path: path.into(),
        bytes: Vec::new(),
        blob_id: None,
        kind,
    }
}

#[test]
fn every_refusing_finding_refuses_the_build() {
    use rubrica_site::FileKind;
    let deep = format!("{}x.md", "d/".repeat(70));
    let cases: Vec<(&str, Vec<SourceFile>)> = vec![
        (
            "DuplicateUrl",
            vec![home(), page("alpha.md", "A", ""), page("Alpha.md", "A", "")],
        ),
        (
            "DirectoryFileConflict",
            vec![home(), md("notes", "x"), page("notes/x.md", "X", "")],
        ),
        ("ReservedPagePath", vec![home(), page("_x.md", "X", "")]),
        (
            "IndexPageWithoutSection",
            vec![home(), page("sec/index.md", "I", "")],
        ),
        ("PathTooLong", vec![home(), md(&deep, "x")]),
        ("BadSourcePath", vec![home(), page("x /y.md", "X", "")]),
        (
            "NotARegularFile",
            vec![home(), kind(FileKind::Symlink, "linked.md")],
        ),
        (
            "FileTooLarge",
            vec![home(), kind(FileKind::TooLarge, "big.md")],
        ),
        (
            "NotUtf8",
            vec![home(), SourceFile::regular("bad.md", vec![b'-', 0xff])],
        ),
        (
            "BadFrontMatter",
            vec![home(), md("bad.md", "no front matter\n")],
        ),
        ("MdUnderAssets", vec![home(), page("assets/x.md", "X", "")]),
        (
            "AssetTooLarge",
            vec![
                home(),
                SourceFile::regular("assets/big.png", vec![0u8; (2 << 20) + 1]),
            ],
        ),
    ];
    for (name, files) in cases {
        let found = refused(files);
        assert!(
            found.iter().any(|f| format!("{f:?}").starts_with(name)),
            "{name}: {found:?}"
        );
    }
    // A site with no published page.
    let (site, sources) = small_site(vec![md(
        "draft.md",
        "---\ntitle: D\npublished: false\n---\n",
    )]);
    assert!(refusals(&site).is_empty());
    assert!(matches!(
        build_files(&site, &synthetic_cfg(), &no_history(), &sources),
        Err(BuildError::Refused)
    ));
}

#[test]
fn link_and_content_findings_do_not_refuse_the_build() {
    let out = build_small(vec![page(
        "home.md",
        "Home",
        "# Home\n\n[x](/nowhere) [y](#nope) [z](/home#nope) [r](rel)\n",
    )]);
    assert!(out.contains_key("index.html"));
}

// ---------- budgets ----------

#[test]
fn the_repository_budgets_are_errors_and_stop_all_other_work() {
    use rubrica_site::links::{ops, ops_reset};
    // Files: one over the count, each a tiny ignored path.
    let mut files: Vec<SourceFile> = (0..=rubrica_site::MAX_REPO_FILES)
        .map(|i| SourceFile::regular(format!("node_modules/{i}"), Vec::new()))
        .collect();
    files.push(home());
    ops_reset();
    let site = Site::from_sources(files, &SiteConfig::default());
    assert_eq!(ops(), 0, "no path work over the budget");
    assert!(site.pages().is_empty());
    assert!(
        matches!(
            site.findings(),
            [Finding::RepositoryOverBudget {
                what: rubrica_site::Budget::Files,
                ..
            }]
        ),
        "{:?}",
        site.findings()
    );
    // Path bytes.
    let long = "p".repeat(1000);
    let files: Vec<SourceFile> = (0..=(rubrica_site::MAX_REPO_PATH_BYTES / 1000))
        .map(|i| SourceFile::regular(format!("node_modules/{i}{long}"), Vec::new()))
        .collect();
    let site = Site::from_sources(files, &SiteConfig::default());
    assert!(matches!(
        site.findings(),
        [Finding::RepositoryOverBudget {
            what: rubrica_site::Budget::PathBytes,
            ..
        }]
    ));
    // Bytes.
    let chunk = vec![b'x'; 4 << 20];
    let files: Vec<SourceFile> = (0..=(rubrica_site::MAX_REPO_BYTES / (4 << 20)))
        .map(|i| SourceFile::regular(format!("node_modules/{i}.md"), chunk.clone()))
        .collect();
    let site = Site::from_sources(files, &SiteConfig::default());
    assert!(matches!(
        site.findings(),
        [Finding::RepositoryOverBudget {
            what: rubrica_site::Budget::Bytes,
            ..
        }]
    ));
    assert!(site.findings()[0].refuses_build());
    // At the limits, nothing is reported.
    let files: Vec<SourceFile> = (0..rubrica_site::MAX_REPO_FILES - 1)
        .map(|i| SourceFile::regular(format!("node_modules/{i}"), Vec::new()))
        .chain([home()])
        .collect();
    let site = Site::from_sources(files, &SiteConfig::default());
    assert!(site.findings().is_empty(), "{:?}", &site.findings()[..1]);
}

#[test]
fn a_reader_budget_counts_every_entry_and_byte() {
    let mut b = rubrica_site::RepoBudget::default();
    for i in 0..rubrica_site::MAX_REPO_FILES {
        assert!(b.entry(&format!("{i}")), "{i}");
    }
    assert!(!b.entry("one more"));
    let mut b = rubrica_site::RepoBudget::default();
    assert!(b.read(rubrica_site::MAX_REPO_BYTES));
    assert!(!b.read(1));
    // The git reader counts its objects itself, and records the
    // budget it stopped on.
    let mut b = rubrica_site::RepoBudget::default();
    assert!(b.holds());
    b.refuse(rubrica_site::Budget::ObjectBytes, 5, 4);
    assert!(!b.holds());
    assert!(matches!(
        b.findings()[..],
        [Finding::RepositoryOverBudget {
            what: rubrica_site::Budget::ObjectBytes,
            at_least: 5,
            limit: 4,
        }]
    ));
    // Within the entry and path budgets, any one tree git writes is within
    // the per-tree cap: at most 28 bytes an entry besides its name.
    assert_eq!(
        rubrica_site::MAX_REPO_TREE_BYTES,
        28 * rubrica_site::MAX_REPO_FILES as u64 + rubrica_site::MAX_REPO_PATH_BYTES as u64
    );
}

#[test]
fn the_asset_budget_warns_over_25_mb_and_errors_over_50_mb() {
    use rubrica_render::frontmatter::Severity;
    use rubrica_site::CheckOptions;
    let asset = vec![0u8; 2 << 20];
    let with = |n: usize| {
        let files: Vec<SourceFile> = (0..n)
            .map(|i| SourceFile::regular(format!("assets/{i}.png"), asset.clone()))
            .chain([home()])
            .collect();
        Site::from_sources(files, &SiteConfig::default())
    };
    // 11 × 2 MiB = 23.1 MB: nothing.
    assert!(with(11).findings().is_empty());
    // 12 × 2 MiB = 25.2 MB: a warning; the build goes on.
    let s = with(12);
    assert!(matches!(s.findings(), [Finding::AssetBudget { bytes }] if *bytes == 12 * (2 << 20)));
    assert_eq!(
        s.findings()[0].severity(CheckOptions::default()),
        Severity::Warning
    );
    assert!(refusals(&s).is_empty());
    // 24 × 2 MiB = 50.3 MB: an error, and no build.
    let s = with(24);
    assert_eq!(
        s.findings()[0].severity(CheckOptions::default()),
        Severity::Error
    );
    assert_eq!(refusals(&s).len(), 1);
}

// ---------- nav, size, determinism ----------

#[test]
fn the_nav_on_each_page_is_bounded() {
    let files: Vec<SourceFile> = (0..700)
        .map(|i| page(&format!("p{i:04}.md"), &format!("Page {i}"), "# P\n"))
        .chain([home()])
        .collect();
    let out = build_small(files);
    let html = text(&out, "p0001.html");
    let nav = &html[html.find("<ul class=\"tree\">").unwrap()..];
    let nav = &nav[..nav.find("</nav>").unwrap()];
    let links = nav.matches("<a ").count();
    assert!(
        links <= build::NAV_INLINE_LIMIT + 1,
        "{links} links in the nav"
    );
    assert!(
        nav.contains("href=\"/_search\""),
        "the rest is a link to every page"
    );
    // `_search.html` lists every page.
    let search = text(&out, "_search.html");
    for i in [0, 350, 699] {
        assert!(search.contains(&format!("<a href=\"/p{i:04}\">Page {i}</a>")));
    }
}

#[test]
fn build_is_deterministic() {
    // The same sources build the same files, byte for byte, from a site
    // made afresh each time.
    let make = || {
        vec![
            home(),
            page(
                "alpha.md",
                "Alpha",
                "# Alpha\n\n## Part\n\n| a |\n|---|\n| b |\n",
            ),
            page("notes/today.md", "Today", "# Today\n\n[a](/alpha#part)\n"),
        ]
    };
    let build_once = || {
        let files = make();
        let sources = sources_of(&files);
        let site = Site::from_sources(files, &SiteConfig::default());
        build_files(&site, &synthetic_cfg(), &no_history(), &sources).unwrap()
    };
    let a = build_once();
    assert!(a.contains_key("alpha.html") && a.contains_key("notes/today.html"));
    assert_eq!(a, build_once());
}

#[test]
fn health_names_the_version_and_the_commit() {
    // build/mod.rs writes `ok rubrica <the crate's version> <the commit built>`.
    let out = build_small(vec![home()]);
    assert_eq!(
        text(&out, "_health"),
        format!(
            "ok rubrica {} {}\n",
            env!("CARGO_PKG_VERSION"),
            "c".repeat(40)
        )
    );
}

#[test]
fn a_redirect_stub_is_a_redirect_page() {
    let out = build_small(vec![
        home(),
        page("new.md", "New", "# New\n\n## Part\n"),
        md(
            "old.md",
            "---\ntitle: Old\nredirect_to: /new#part\n---\nMoved to [/new](/new).\n",
        ),
    ]);
    let html = text(&out, "old.html");
    assert!(
        html.contains("<meta http-equiv=\"refresh\" content=\"0; url=/new#part\">"),
        "{html}"
    );
    assert!(html.contains("<a href=\"/new#part\">"));
}

// ---------- no script, anywhere ----------

#[test]
fn the_built_site_has_no_script_tag_no_js_file_and_a_script_src_none_csp() {
    let out = build_small(vec![
        home(),
        page(
            "notes.md",
            "Notes",
            "# Notes\n\n```mermaid\ngraph TD; A-->B\n```\n\n```bash\necho hi\n```\n",
        ),
        md(
            "old.md",
            "---\ntitle: Old\nredirect_to: /notes\n---\nMoved to [/notes](/notes).\n",
        ),
    ]);
    // With the search bundle the loader and module are the two script files, and `_search.html`
    // the one page that names a script (tests/no_js.rs audits that page and its policy); every
    // other page, and a build without the bundle, has none.
    let bundle = cfg!(feature = "search-wasm");
    for path in out.keys() {
        for ext in [".js", ".mjs", ".wasm"] {
            let loader = bundle
                && (path == "_assets/rubrica-search.js"
                    || path == "_assets/rubrica-search_bg.wasm");
            assert!(
                loader || !path.ends_with(ext),
                "!path.ends_with({ext:?}) ({path})"
            );
        }
    }
    let mut pages = 0;
    for (path, bytes) in &out {
        if !path.ends_with(".html") {
            continue;
        }
        pages += 1;
        let html = String::from_utf8(bytes.clone()).unwrap();
        if bundle && path == "_search.html" {
            continue;
        }
        assert!(!html.contains("<script"), "a script tag in {path}");
        let csp = tokens(&html)
            .into_iter()
            .find_map(|t| match t {
                Tok::Start(n, a)
                    if n == "meta" && attr(&a, "http-equiv") == Some("Content-Security-Policy") =>
                {
                    attr(&a, "content").map(str::to_owned)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no CSP meta in {path}"));
        assert!(csp.contains("script-src 'none'"), "{path}: {csp}");
    }
    assert!(pages >= 5, "{pages} pages");
    // The diagram is source in a code block.
    assert!(text(&out, "notes.html")
        .contains("<pre><code class=\"language-mermaid\">graph TD; A--&gt;B\n</code></pre>"));
    // The assets are the stylesheet and the icon (and the search bundle's two files), and nothing else.
    let assets: Vec<&str> = out
        .keys()
        .filter(|k| k.starts_with("_assets/"))
        .map(String::as_str)
        .collect();
    if bundle {
        assert_eq!(
            assets,
            [
                "_assets/icon.svg",
                "_assets/rubrica-search.js",
                "_assets/rubrica-search_bg.wasm",
                "_assets/rubrica.css"
            ]
        );
    } else {
        assert_eq!(assets, ["_assets/icon.svg", "_assets/rubrica.css"]);
    }
}

#[test]
fn search_json_has_every_page_and_heading_id() {
    // A synthetic twin of the conformance corpus's test: a characterisation, green
    // before and after the script goes.
    let out = build_small(vec![
        home(),
        page("alpha.md", "Alpha", "# Alpha\n\n## Part one\n\nbody a\n"),
        page("beta/gamma.md", "Gamma", "# Gamma\n\n## Part two\n"),
    ]);
    let v: serde_json::Value = serde_json::from_slice(&out["_search.json"]).unwrap();
    let entries = v.as_array().unwrap();
    assert_eq!(entries.len(), 3);
    let alpha = entries
        .iter()
        .find(|e| e["url"] == "/alpha")
        .expect("/alpha");
    assert_eq!(alpha["title"], "Alpha");
    let ids: Vec<(&str, &str)> = alpha["headings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| (h["id"].as_str().unwrap(), h["text"].as_str().unwrap()))
        .collect();
    assert_eq!(ids, [("alpha", "Alpha"), ("part-one", "Part one")]);
    assert!(alpha["body"].as_str().unwrap().contains("body a"));
    let gamma = entries
        .iter()
        .find(|e| e["url"] == "/beta/gamma")
        .expect("/beta/gamma");
    assert_eq!(gamma["headings"][1]["id"], "part-two");
}

/// `_search` lists every page; without the bundle it is the pages list alone, and with it the
/// search form and results come first (tests below).
#[test]
fn a_tag_with_only_the_pages_list_is_the_search_page() {
    let out = build_small(vec![
        home(),
        page("alpha.md", "Alpha", "# Alpha\n"),
        page("beta.md", "Beta", "# Beta\n"),
    ]);
    let html = text(&out, "_search.html");
    if cfg!(not(feature = "search-wasm")) {
        assert!(!html.contains("<form"), "{html}");
        assert!(!html.contains("r:status"), "{html}");
        assert!(!html.contains("r:results"), "{html}");
        assert!(
            html.contains("<h1 class=\"doc-title\">Pages</h1>"),
            "{html}"
        );
    }
    for (href, title) in [("/alpha", "Alpha"), ("/beta", "Beta")] {
        assert!(
            html.contains(&format!("<a href=\"{href}\">{title}</a>")),
            "{html}"
        );
    }
    // No page carries the header search box either.
    assert!(!text(&out, "alpha.html").contains("<form"));
}

#[cfg(not(feature = "search-wasm"))]
#[test]
fn the_search_template_has_no_script_until_the_bundle_exists() {
    // The search page's one script is the wasm bundle's loader: without the bundle (the
    // `search-wasm` feature), the page names no script at all.
    let out = build_small(vec![home(), page("alpha.md", "Alpha", "# Alpha\n")]);
    let html = text(&out, "_search.html");
    assert!(!html.to_ascii_lowercase().contains("<script"), "{html}");
}

/// With the bundle, the search page alone names the loader, once, and the build writes the
/// loader and the `.wasm` as the bundle crate holds them.
#[cfg(feature = "search-wasm")]
#[test]
fn a_built_site_serves_the_loader_and_wasm_on_the_search_page_only() {
    let (site, sources) = small_site(vec![home(), page("alpha.md", "Alpha", "# Alpha\n")]);
    let mut out: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    build::build_with_search_wasm(
        &site,
        &synthetic_cfg(),
        &no_history(),
        &mut |page| Ok(sources[&*page.file].clone()),
        &mut |path, bytes| {
            assert!(
                out.insert(path.to_owned(), bytes.to_vec()).is_none(),
                "{path} written twice"
            );
            Ok(())
        },
    )
    .expect("the site builds");
    let tag = "<script type=\"module\" src=\"/_assets/rubrica-search.js\"></script>";
    let search = text(&out, "_search.html");
    assert_eq!(search.matches(tag).count(), 1, "{search}");
    assert_eq!(
        search.to_ascii_lowercase().matches("<script").count(),
        1,
        "{search}"
    );
    for (path, bytes) in &out {
        if path.ends_with(".html") && path != "_search.html" {
            let html = String::from_utf8_lossy(bytes).to_ascii_lowercase();
            assert!(!html.contains("<script"), "a script in {path}");
        }
    }
    assert!(out.contains_key("index.html"), "no home page");
    assert_eq!(
        out.get("_assets/rubrica-search_bg.wasm").map(Vec::as_slice),
        Some(rubrica_wasm_bundle::WASM),
        "the .wasm is not the bundle's"
    );
    assert_eq!(
        out.get("_assets/rubrica-search.js").map(Vec::as_slice),
        Some(rubrica_wasm_bundle::LOADER.as_bytes()),
        "the loader is not the bundle's"
    );
}

// ---------- output amplification ----------

/// The `<ul class="tree">` of a built page.
fn nav_of(html: &str) -> &str {
    let nav = &html[html.find("<ul class=\"tree\">").unwrap()..];
    &nav[..nav.find("</nav>").unwrap()]
}

#[test]
fn long_titles_are_cut_in_the_nav_the_breadcrumbs_and_recent_changes() {
    let long = "L".repeat(2_000);
    let cut = format!("{}…", "L".repeat(build::MAX_TITLE_CHARS));
    let (site, sources) = small_site(vec![
        home(),
        page("sec.md", &long, "# S\n"),
        page("sec/leaf.md", "Leaf", "# Leaf\n"),
    ]);
    let mut h = no_history();
    h.recent = vec![RecentCommit {
        commit: commit('3', 1_792_515_603, "edit"),
        files: vec!["sec.md".into()],
        more: 0,
    }];
    let out = build_files(&site, &synthetic_cfg(), &h, &sources).unwrap();
    let leaf = text(&out, "sec/leaf.html");
    let nav = nav_of(&leaf);
    assert!(nav.contains(&format!(">{cut}</a>")), "{nav}");
    assert!(!nav.contains(&long));
    let crumbs = &leaf[leaf.find("class=\"breadcrumbs\"").unwrap()..];
    let crumbs = &crumbs[..crumbs.find("</nav>").unwrap()];
    assert!(crumbs.contains(&cut) && !crumbs.contains(&long), "{crumbs}");
    let recent = text(&out, "_recent.html");
    assert!(recent.contains(&cut) && !recent.contains(&long));
    // The page's own title, and search, keep it whole.
    let sec = text(&out, "sec.html");
    assert!(sec.contains(&format!("<title>{long} · ")));
    assert!(text(&out, "_search.html").contains(&long));
    // A title of exactly the limit is not cut.
    let exact = "E".repeat(build::MAX_TITLE_CHARS);
    let out = build_small(vec![home(), page("exact.md", &exact, "# E\n")]);
    assert!(nav_of(&text(&out, "index.html")).contains(&format!(">{exact}</a>")));
}

#[test]
fn the_nav_is_bounded_in_bytes() {
    // 300 links (under NAV_INLINE_LIMIT) of 120-character
    // titles at 900-byte paths are about 300 KB a page without the byte
    // bound.
    let dir = "d".repeat(890);
    let files: Vec<SourceFile> = (0..300)
        .map(|i| {
            page(
                &format!("{dir}/p{i:04}.md"),
                &format!("{i:04}{}", "t".repeat(116)),
                "# P\n",
            )
        })
        .chain([home(), page(&format!("{dir}.md"), "D", "# D\n")])
        .collect();
    let out = build_small(files);
    for p in ["index.html", &format!("{dir}/p0001.html")] {
        let html = text(&out, p);
        let nav = nav_of(&html);
        // At most the bound, one entry (a 120-character title and a
        // percent-encoded href) and the closing tags.
        assert!(
            nav.len() <= build::NAV_MAX_BYTES + 6 * 1024,
            "{p}: {} bytes of nav",
            nav.len()
        );
        assert!(
            nav.contains("href=\"/_search\""),
            "{p}: no link to every page"
        );
    }
}

#[test]
fn the_output_is_bounded_and_a_site_over_it_is_refused() {
    // The file that would cross the budget is not written.
    let (site, sources) = small_site(vec![home(), page("alpha.md", "A", "# A\n")]);
    let mut wrote = 0u64;
    let r = build::build_bounded(
        &site,
        &synthetic_cfg(),
        &no_history(),
        &mut |page| Ok(sources[&*page.file].clone()),
        &mut |_, bytes| {
            wrote += bytes.len() as u64;
            Ok(())
        },
        4_000,
    );
    assert!(
        matches!(r, Err(BuildError::OutputOverBudget { limit: 4_000 })),
        "{r:?}"
    );
    assert!(wrote <= 4_000, "{wrote} bytes written");
    // The default budget is 2 GiB.
    assert_eq!(build::MAX_OUTPUT_BYTES, 2 << 30);
    assert!(build_small(vec![home()]).contains_key("index.html"));
}

#[test]
fn the_output_budget_charges_each_file_at_least_4096_bytes() {
    // A file costs at least a block of disk, whatever its
    // length, so many tiny files cannot pass the budget's intent.
    let (site, sources) = small_site(vec![home(), page("alpha.md", "A", "# A\n")]);
    let mut lens = Vec::new();
    build::build(
        &site,
        &synthetic_cfg(),
        &no_history(),
        &mut |page| Ok(sources[&*page.file].clone()),
        &mut |_, bytes| {
            lens.push(bytes.len() as u64);
            Ok(())
        },
    )
    .unwrap();
    let bytes: u64 = lens.iter().sum();
    let charged: u64 = lens.iter().map(|&n| n.max(4096)).sum();
    assert!(lens.iter().any(|&n| n < 4096) && charged > bytes);
    let run = |limit: u64| {
        build::build_bounded(
            &site,
            &synthetic_cfg(),
            &no_history(),
            &mut |page| Ok(sources[&*page.file].clone()),
            &mut |_, _| Ok(()),
            limit,
        )
    };
    // Its bytes alone fit; charged a block a file, it does not.
    assert!(
        matches!(run(bytes), Err(BuildError::OutputOverBudget { .. })),
        "a tiny file was charged its length"
    );
    assert!(matches!(
        run(charged - 1),
        Err(BuildError::OutputOverBudget { .. })
    ));
    // What it reports is still the bytes written.
    assert_eq!(run(charged).unwrap().bytes, bytes);
}

/// Whether every bidi embedding, override and isolate `s` opens is closed
/// in it (UAX #9's matching: a PDF ends the innermost embedding of the
/// current isolate, a PDI the innermost isolate and what is open in it).
fn bidi_closed(s: &str) -> bool {
    let mut open: Vec<char> = Vec::new();
    for c in s.chars() {
        match c {
            '\u{202A}' | '\u{202B}' | '\u{202D}' | '\u{202E}' => open.push('E'),
            '\u{202C}' => {
                if open.last() == Some(&'E') {
                    open.pop();
                }
            }
            '\u{2066}'..='\u{2068}' => open.push('I'),
            '\u{2069}' => {
                if let Some(at) = open.iter().rposition(|&o| o == 'I') {
                    open.truncate(at);
                }
            }
            _ => {}
        }
    }
    open.is_empty()
}

#[test]
fn a_cut_title_closes_the_isolates_and_embeddings_it_cut() {
    // A title cut inside an isolate or an embedding would
    // leave it open over the `…` and whatever follows on the line.
    let openers = [
        '\u{2066}', '\u{2067}', '\u{2068}', '\u{202A}', '\u{202B}', '\u{202D}', '\u{202E}',
    ];
    for opener in openers {
        for (pre, inner) in [
            (100, "\u{2067}\u{202B}"),
            (110, ""),
            (60, "\u{2069}\u{202C}"),
        ] {
            let title = format!("{}{opener}{inner}{}", "a".repeat(pre), "\u{628}".repeat(80));
            assert!(title.chars().count() > build::MAX_TITLE_CHARS);
            let (site, sources) = small_site(vec![
                home(),
                page("sec.md", &title, "# S\n"),
                page("sec/leaf.md", "Leaf", "# Leaf\n"),
            ]);
            let mut h = no_history();
            h.recent = vec![RecentCommit {
                commit: commit('3', 1_792_515_603, "edit"),
                files: vec!["sec.md".into()],
                more: 0,
            }];
            let out = build_files(&site, &synthetic_cfg(), &h, &sources).unwrap();
            let mut seen = 0;
            for f in ["sec/leaf.html", "_recent.html"] {
                let html = text(&out, f);
                for (at, _) in html.match_indices("href=\"/sec\"") {
                    let a = &html[at..];
                    let shown = &a[a.find('>').unwrap() + 1..a.find("</a>").unwrap()];
                    assert!(shown.ends_with('…'), "{f}: {shown:?}");
                    assert!(bidi_closed(shown), "{f}: {opener:?} left open in {shown:?}");
                    seen += 1;
                }
            }
            assert!(
                seen >= 3,
                "the nav, the breadcrumbs and recent changes: {seen}"
            );
        }
    }
    // A title the cut does not reach is shown as written.
    let whole = format!("a\u{2067}{}", "b".repeat(50));
    let out = build_small(vec![home(), page("whole.md", &whole, "# W\n")]);
    assert!(nav_of(&text(&out, "index.html")).contains(&format!(">{whole}</a>")));
}

#[test]
fn a_feed_heading_is_dated_by_its_earliest_version() {
    // Versions come newest first in the walk's order, which
    // is not committer time's when a branch's clock is behind. A heading
    // is dated by the earliest commit time among the versions with it.
    let with = "---\ntitle: Changelog\n---\n## v4\n\nFour.\n\n## v3\n\nThree.\n";
    let without = "---\ntitle: Changelog\n---\n## v3\n\nThree.\n";
    let (site, sources) = small_site(vec![home(), md("changelog.md", with)]);
    let mut cfg = synthetic_cfg();
    cfg.feed = Some(FeedConfig {
        url: "/changelog.xml".into(),
        source: "changelog.md".into(),
    });
    let merge = commit('5', 1_792_001_000, "merge");
    let a1 = commit('4', 1_792_000_400, "v4 on main");
    let s2 = commit('3', 1_792_000_200, "after v4 on the side");
    let s1 = commit('2', 1_792_000_900, "v4 on the side");
    let root = commit('1', 1_792_000_100, "v3");
    let mut h = no_history();
    h.feed_versions = [&merge, &a1, &s2, &s1]
        .into_iter()
        .map(|c| (c.clone(), with.as_bytes().to_vec()))
        .chain([(root.clone(), without.as_bytes().to_vec())])
        .collect();
    let out = build_files(&site, &cfg, &h, &sources).unwrap();
    let xml = text(&out, "changelog.xml");
    let v4 = &xml[xml.find("changelog#v4</id>").expect(&xml)..];
    let v4 = &v4[..v4.find("</entry>").unwrap()];
    assert!(
        v4.contains(&format!("<updated>{}</updated>", s2.iso())),
        "{v4}"
    );
    let v3 = &xml[xml.find("changelog#v3</id>").unwrap()..];
    assert!(v3.contains(&format!("<updated>{}</updated>", root.iso())));
}

#[test]
fn recent_changes_counts_the_pages_it_leaves_out() {
    let (site, sources) = small_site(vec![home(), page("alpha.md", "A", "# A\n")]);
    let mut h = no_history();
    h.recent = vec![RecentCommit {
        commit: commit('3', 1_792_515_603, "edit"),
        files: vec!["alpha.md".into()],
        more: 7,
    }];
    let out = build_files(&site, &synthetic_cfg(), &h, &sources).unwrap();
    assert!(text(&out, "_recent.html").contains("and 7 more"));
}

#[test]
fn a_site_with_no_home_page_is_refused() {
    // Refused, as no published page is.
    let (site, sources) = small_site(vec![page("alpha.md", "A", "# A\n")]);
    assert!(refusals(&site).is_empty());
    assert!(matches!(
        build_files(&site, &synthetic_cfg(), &no_history(), &sources),
        Err(BuildError::Refused)
    ));
}

#[test]
fn well_known_names_are_reserved() {
    // A page `robots.txt` would answer `/robots.txt` (`try_files`).
    let (site, _) = small_site(vec![
        home(),
        page("robots.txt.md", "R", ""),
        page("Favicon.ICO.md", "F", ""),
        page(".well-known/security.txt.md", "W", ""),
        page("docs/robots.txt.md", "Nested", ""),
    ]);
    let reserved: Vec<String> = site
        .findings()
        .iter()
        .filter_map(|f| match f {
            Finding::ReservedPagePath { file, why } => {
                assert_eq!(*why, rubrica_site::Reserved::WellKnown, "{file}");
                Some(file.to_string())
            }
            _ => None,
        })
        .collect();
    assert_eq!(reserved, ["Favicon.ICO.md", "robots.txt.md"]);
    // `.well-known/` is a dot directory: never part of the site.
    assert!(site
        .pages()
        .keys()
        .all(|p| !p.as_str().contains("well-known")));
    assert!(site.pages().keys().any(|p| p.as_str() == "docs/robots.txt"));
}
