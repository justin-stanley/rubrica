//! The corpus model on small synthetic corpora: URL rules, nav,
//! breadcrumbs, TOC, link resolution, anchors and their suggestions,
//! redirects, and the findings `check` will report.

use rubrica_render::frontmatter::Severity;
use rubrica_site::{
    AnchorProblem, CheckOptions, FileKind, Finding, NavEntry, NoPage, Origin, PagePath,
    RenderCache, RenderKey, RenderResult, Site, SiteConfig, SourceFile, Target,
};

fn file(path: &str, src: &str) -> SourceFile {
    SourceFile::regular(path.to_owned(), src.as_bytes().to_vec())
}

/// A page file with a title and a body.
fn md(path: &str, title: &str, body: &str) -> SourceFile {
    file(path, &format!("---\ntitle: {title}\n---\n{body}"))
}

fn site(files: Vec<SourceFile>) -> Site {
    Site::from_sources(files, &SiteConfig::default())
}

/// The configuration the Wiki.js-era tests below were written against:
/// `scripts/` ignored and the wiki's own origin set; the neutral default
/// otherwise. Only the tests that name it depend on either.
fn wikijs_cfg() -> SiteConfig {
    SiteConfig {
        ignore_dirs: vec!["scripts".to_owned(), "node_modules".to_owned()],
        wiki_origin: Some(Origin::new("https://wiki.example.org").expect("an origin")),
        ..SiteConfig::default()
    }
}

fn site_cfg(cfg: SiteConfig, files: Vec<SourceFile>) -> Site {
    Site::from_sources(files, &cfg)
}

fn p(s: &str) -> PagePath {
    PagePath::new(s).unwrap()
}

fn urls(site: &Site) -> Vec<(String, String)> {
    site.pages()
        .values()
        .map(|pg| (pg.path.to_string(), pg.url.clone()))
        .collect()
}

#[test]
fn url_rules_home_is_the_root_and_paths_drop_md() {
    let s = site(vec![
        md("home.md", "Home", ""),
        md("abc.md", "A", ""),
        md("abc/def.md", "B", ""),
        md("abc/home.md", "Not home", ""),
        md("xyz/y/z.md", "Z", ""),
    ]);
    assert_eq!(
        urls(&s),
        [
            ("abc", "/abc"),
            ("abc/def", "/abc/def"),
            ("abc/home", "/abc/home"),
            ("home", "/"),
            ("xyz/y/z", "/xyz/y/z")
        ]
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
    );
    assert!(s.findings().is_empty(), "{:?}", s.findings());
}

#[test]
fn ignored_files_and_directories_are_not_pages() {
    let s = site_cfg(
        wikijs_cfg(),
        vec![
            md("home.md", "Home", ""),
            md("README.md", "Readme", ""),
            md("scripts/x.md", "S", ""),
            md("a/scripts/x.md", "S", ""),
            md(".github/w.md", "W", ""),
            md(".githooks/h.md", "H", ""),
            md("node_modules/m/README.md", "N", ""),
            md("a/.hidden.md", "H", ""),
            file("packages.yaml", "members: []\n"),
            file(".githooks/pre-push", "#!/bin/sh\n"),
            file("notes.txt", "x"),
        ],
    );
    assert_eq!(urls(&s), [("home".to_owned(), "/".to_owned())]);
    assert!(s.findings().is_empty(), "{:?}", s.findings());
    assert!(s.assets().is_empty());
}

#[test]
fn a_same_named_md_is_the_sections_index() {
    let s = site(vec![
        md("home.md", "Home", ""),
        md("architecture.md", "Architecture", ""),
        md("architecture/topology.md", "Topology", ""),
        md("lonely.md", "Lonely", ""),
    ]);
    let topo = s.page(&p("architecture/topology")).unwrap();
    assert_eq!(topo.section, Some(p("architecture")));
    assert_eq!(s.page(&p("architecture")).unwrap().section, None);
    assert_eq!(s.page(&p("lonely")).unwrap().section, None);
    let root = &s.nav().root;
    assert_eq!(root.index, Some(p("home")));
    let arch = root
        .entries
        .iter()
        .find_map(|e| match e {
            NavEntry::Section(sec) if sec.dir == "architecture" => Some(sec),
            _ => None,
        })
        .expect("architecture is a section");
    assert_eq!(arch.index, Some(p("architecture")));
    assert_eq!(&*arch.title, "Architecture");
    // The index is the section's own link, not also an entry beside it.
    assert!(!root
        .entries
        .iter()
        .any(|e| matches!(e, NavEntry::Page { path, .. } if path.as_str() == "architecture")));
}

#[test]
fn nav_tree_lists_every_page_once_under_its_directory() {
    let s = site(vec![
        md("home.md", "Home", ""),
        md("bbb.md", "Beta", ""),
        md("aaa.md", "alpha", ""),
        md("zzz/q.md", "Q", ""),
        md("zzz/p.md", "P", ""),
        md("zzz/deep/er/x.md", "X", ""),
        md("ccc.md", "C", ""),
        md("ccc/d.md", "D", ""),
    ]);
    let listed: Vec<String> = s.nav().pages().iter().map(|p| p.to_string()).collect();
    let mut sorted = listed.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        listed.len(),
        "a page listed twice: {listed:?}"
    );
    let all: Vec<String> = s.pages().keys().map(|p| p.to_string()).collect();
    assert_eq!(sorted, all);
    // Order: home is the root's index; then entries by title,
    // case-insensitively (`alpha` before `Beta`), sections among them.
    assert_eq!(
        listed,
        [
            "home",
            "aaa",
            "bbb",
            "ccc",
            "ccc/d",
            "zzz/p",
            "zzz/q",
            "zzz/deep/er/x"
        ]
    );
    let root = &s.nav().root;
    let titles: Vec<&str> = root
        .entries
        .iter()
        .map(|e| match e {
            NavEntry::Page { title, .. } => title.as_str(),
            NavEntry::Section(sec) => &*sec.title,
        })
        .collect();
    assert_eq!(titles, ["alpha", "Beta", "C", "zzz"]);
    // Every page sits under its own directory.
    fn check(sec: &rubrica_site::NavSection) {
        for e in &sec.entries {
            match e {
                NavEntry::Page { path, .. } => assert_eq!(path.dir(), sec.dir, "{path}"),
                NavEntry::Section(sub) => {
                    assert_eq!(sub.dir.rsplit_once('/').map_or("", |(a, _)| a), sec.dir);
                    check(sub);
                }
            }
        }
    }
    check(root);
    // Sections: root, c, z, z/deep, z/deep/er; pages: 8.
    assert_eq!(s.nav().len(), 5 + 8);
}

#[test]
fn breadcrumbs_follow_the_path() {
    let s = site(vec![
        md("home.md", "Home", ""),
        md("sec.md", "Section A", ""),
        md("sec/b/c.md", "C", ""),
    ]);
    let c = s.page(&p("sec/b/c")).unwrap();
    let crumbs: Vec<(&str, &str, Option<String>)> = c
        .breadcrumbs
        .iter()
        .map(|k| (&*k.dir, &*k.title, k.page.as_ref().map(|p| p.to_string())))
        .collect();
    assert_eq!(
        crumbs,
        [
            ("", "Home", Some("home".to_owned())),
            ("sec", "Section A", Some("sec".to_owned())),
            ("sec/b", "b", None),
        ]
    );
    assert!(s.page(&p("home")).unwrap().breadcrumbs.is_empty());
    assert_eq!(s.page(&p("sec")).unwrap().breadcrumbs.len(), 1);
}

#[test]
fn toc_nests_every_heading_by_level() {
    let s = site(vec![md(
        "home.md",
        "Home",
        "# One\n### Skips a level\n## Two\n#### Deep\n# Three\n",
    )]);
    let toc = &s.page(&p("home")).unwrap().toc;
    assert_eq!(toc.len(), 2);
    assert_eq!(toc[0].title, "One");
    let kids: Vec<&str> = toc[0].children.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(kids, ["Skips a level", "Two"]);
    assert_eq!(toc[0].children[1].children[0].title, "Deep");
    assert_eq!(toc[0].line, 4, "file lines, after the front matter");
    assert_eq!(toc[1].href().as_deref(), Some("#three"));
}

#[test]
fn toc_entry_for_a_dropped_id_has_no_href() {
    let s = site(vec![md("home.md", "Home", "# Page\n## Open\n## Open\n")]);
    let toc = &s.page(&p("home")).unwrap().toc;
    let kids = &toc[0].children;
    assert_eq!(kids[0].title, "Open");
    assert_eq!(kids[0].id, None);
    assert_eq!(kids[0].href(), None);
    assert_eq!(kids[1].href().as_deref(), Some("#open-1"));
}

fn anchors(s: &Site) -> Vec<(String, usize, String, Option<String>)> {
    s.findings()
        .iter()
        .filter_map(|f| match f {
            Finding::BrokenAnchor {
                file, line, href, ..
            } => Some((file.to_string(), *line, href.clone(), f.fix())),
            _ => None,
        })
        .collect()
}

#[test]
fn anchor_check_uses_the_target_pages_ids() {
    let s = site(vec![
        md("xxx.md", "X", "# Title here\n## y\n"),
        md(
            "home.md",
            "Home",
            "## z\n[ok](/xxx#y) [bad](/xxx#z) [own](#z) [own bad](#y)\n",
        ),
    ]);
    assert_eq!(
        anchors(&s),
        [
            ("home.md".to_owned(), 5, "/xxx#z".to_owned(), None),
            ("home.md".to_owned(), 5, "#y".to_owned(), None),
        ]
    );
    let same: Vec<bool> = s
        .findings()
        .iter()
        .filter_map(|f| match f {
            Finding::BrokenAnchor { same_page, .. } => Some(*same_page),
            _ => None,
        })
        .collect();
    assert_eq!(same, [false, true]);
}

#[test]
fn an_anchor_reaches_a_sec_span_and_the_top() {
    let s = site(vec![
        md("xxx.md", "X", "<!-- @sec id=a/b -->\n## Head\n"),
        md(
            "home.md",
            "Home",
            "[s](/xxx#sec-a/b) [t](/xxx#top) [T](/xxx#TOP) [e](/xxx#) [enc](/xxx#sec-a%2Fb) [no](/xxx#sec-a)\n",
        ),
    ]);
    assert_eq!(
        anchors(&s),
        [("home.md".to_owned(), 4, "/xxx#sec-a".to_owned(), None)]
    );
}

#[test]
fn suggestion_uses_the_loose_match() {
    // The compat slugger's `h-` numbering and its keeping of U+FE0F are the subject.
    let compat = SiteConfig {
        render: rubrica_render::Profile::WikijsCompat,
        ..SiteConfig::default()
    };
    let s = site_cfg(compat, vec![
        md(
            "ttt.md",
            "T",
            "# T\n## Capacity & memory\n## 4. NAS config backup\n## ⚠️ Open item\n## Intro\nx\n## Setup\n",
        ),
        md(
            "home.md",
            "Home",
            "[a](/ttt#capacity--memory)\n[b](/ttt#4-nas-config-backup)\n[c](/ttt#open-item)\n[d](/ttt#L9)\n[e](/ttt#L2)\n[f](/ttt#nothing-like-it)\n",
        ),
    ]);
    assert_eq!(
        anchors(&s),
        [
            (
                "home.md".to_owned(),
                4,
                "/ttt#capacity--memory".to_owned(),
                Some("/ttt#capacity-memory".to_owned())
            ),
            (
                "home.md".to_owned(),
                5,
                "/ttt#4-nas-config-backup".to_owned(),
                Some("/ttt#h-4-nas-config-backup".to_owned())
            ),
            (
                "home.md".to_owned(),
                6,
                "/ttt#open-item".to_owned(),
                Some("/ttt#%EF%B8%8F-open-item".to_owned())
            ),
            // Line 9 of ttt.md is under "## Intro" (line 8).
            (
                "home.md".to_owned(),
                7,
                "/ttt#L9".to_owned(),
                Some("/ttt#intro".to_owned())
            ),
            // Line 2 is above every heading but the first h1: drop the fragment.
            (
                "home.md".to_owned(),
                8,
                "/ttt#L2".to_owned(),
                Some("/ttt".to_owned())
            ),
            (
                "home.md".to_owned(),
                9,
                "/ttt#nothing-like-it".to_owned(),
                None
            ),
        ]
    );
}

#[test]
fn a_link_to_a_dropped_id_says_so_with_no_fix() {
    let s = site(vec![md("home.md", "Home", "## Open\n[x](#open)\n")]);
    match s.findings() {
        [f @ Finding::BrokenAnchor {
            why: AnchorProblem::DroppedId { text },
            ..
        }] => {
            assert_eq!(&**text, "Open");
            assert_eq!(f.fix(), None);
        }
        other => panic!("{other:?}"),
    }
}

fn targets(s: &Site, page: &str) -> Vec<Target> {
    s.page(&p(page))
        .unwrap()
        .links
        .iter()
        .map(|l| l.target.clone())
        .collect()
}

#[test]
fn links_are_resolved_as_a_browser_and_wikijs_resolve_them() {
    let s = site_cfg(wikijs_cfg(), vec![
        md("home.md", "Home", ""),
        md("abc/def.md", "B", ""),
        md("abc def.md", "Spaced", ""),
        file("assets/i.png", "png"),
        md(
            "xyz.md",
            "X",
            concat!(
                "[1](/abc/def) [2](/abc/def/) [3](/en/abc/def) [4](/abc/./c/../def) [5](/abc%2Fdef) [6](/home) [7](/)\n",
                "[8](/t/ops) [9](/a) [10](//host/abc/def) [11](https://wiki.example.org/abc/def#x)\n",
                "[12](/abc%20def) [13](</abc def>) [14](/assets/i.png) [15](/assets/j.png) [16](/abc/def.md)\n",
                "[17](/abc/def?q=1) [18](abc/def) [19](/../../../abc/def) [20](/en/) [21](/a%FF) [22](/ABC/DEF)\n",
            ),
        ),
    ]);
    let page = |s: &str, f: Option<&str>| Target::Page {
        path: p(s),
        fragment: f.map(str::to_owned),
    };
    let none = |s: &str| Target::NoPage {
        path: s.to_owned(),
        why: NoPage::Missing,
    };
    assert_eq!(
        targets(&s, "xyz"),
        [
            page("abc/def", None),
            page("abc/def", None),
            page("abc/def", None),
            page("abc/def", None),
            page("abc/def", None),
            page("home", None),
            page("home", None),
            Target::System {
                path: "/t/ops".to_owned()
            },
            Target::System {
                path: "/a".to_owned()
            },
            Target::External,
            page("abc/def", Some("x")),
            page("abc def", None),
            page("abc def", None),
            Target::Asset {
                path: "assets/i.png".to_owned(),
                exists: true
            },
            Target::Asset {
                path: "assets/j.png".to_owned(),
                exists: false
            },
            Target::Asset {
                path: "abc/def.md".to_owned(),
                exists: true
            },
            page("abc/def", None),
            Target::Relative {
                page: Some(p("abc/def")),
                file: Some("abc/def.md".into()),
            },
            page("abc/def", None),
            Target::NoPage {
                path: "/en/".to_owned(),
                why: NoPage::Invalid(rubrica_site::BadPath::EmptySegment)
            },
            Target::NoPage {
                path: "/a%FF".to_owned(),
                why: NoPage::Invalid(rubrica_site::BadPath::NotUtf8)
            },
            none("ABC/DEF"),
        ]
    );
}

#[test]
fn a_link_to_an_unpublished_page_is_a_broken_page_link_and_a_warning() {
    let s = site(vec![
        file("draft.md", "---\ntitle: D\npublished: false\n---\n"),
        file("bad.md", "no front matter\n"),
        md("home.md", "Home", "[d](/draft) [b](/bad) [m](/missing)\n"),
    ]);
    assert!(s.page(&p("draft")).is_none());
    let broken: Vec<(String, NoPage, bool)> = s
        .findings()
        .iter()
        .filter_map(|f| match f {
            Finding::BrokenPageLink { path, why, .. } => Some((
                path.clone(),
                why.clone(),
                f.severity(CheckOptions::default()) == Severity::Warning,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        broken,
        [
            ("draft".to_owned(), NoPage::Unpublished, true),
            ("bad".to_owned(), NoPage::BadFrontMatter, false),
            ("missing".to_owned(), NoPage::Missing, false),
        ]
    );
    assert!(s
        .findings()
        .iter()
        .any(|f| matches!(f, Finding::BadFrontMatter { file, .. } if &**file == "bad.md")));
}

#[test]
fn a_relative_link_is_a_finding_only_when_it_names_a_page() {
    // check-frontmatter.cjs's rule (lines 151-170): a relative link that
    // reaches an existing `.md` file; anything else (`nope`, a source file,
    // one climbing out to a file that is not there) is not a finding.
    let s = site(vec![
        md("abc/b.md", "B", ""),
        md(
            "abc/c.md",
            "C",
            "[1](b) [2](b.md#x) [3](../abc/b?q) [4](nope) [5](../../b) [6](./) [7](x.ts)\n",
        ),
    ]);
    let rel: Vec<(String, Option<String>, Severity)> = s
        .findings()
        .iter()
        .filter_map(|f| match f {
            Finding::RelativeLink { href, .. } => {
                Some((href.clone(), f.fix(), f.severity(CheckOptions::default())))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        rel,
        [
            ("b".to_owned(), Some("/abc/b".to_owned()), Severity::Error),
            (
                "b.md#x".to_owned(),
                Some("/abc/b#x".to_owned()),
                Severity::Error
            ),
            (
                "../abc/b?q".to_owned(),
                Some("/abc/b?q".to_owned()),
                Severity::Error
            ),
        ]
    );
    assert_eq!(s.findings().len(), 3, "{:?}", s.findings());
}

#[test]
fn protocol_relative_and_scheme_links_are_external_and_unchecked() {
    let s = site(vec![md(
        "home.md",
        "Home",
        "[a](//nope/x) [b](https://e.com/x#nope) [c](mailto:a@b.c) [d](ftp://x)\n",
    )]);
    assert!(targets(&s, "home").iter().all(|t| *t == Target::External));
    assert!(s.findings().is_empty(), "{:?}", s.findings());
}

#[test]
fn backlinks_are_the_inverse_of_internal_links() {
    let s = site(vec![
        md(
            "home.md",
            "Home",
            "[a](/aaa) [a again](/aaa#x) [b](/bbb)\n# X\n",
        ),
        md("aaa.md", "A", "[b](/bbb) [self](/aaa) [home](/)\n"),
        md("bbb.md", "B", "[gone](/gone)\n"),
    ]);
    let names = |v: &[PagePath]| v.iter().map(|p| p.to_string()).collect::<Vec<_>>();
    assert_eq!(names(s.backlinks(&p("aaa"))), ["aaa", "home"]);
    assert_eq!(names(s.backlinks(&p("bbb"))), ["aaa", "home"]);
    assert_eq!(names(s.backlinks(&p("home"))), ["aaa"]);
    for page in s.pages().values() {
        for l in &page.links {
            if let Target::Page { path, .. } = &l.target {
                assert!(s.backlinks(path).contains(&page.path));
            }
        }
    }
}

#[test]
fn a_duplicate_sec_id_across_pages_names_both_files() {
    let s = site(vec![
        md("aaa.md", "A", "<!-- @sec id=x/y -->\n## H\n"),
        md("bbb.md", "B", "x\n<!-- @sec id=x/y -->\n## H\n"),
    ]);
    let dups: Vec<&Finding> = s
        .findings()
        .iter()
        .filter(|f| matches!(f, Finding::DuplicateSecId { .. }))
        .collect();
    assert_eq!(
        dups,
        [&Finding::DuplicateSecId {
            file: "bbb.md".into(),
            line: 5,
            id: "x/y".to_owned(),
            first_file: "aaa.md".into(),
            first_line: 4,
        }]
    );
}

#[test]
fn render_findings_are_reported_at_file_lines() {
    let s = site(vec![md(
        "aaa.md",
        "A",
        "x\n<!-- @sec id=q -->\n- not a heading\n",
    )]);
    match s.findings() {
        [Finding::Render { file, line, .. }] => assert_eq!((&**file, *line), ("aaa.md", 5)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn urls_that_differ_only_in_case_or_normalisation_are_duplicates() {
    let s = site(vec![
        md("Guide.md", "G", ""),
        md("guide.md", "g", ""),
        md("caf\u{e9}.md", "C", ""),
        md("cafe\u{301}.md", "C", ""),
    ]);
    let dups: Vec<(String, String)> = s
        .findings()
        .iter()
        .filter_map(|f| match f {
            Finding::DuplicateUrl { file, other } => Some((file.to_string(), other.to_string())),
            _ => None,
        })
        .collect();
    assert_eq!(
        dups,
        [
            ("caf\u{e9}.md".to_owned(), "cafe\u{301}.md".to_owned()),
            ("guide.md".to_owned(), "Guide.md".to_owned())
        ]
    );
}

#[test]
fn reserved_and_bad_paths_are_not_pages() {
    let s = site(vec![
        md("_recent.md", "R", ""),
        md("t/x.md", "T", ""),
        md("t.md", "T", ""),
        md("en/x.md", "E", ""),
        md("en.md", "E", ""),
        md("assets/x.md", "A", ""),
        md("ab-cd/x.md", "Locale", ""),
        md("abc\\b.md", "B", ""),
        md("abc?b.md", "Q", ""),
        md(".md", "Dot", ""),
        md("abc/..md", "Dots", ""),
        md("abc//d.md", "Empty", ""),
        file("xxx.md", "\u{FFFD}"),
        SourceFile::regular("yyy.md".to_owned(), vec![0xff, 0xfe]),
        md("okay.md", "Ok", ""),
    ]);
    assert_eq!(urls(&s), [("okay".to_owned(), "/okay".to_owned())]);
    let kinds: Vec<String> = s
        .findings()
        .iter()
        .map(|f| {
            let k = format!("{f:?}");
            format!("{}:{}", f.file(), &k[..k.find(' ').unwrap_or(k.len())])
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "_recent.md:ReservedPagePath",
            "ab-cd/x.md:ReservedPagePath",
            "abc//d.md:BadSourcePath",
            "abc?b.md:BadSourcePath",
            "abc\\b.md:BadSourcePath",
            "assets/x.md:MdUnderAssets",
            "en.md:ReservedPagePath",
            "en/x.md:ReservedPagePath",
            "t.md:ReservedPagePath",
            "t/x.md:ReservedPagePath",
            "xxx.md:BadFrontMatter",
            "yyy.md:NotUtf8",
        ]
    );
}

#[test]
fn redirect_stubs_resolve_and_links_to_them_are_findings() {
    let s = site(vec![
        md("new.md", "New", "## Sec\n"),
        file(
            "old.md",
            "---\ntitle: Old\nredirect_to: /new\n---\nMoved to [/new](/new).\n",
        ),
        file("older.md", "---\ntitle: Older\nredirect_to: /old\n---\n"),
        file("lost.md", "---\ntitle: Lost\nredirect_to: /gone\n---\n"),
        md("home.md", "Home", "[o](/old#sec)\n"),
    ]);
    assert_eq!(
        s.redirects()
            .iter()
            .map(|(a, b)| (a.to_string(), b.url()))
            .collect::<Vec<_>>(),
        [("old".to_owned(), "/new".to_owned())]
    );
    assert_eq!(
        s.page(&p("old"))
            .unwrap()
            .redirect_to
            .as_ref()
            .map(|r| &r.to),
        Some(&p("new"))
    );
    let got: Vec<String> = s.findings().iter().map(|f| format!("{f:?}")).collect();
    assert_eq!(got.len(), 3, "{got:#?}");
    assert!(
        matches!(&s.findings()[0], f @ Finding::LinkToRedirectStub { file, .. } if &**file == "home.md" && f.fix().as_deref() == Some("/new#sec"))
    );
    assert!(
        matches!(&s.findings()[1], Finding::RedirectTargetMissing { file, line: 3, .. } if &**file == "lost.md")
    );
    assert!(
        matches!(&s.findings()[2], Finding::RedirectChain { file, target, .. } if &**file == "older.md" && target.as_str() == "old")
    );
}

#[test]
fn assets_are_files_under_assets() {
    let s = site(vec![
        file("assets/a/b.png", "png"),
        file("assets/c.pdf", "pdf"),
        md(
            "home.md",
            "Home",
            "[pdf](/assets/c.pdf) [missing](/assets/d.pdf)\n",
        ),
    ]);
    let paths: Vec<&str> = s.assets().iter().map(|a| a.path.as_str()).collect();
    assert_eq!(paths, ["assets/a/b.png", "assets/c.pdf"]);
    match s.findings() {
        [f @ Finding::BrokenAssetLink { href, .. }] => {
            assert_eq!(href, "/assets/d.pdf");
            assert_eq!(f.severity(CheckOptions::default()), Severity::Warning);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn wikijs_limits_add_the_path_length_rule() {
    let long = "a".repeat(256);
    let files = vec![md(&format!("{long}.md"), "L", "")];
    assert!(Site::from_sources(files.clone(), &SiteConfig::default())
        .findings()
        .is_empty());
    let cfg = SiteConfig {
        wikijs_limits: true,
        ..SiteConfig::default()
    };
    match Site::from_sources(files, &cfg).findings() {
        [Finding::PathTooLongForWikijs { chars: 256, .. }] => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_same_path_twice_keeps_the_first() {
    let s = site(vec![md("aaa.md", "First", ""), md("aaa.md", "Second", "")]);
    assert_eq!(s.page(&p("aaa")).unwrap().front.title, "First");
    assert!(matches!(s.findings(), [Finding::DuplicateUrl { .. }]));
}

#[test]
fn a_home_directory_has_no_index_and_home_is_listed_once() {
    let s = site(vec![
        md("home.md", "Home", ""),
        md("home/sub.md", "Sub", ""),
        md("home/sub/deeper.md", "Deeper", ""),
    ]);
    let listed: Vec<String> = s.nav().pages().iter().map(|p| p.to_string()).collect();
    assert_eq!(listed, ["home", "home/sub", "home/sub/deeper"]);
    assert_eq!(s.page(&p("home/sub")).unwrap().section, None);
    assert_eq!(
        s.page(&p("home/sub/deeper")).unwrap().section,
        Some(p("home/sub"))
    );
    let crumbs: Vec<(String, Option<String>)> = s
        .page(&p("home/sub/deeper"))
        .unwrap()
        .breadcrumbs
        .iter()
        .map(|c| (c.dir.to_string(), c.page.as_ref().map(|p| p.to_string())))
        .collect();
    assert_eq!(
        crumbs,
        [
            ("".to_owned(), Some("home".to_owned())),
            ("home".to_owned(), None),
            ("home/sub".to_owned(), Some("home/sub".to_owned())),
        ]
    );
}

#[test]
fn a_link_to_a_file_that_is_not_utf8_says_so() {
    let s = site(vec![
        SourceFile::regular("binary.md".to_owned(), vec![b'-', 0xff]),
        md("home.md", "Home", "[b](/binary)\n"),
    ]);
    assert!(s.findings().iter().any(|f| matches!(
        f,
        Finding::BrokenPageLink {
            why: NoPage::NotUtf8,
            ..
        }
    )));
}

fn kinds(s: &Site) -> Vec<String> {
    s.findings()
        .iter()
        .map(|f| {
            let k = format!("{f:?}");
            format!(
                "{}:{}:{}",
                f.file(),
                f.line(),
                &k[..k.find(' ').unwrap_or(k.len())]
            )
        })
        .collect()
}

#[test]
fn links_and_anchors_on_an_unpublished_page_are_checked_but_it_is_not_built() {
    let s = site(vec![
        md("home.md", "Home", "## Top\n"),
        file(
            "draft.md",
            "---\ntitle: D\npublished: false\n---\n## Mine\n[a](/missing) [b](#nope) [c](/home#nope) [d](#mine) [e](/home#top) [f](/old)\n",
        ),
        file("old.md", "---\ntitle: O\nredirect_to: /home\n---\n"),
    ]);
    assert!(s.page(&p("draft")).is_none());
    assert_eq!(
        kinds(&s),
        [
            "draft.md:6:BrokenPageLink",
            "draft.md:6:BrokenAnchor",
            "draft.md:6:BrokenAnchor",
            "draft.md:6:LinkToRedirectStub",
        ]
    );
    // Not in the link graph or the nav.
    assert!(s.backlinks(&p("home")).is_empty());
    assert!(!s.nav().pages().iter().any(|q| q.as_str() == "draft"));
}

#[test]
fn sec_ids_are_harvested_from_every_file_check_packages_reads() {
    let marker = |id: &str| format!("<!-- @sec id={id} -->\n## H\n");
    let s = site_cfg(
        wikijs_cfg(),
        vec![
            md("aaa.md", "A", &marker("x")),
            // check-packages reads all of these.
            file("README.md", &marker("x")),
            file("bad.md", &format!("no front matter\n{}", marker("x"))),
            file(
                "draft.md",
                &format!("---\ntitle: D\npublished: false\n---\n{}", marker("x")),
            ),
            file("scripts/s.md", &marker("x")),
            SourceFile::regular(
                "binary.md",
                [b"\xff\n".as_slice(), marker("x").as_bytes()].concat(),
            ),
            // A duplicate within a file nothing renders.
            file(
                "scripts/twice.md",
                &format!("{}{}", marker("y"), marker("y")),
            ),
            // A duplicate within a page is its render's finding, reported once.
            md("ccc.md", "C", &format!("{}{}", marker("z"), marker("z"))),
            // A second use within a page whose first is another file's: once.
            md("ddd.md", "D", &format!("{}{}", marker("x"), marker("x"))),
            // And these it skips (a hidden directory, node_modules, .git).
            file(".githooks/h.md", &marker("x")),
            file(".github/f.md", &marker("x")),
            file("node_modules/m/n.md", &marker("x")),
            file("a/.git/g.md", &marker("x")),
            // Not a `.md`.
            file("notes.txt", &marker("x")),
        ],
    );
    let dups: Vec<(String, usize, String, String)> = s
        .findings()
        .iter()
        .filter_map(|f| match f {
            Finding::DuplicateSecId {
                file,
                line,
                id,
                first_file,
                ..
            } => Some((file.to_string(), *line, id.clone(), first_file.to_string())),
            _ => None,
        })
        .collect();
    let d = |f: &str, l: usize, id: &str, first: &str| {
        (f.to_owned(), l, id.to_owned(), first.to_owned())
    };
    assert_eq!(
        dups,
        [
            d("aaa.md", 4, "x", "README.md"),
            d("bad.md", 2, "x", "README.md"),
            d("binary.md", 2, "x", "README.md"),
            d("ddd.md", 4, "x", "README.md"),
            d("draft.md", 5, "x", "README.md"),
            d("scripts/s.md", 1, "x", "README.md"),
            d("scripts/twice.md", 3, "y", "scripts/twice.md"),
        ]
    );
    let render_dups: Vec<(String, usize)> = s
        .findings()
        .iter()
        .filter_map(|f| match f {
            Finding::Render {
                file,
                line,
                finding: rubrica_render::RenderFinding::DuplicateSecId { .. },
            } => Some((file.to_string(), *line)),
            _ => None,
        })
        .collect();
    assert_eq!(
        render_dups,
        [("ccc.md".to_owned(), 6), ("ddd.md".to_owned(), 6)]
    );
}

#[test]
fn a_dead_anchor_is_a_warning_and_an_error_under_strict() {
    let s = site(vec![
        md("xxx.md", "X", "## y\n"),
        md("home.md", "Home", "[a](/xxx#z) [b](#z) [c](/missing)\n"),
    ]);
    let sev = |strict: bool| -> Vec<Severity> {
        s.findings()
            .iter()
            .map(|f| f.severity(CheckOptions { strict }))
            .collect()
    };
    assert_eq!(
        kinds(&s),
        [
            "home.md:4:BrokenAnchor",
            "home.md:4:BrokenAnchor",
            "home.md:4:BrokenPageLink"
        ]
    );
    assert_eq!(
        sev(false),
        [Severity::Warning, Severity::Warning, Severity::Error]
    );
    assert_eq!(
        sev(true),
        [Severity::Error, Severity::Error, Severity::Error]
    );
}

#[test]
fn symlinks_and_submodules_are_never_read() {
    let link = |path: &str, bytes: &str| SourceFile {
        kind: FileKind::Symlink,
        ..file(path, bytes)
    };
    let s = site_cfg(
        wikijs_cfg(),
        vec![
            md("home.md", "Home", "[s](/secret) [t](/assets/t.png)\n"),
            // A committed symlink's blob is its target path.
            link("secret.md", "../../outside/secret.md"),
            // As if a reader had followed it: still never read.
            link(
                "followed.md",
                "---\ntitle: FOLLOWED\n---\n<!-- @sec id=leaked-id -->\n## FOLLOWED\n",
            ),
            link("assets/t.png", "/etc/passwd"),
            SourceFile {
                kind: FileKind::Submodule,
                ..file("assets/sub", "")
            },
            SourceFile {
                kind: FileKind::Other,
                ..file("fifo.md", "")
            },
            // Ignored paths are not reported, whatever they are.
            link("scripts/tool.md", "../x"),
            link(".githooks/pre-push", "../x"),
            md("run.md", "Run", ""),
        ],
    );
    assert_eq!(
        kinds(&s),
        [
            "assets/sub:1:NotARegularFile",
            "assets/t.png:1:NotARegularFile",
            "fifo.md:1:NotARegularFile",
            "followed.md:1:NotARegularFile",
            "home.md:4:BrokenPageLink",
            "home.md:4:BrokenAssetLink",
            "secret.md:1:NotARegularFile",
        ]
    );
    assert!(s.assets().is_empty());
    assert_eq!(
        s.pages().keys().map(|k| k.to_string()).collect::<Vec<_>>(),
        ["home", "run"]
    );
    let all = format!("{s:?}");
    for leaked in ["outside", "FOLLOWED", "passwd", "leaked-id"] {
        assert!(!all.contains(leaked), "{leaked} reached the site");
    }
    assert!(matches!(
        &s.findings()[0],
        Finding::NotARegularFile {
            kind: FileKind::Submodule,
            ..
        }
    ));
}

#[test]
fn a_link_or_checkout_where_pages_could_be_is_reported() {
    // A symbolic link or a submodule named like a
    // directory of pages (or `assets`) hides whatever it would hold, so it
    // is reported, at the directory, though the path alone is no page.
    // Where no page or asset could be under it, it is ignored as before.
    let link = |path: &str| SourceFile {
        kind: FileKind::Symlink,
        ..file(path, "../outside")
    };
    let sub = |path: &str| SourceFile {
        kind: FileKind::Submodule,
        ..file(path, "")
    };
    let s = site_cfg(
        wikijs_cfg(),
        vec![
            md("home.md", "Home", ""),
            link("linked"),
            link("assets"),
            link("guides/more"),
            sub("vendor/sub"),
            link("scripts/bin"),
            link(".githooks/x"),
            sub("node_modules/x"),
            link("a/b/node_modules"),
            link("README.md"),
            SourceFile {
                kind: FileKind::Other,
                ..file("fifo", "")
            },
        ],
    );
    assert_eq!(
        kinds(&s),
        [
            "assets:1:NotARegularFile",
            "guides/more:1:NotARegularFile",
            "linked:1:NotARegularFile",
            "vendor/sub:1:NotARegularFile",
        ]
    );
    assert!(s.findings().iter().all(|f| matches!(
        f,
        Finding::NotARegularFile {
            directory: true,
            ..
        }
    )));
    // A file the site uses is reported as a file, not as a directory.
    let s = site_cfg(wikijs_cfg(), vec![md("home.md", "Home", ""), link("x.md")]);
    assert!(matches!(
        s.findings(),
        [Finding::NotARegularFile {
            directory: false,
            ..
        }]
    ));
}

#[test]
fn a_file_over_the_readers_cap_is_reported_and_never_read() {
    // `rubrica check` does not read a file over its cap; it hands the
    // site the path alone, as `FileKind::TooLarge`. Where the site would use
    // the file it is `FileTooLarge` (an error); an ignored one is nothing.
    let big = |path: &str| SourceFile {
        kind: FileKind::TooLarge,
        ..file(path, "")
    };
    let s = site_cfg(
        wikijs_cfg(),
        vec![
            md("home.md", "Home", "[b](/big) [p](/assets/p.png)\n"),
            big("big.md"),
            big("assets/p.png"),
            big("scripts/huge.md"),
            big("node_modules/x/huge.js"),
        ],
    );
    assert_eq!(
        kinds(&s),
        [
            "assets/p.png:1:FileTooLarge",
            "big.md:1:FileTooLarge",
            "home.md:4:BrokenPageLink",
            "home.md:4:BrokenAssetLink",
        ]
    );
    assert!(s
        .findings()
        .iter()
        .filter(|f| matches!(f, Finding::FileTooLarge { .. }))
        .all(|f| f.severity(CheckOptions::default()) == Severity::Error));
    assert!(s.assets().is_empty());
    assert_eq!(s.pages().len(), 1);
}

/// A cache that records what it is asked and can hand back a substitute.
#[derive(Default)]
struct Recording {
    store: std::collections::HashMap<RenderKey, RenderResult>,
    gets: usize,
    hits: usize,
    puts: usize,
}

impl RenderCache for Recording {
    fn get(&mut self, key: &RenderKey) -> Option<RenderResult> {
        self.gets += 1;
        let r = self.store.get(key).cloned();
        self.hits += usize::from(r.is_some());
        r
    }
    fn put(&mut self, key: RenderKey, render: &RenderResult) {
        self.puts += 1;
        self.store.insert(key, render.clone());
    }
}

fn cache_corpus() -> Vec<SourceFile> {
    vec![
        md("home.md", "Home", "## A\n[x](/xxx#b)\n"),
        md("xxx.md", "X", "## B\n<!-- @sec id=s -->\n## C\n"),
        SourceFile {
            blob_id: Some("0123abcd".to_owned()),
            ..md("yyy.md", "Y", "[h](/)\n")
        },
        file("bad.md", "no front matter\n"),
    ]
}

#[test]
fn a_render_cache_is_used_and_changes_nothing() {
    let plain = site(cache_corpus());
    let mut cache = Recording::default();
    let first = Site::from_sources_with(cache_corpus(), &SiteConfig::default(), &mut cache);
    assert_eq!(first, plain);
    assert_eq!((cache.gets, cache.hits, cache.puts), (4, 0, 4));
    assert!(cache
        .store
        .contains_key(&RenderKey::Blob("0123abcd".to_owned())));
    let second = Site::from_sources_with(cache_corpus(), &SiteConfig::default(), &mut cache);
    assert_eq!(second, plain, "a cached build is the same site");
    assert_eq!((cache.gets, cache.hits, cache.puts), (8, 4, 4));
    // The hit is what the site uses: a substitute shows.
    let key = RenderKey::of(cache_corpus()[1].bytes.as_slice(), None);
    let Some(Ok((front, rendered, offset))) = cache.store.get(&key).cloned() else {
        panic!("xxx.md is cached");
    };
    // A hit is shared with the page, not copied.
    assert!(std::sync::Arc::ptr_eq(
        &rendered,
        &second.page(&p("xxx")).unwrap().rendered
    ));
    let mut changed = front.clone();
    changed.title = "From the cache".to_owned();
    cache.store.insert(key, Ok((changed, rendered, offset)));
    let third = Site::from_sources_with(cache_corpus(), &SiteConfig::default(), &mut cache);
    assert_eq!(third.page(&p("xxx")).unwrap().front.title, "From the cache");
    // Keys are the content's, not the path's.
    assert_eq!(RenderKey::of(b"abc", None), RenderKey::of(b"abc", None),);
    assert_ne!(RenderKey::of(b"abc", None), RenderKey::of(b"abd", None));
}

#[test]
fn redirect_stubs_are_out_of_the_nav_and_keep_their_fragment() {
    let s = site(vec![
        md("home.md", "Home", "[a](/old) [b](/old#x) [c](/sec)\n"),
        md("new.md", "New", "## Part\n"),
        file(
            "old.md",
            "---\ntitle: Old\nredirect_to: /new#part\n---\n## Stub heading\n",
        ),
        file("sec.md", "---\ntitle: Sec\nredirect_to: /new\n---\n"),
        md("sec/child.md", "Child", ""),
    ]);
    let fixes: Vec<Option<String>> = s
        .findings()
        .iter()
        .filter(|f| matches!(f, Finding::LinkToRedirectStub { .. }))
        .map(Finding::fix)
        .collect();
    assert_eq!(
        fixes,
        [
            Some("/new#part".to_owned()),
            Some("/new#x".to_owned()),
            Some("/new".to_owned())
        ]
    );
    let old = s.page(&p("old")).unwrap();
    assert!(old.toc.is_empty());
    assert_eq!(old.redirect_to.as_ref().unwrap().url(), "/new#part");
    assert_eq!(s.redirects()[&p("old")].fragment.as_deref(), Some("part"));
    let listed: Vec<String> = s.nav().pages().iter().map(|q| q.to_string()).collect();
    assert_eq!(listed, ["home", "new", "sec/child"]);
    // `sec` is a stub, so its section has no index.
    assert_eq!(s.page(&p("sec/child")).unwrap().section, None);
}

#[test]
fn an_invalid_link_path_is_reported_escaped() {
    let s = site(vec![md(
        "home.md",
        "Home",
        "[a](</a\u{1b}[31m%FF>) [b](/a%00b) [c](</en/>)\n",
    )]);
    let paths: Vec<(String, NoPage)> = s
        .findings()
        .iter()
        .filter_map(|f| match f {
            Finding::BrokenPageLink { path, why, .. } => Some((path.clone(), why.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        paths,
        [
            (
                "/a%1B%5B31m%FF".to_owned(),
                NoPage::Invalid(rubrica_site::BadPath::NotUtf8)
            ),
            (
                "/a%00b".to_owned(),
                NoPage::Invalid(rubrica_site::BadPath::Character('\0'))
            ),
            (
                "/en/".to_owned(),
                NoPage::Invalid(rubrica_site::BadPath::EmptySegment)
            ),
        ]
    );
    assert!(paths
        .iter()
        .all(|(p, _)| p.bytes().all(|b| b.is_ascii_graphic())));
}

#[test]
fn the_wiki_origin_is_checked_and_matched_case_sensitively() {
    assert!(Origin::new("").is_err());
    assert!(Origin::new("https://wiki.example/").is_err());
    let s = site_cfg(wikijs_cfg(), vec![
        md("abc.md", "A", ""),
        md(
            "home.md",
            "Home",
            "[a](https://wiki.example.org/abc) [b](HTTPS://WIKI.EXAMPLE.ORG/abc) [c](https://wiki.example.orgx/abc) [d](https://wiki.example.org)\n",
        ),
    ]);
    assert_eq!(
        targets(&s, "home"),
        [
            Target::Page {
                path: p("abc"),
                fragment: None
            },
            Target::External,
            Target::External,
            Target::External,
        ]
    );
}

fn relative(s: &Site) -> Vec<(String, String, Option<String>, Severity)> {
    s.findings()
        .iter()
        .filter_map(|f| match f {
            Finding::RelativeLink { href, reaches, .. } => Some((
                href.clone(),
                reaches.to_string(),
                f.fix(),
                f.severity(CheckOptions::default()),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn a_relative_link_to_any_md_file_is_a_finding_and_only_a_page_gets_a_fix() {
    // check-frontmatter.cjs asks only whether `<path>.md` exists: a draft, a
    // file with bad front matter, a reserved one, README.md and scripts/
    // count. Only a published page has an absolute link to suggest.
    let s = site_cfg(wikijs_cfg(), vec![
        md("abc/b.md", "B", ""),
        file("abc/bad.md", "no front matter\n"),
        file("abc/d.md", "---\ntitle: D\npublished: false\n---\n"),
        md("_x.md", "X", ""),
        md("README.md", "R", ""),
        md("scripts/n.md", "N", ""),
        SourceFile::regular("abc/bin.md", vec![0xff]),
        md(
            "abc/c.md",
            "C",
            "[1](d) [2](bad) [3](../_x) [4](../README.md) [5](../scripts/n) [6](b) [7](nope) [8](bin) [9](../README)\n",
        ),
    ]);
    let e = Severity::Error;
    let none = |h: &str, f: &str| (h.to_owned(), f.to_owned(), None, e);
    assert_eq!(
        relative(&s),
        [
            none("d", "abc/d.md"),
            none("bad", "abc/bad.md"),
            none("../_x", "_x.md"),
            none("../README.md", "README.md"),
            none("../scripts/n", "scripts/n.md"),
            (
                "b".to_owned(),
                "abc/b.md".to_owned(),
                Some("/abc/b".to_owned()),
                e
            ),
            none("bin", "abc/bin.md"),
            none("../README", "README.md"),
        ]
    );
}

#[test]
fn a_relative_link_climbing_above_the_root_stays_there_as_a_browser_does() {
    // node: `new URL('../../../abc/b', 'http://x/abc/c').pathname` is
    // `/abc/b`, and `new URL('..', 'http://x/abc/c').pathname` is `/`.
    let s = site(vec![
        md("home.md", "H", ""),
        md("abc/b.md", "B", ""),
        md(
            "abc/c.md",
            "C",
            "[x](../../../abc/b#y) [y](..) [z](../../nope)\n",
        ),
    ]);
    let e = Severity::Error;
    assert_eq!(
        relative(&s),
        [
            (
                "../../../abc/b#y".to_owned(),
                "abc/b.md".to_owned(),
                Some("/abc/b#y".to_owned()),
                e
            ),
            (
                "..".to_owned(),
                "home.md".to_owned(),
                Some("/".to_owned()),
                e
            ),
        ]
    );
    let c = s.page(&p("abc/c")).unwrap();
    assert_eq!(
        c.links[2].target,
        Target::Relative {
            page: None,
            file: None
        }
    );
}

#[test]
fn an_empty_or_query_only_relative_link_is_its_own_page() {
    // check-frontmatter.cjs strips `?` and `#` and, with no path left, has
    // nothing to check; Wiki.js and a browser stay on the page. So, as `#x`:
    // the same page, its fragment checked.
    let s = site(vec![
        md("docs.md", "D", ""),
        md(
            "docs/beta.md",
            "B",
            "## Y\n[a]() [b](?) [c](?x=1) [d](?x#y) [e](?x#nope) [f](?x#%59)\n",
        ),
    ]);
    assert_eq!(relative(&s), []);
    let same = |f: &str| Target::SamePage {
        fragment: f.to_owned(),
    };
    let beta = s.page(&p("docs/beta")).unwrap();
    let targets: Vec<&Target> = beta.links.iter().map(|l| &l.target).collect();
    assert_eq!(
        targets,
        [
            &same(""),
            &same(""),
            &same(""),
            &same("y"),
            &same("nope"),
            &same("Y")
        ]
    );
    let anchors: Vec<(&str, bool)> = s
        .findings()
        .iter()
        .filter_map(|f| match f {
            Finding::BrokenAnchor {
                fragment,
                same_page,
                ..
            } => Some((fragment.as_str(), *same_page)),
            _ => None,
        })
        .collect();
    assert_eq!(anchors, [("nope", true), ("Y", true)]);
    assert_eq!(s.findings().len(), 2, "{:?}", s.findings());
}
