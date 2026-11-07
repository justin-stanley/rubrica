//! The site's memory is linear in its input: no finding, link, breadcrumb,
//! directory or nav entry copies a string of the page, heading, section or
//! stub it points at; and the suggestion index, which is not held, is one at
//! a time.
//!
//! The measure is an accounting of what [`Site`] holds once built (held
//! memory, not the peak while building), not an allocator (the workspace
//! forbids `unsafe`, which a counting `GlobalAlloc` needs): every string the
//! site's own structures reach (page paths, URLs, files, hrefs, targets,
//! findings, suggestions, breadcrumbs, directories, the TOC, the nav,
//! backlinks, redirects, the fold keys), each buffer counted once however
//! many values share it (by its start address, keeping the longest length
//! seen, so an `Arc<str>` cloned 20,000 times is one and a directory that is
//! a prefix of a page's path costs nothing), plus `size_of` of every element
//! their vectors have room for (their capacity, where the site shows it). The render itself (`Page::rendered`, `Page::front`) is
//! `rubrica-render`'s and is left out.
//!
//! The peak adds what building holds and drops (the crate docs list it):
//! of that, only the suggestion index could grow past the input (a trie
//! over every target's heading ids, held for every target until the link
//! check ended: 44 bytes per input byte of real peak on 1,000-character
//! headings); `one_suggestion_index_is_alive_at_a_time` and
//! `the_suggestion_index_is_linear_in_its_ids` bound it.
//!
//! Each shape is a target string of size `h` reached by `l` references,
//! which copying makes `h × l` while the input is `h + l`; at two sizes
//! (`k` = 1, 2: both doubled), the accounted bytes must stay under the
//! shape's own ceiling (about twice what it holds, so undoing one kind of
//! sharing fails it) per input byte and grow by at most [`GROWTH`] when the
//! input doubles (copying grows by 4). The `*_are_shared` tests check the
//! sharing itself, by identity.

use std::collections::{HashMap, HashSet};
use std::mem::size_of;
use std::sync::Arc;

use rubrica_site::links::{loose_index_bytes, loose_indexes_peak, ops_reset};
use rubrica_site::{
    Crumb, DirPath, Finding, NavEntry, NavSection, NoPage, Page, PageLink, PagePath, Site,
    SiteConfig, SourceFile, Suggestion, Target, TocEntry,
};

/// The most the accounted bytes may grow when the input doubles.
const GROWTH: f64 = 2.3;

// Each shape's ceiling, in bytes held per input byte: about twice what it
// holds (the measure, k = 1 and 2, in the comment).
const CEIL_LINE: f64 = 28.0; // 14.2, 14.2
const CEIL_WORD: f64 = 30.0; // 15.2, 14.7
const CEIL_TITLE: f64 = 9.0; // 4.6, 4.6
const CEIL_DEEP: f64 = 7.5; // 3.7, 3.2
const CEIL_STUB: f64 = 26.0; // 13.0, 13.0
const CEIL_SAME: f64 = 49.0; // 24.2, 24.5
const CEIL_REL: f64 = 61.0; // 30.1, 30.5
const CEIL_DUP: f64 = 12.0; // 5.8, 5.7
const CEIL_CHAINS: f64 = 33.0; // 16.4, 16.4 (13.4 before the directories' fold keys)
const CEIL_RELFILE: f64 = 60.0; // 29.1, 30.0
const CEIL_RESERVED: f64 = 6.5; // 3.2, 3.2
const CEIL_DIRS: f64 = 9.0; // 4.4, 4.4

#[derive(Default)]
struct Acct {
    /// Buffer start address to the bytes of it counted so far.
    seen: HashMap<usize, usize>,
    slices: HashSet<(usize, usize)>,
    bytes: usize,
}

impl Acct {
    fn s(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        let counted = self.seen.entry(s.as_ptr() as usize).or_default();
        if s.len() > *counted {
            self.bytes += s.len() - *counted;
            *counted = s.len();
        }
    }
    fn n<T>(&mut self, count: usize) {
        self.bytes += count * size_of::<T>();
    }
    fn p(&mut self, p: &PagePath) {
        self.s(p.as_str());
    }
    fn target(&mut self, t: &Target) {
        match t {
            Target::Page { path, fragment } => {
                self.p(path);
                if let Some(f) = fragment {
                    self.s(f);
                }
            }
            Target::SamePage { fragment } => self.s(fragment),
            Target::NoPage { path, .. } | Target::System { path } | Target::Asset { path, .. } => {
                self.s(path)
            }
            Target::External => {}
            Target::Relative { page, file } => {
                if let Some(p) = page {
                    self.p(p);
                }
                if let Some(f) = file {
                    self.s(f);
                }
            }
        }
    }
    fn toc(&mut self, entries: &Vec<TocEntry>) {
        self.n::<TocEntry>(entries.capacity());
        for e in entries {
            self.s(&e.title);
            if let Some(id) = &e.id {
                self.s(id);
            }
            self.toc(&e.children);
        }
    }
    fn crumbs(&mut self, crumbs: &[Crumb]) {
        // A shared slice is one allocation.
        if crumbs.is_empty()
            || !self
                .slices
                .insert((crumbs.as_ptr() as usize, std::mem::size_of_val(crumbs)))
        {
            return;
        }
        self.n::<Crumb>(crumbs.len());
        for c in crumbs {
            self.s(&c.dir);
            self.s(&c.title);
            if let Some(p) = &c.page {
                self.p(p);
            }
        }
    }
    fn page(&mut self, page: &Page) {
        self.p(&page.path);
        self.s(&page.url);
        self.s(&page.file);
        self.crumbs(&page.breadcrumbs);
        self.toc(&page.toc);
        if let Some(sec) = &page.section {
            self.p(sec);
        }
        self.n::<PageLink>(page.links.capacity());
        for l in &page.links {
            self.target(&l.target);
        }
        if let Some(r) = &page.redirect_to {
            self.p(&r.to);
            if let Some(f) = &r.fragment {
                self.s(f);
            }
        }
    }
    fn section(&mut self, sec: &NavSection) {
        self.s(&sec.dir);
        self.s(&sec.title);
        if let Some(i) = &sec.index {
            self.p(i);
        }
        self.n::<NavEntry>(sec.entries.capacity());
        for e in &sec.entries {
            match e {
                NavEntry::Page { path, title } => {
                    self.p(path);
                    self.s(title);
                }
                NavEntry::Section(sub) => self.section(sub),
            }
        }
    }
    fn finding(&mut self, f: &Finding) {
        self.s(f.file());
        match f {
            Finding::BrokenPageLink { href, path, .. } => {
                self.s(href);
                self.s(path);
            }
            Finding::BrokenAnchor {
                href,
                to,
                fragment,
                why,
                suggestion,
                ..
            } => {
                self.s(href);
                self.p(to);
                self.s(fragment);
                if let rubrica_site::AnchorProblem::DroppedId { text } = why {
                    self.s(text);
                }
                if let Some(s) = suggestion {
                    self.s(&s.id);
                    self.s(&s.text);
                }
            }
            Finding::RelativeLink {
                href, reaches, to, ..
            } => {
                self.s(href);
                self.s(reaches);
                if let Some(to) = to {
                    self.p(to);
                }
            }
            Finding::BrokenAssetLink { href, .. } => self.s(href),
            Finding::DuplicateSecId { id, first_file, .. } => {
                self.s(id);
                self.s(first_file);
            }
            Finding::DuplicateUrl { other, .. } | Finding::DirectoryFileConflict { other, .. } => {
                self.s(other)
            }
            Finding::RedirectTargetMissing { target, .. } => self.s(target),
            Finding::RedirectChain { target, .. } => self.p(target),
            Finding::LinkToRedirectStub {
                href, stub, target, ..
            } => {
                self.s(href);
                self.p(stub);
                self.s(target);
            }
            Finding::SuggestionsBounded { .. }
            | Finding::IndexPageWithoutSection { .. }
            | Finding::NotUtf8 { .. }
            | Finding::MdUnderAssets { .. }
            | Finding::PathTooLongForWikijs { .. }
            | Finding::PathTooLong { .. }
            | Finding::BadSourcePath { .. }
            | Finding::NotARegularFile { .. } => {}
            // Source-side and once per file or marker: their size, roughly.
            Finding::Render { .. }
            | Finding::FrontMatter { .. }
            | Finding::BadFrontMatter { .. }
            | Finding::ReservedPagePath { .. }
            | Finding::PathDiffersInWikijs { .. } => self.bytes += format!("{f:?}").len(),
            other => panic!("unaccounted finding {other:?}"),
        }
    }
}

/// The bytes `site` holds, as the module docs count them.
fn accounted(site: &Site) -> usize {
    let mut a = Acct::default();
    for page in site.pages().values() {
        a.page(page);
    }
    a.section(&site.nav().root);
    for (to, froms) in site.all_backlinks() {
        a.p(to);
        a.n::<PagePath>(froms.capacity());
        for f in froms {
            a.p(f);
        }
    }
    for (from, r) in site.redirects() {
        a.p(from);
        a.p(&r.to);
    }
    a.n::<Finding>(site.findings().len());
    for f in site.findings() {
        a.finding(f);
    }
    for asset in site.assets() {
        a.s(&asset.path);
        a.bytes += asset.bytes.len();
    }
    for (key, file) in site.folded_paths() {
        a.n::<(Arc<str>, Arc<str>)>(1);
        a.s(key);
        a.s(file);
    }
    for (key, spelling) in site.folded_dirs() {
        a.n::<(DirPath, DirPath)>(1);
        a.s(key);
        a.s(spelling);
    }
    a.bytes
}

fn md(path: &str, title: &str, body: &str) -> SourceFile {
    SourceFile::regular(path, format!("---\ntitle: {title}\n---\n{body}"))
}

fn input(files: &[SourceFile]) -> usize {
    files.iter().map(|f| f.path.len() + f.bytes.len()).sum()
}

/// Builds `shape(1)` and `shape(2)` and checks both against the bounds:
/// at most `ceiling` bytes held per input byte, and linear growth.
fn linear(
    what: &str,
    ceiling: f64,
    shape: impl Fn(usize) -> Vec<SourceFile>,
    check: impl Fn(&Site),
) {
    let mut got = Vec::new();
    for k in [1, 2] {
        let files = shape(k);
        let bytes = input(&files);
        let site = Site::from_sources(files, &SiteConfig::default());
        check(&site);
        let held = accounted(&site);
        eprintln!(
            "memory: {what} (k = {k}): {bytes} input bytes, {held} held ({:.1} per byte)",
            held as f64 / bytes as f64
        );
        got.push((bytes, held));
        assert!(
            held as f64 <= ceiling * bytes as f64,
            "{what} (k = {k}): {held} bytes held for {bytes} input bytes, over {ceiling} per byte"
        );
    }
    let growth = got[1].1 as f64 / got[0].1 as f64;
    let input_growth = got[1].0 as f64 / got[0].0 as f64;
    assert!(
        growth <= GROWTH * input_growth / 2.0,
        "{what}: held grew {growth:.2}× when the input grew {input_growth:.2}×"
    );
}

fn count(site: &Site, pred: impl Fn(&Finding) -> bool) -> usize {
    site.findings().iter().filter(|f| pred(f)).count()
}

#[test]
fn a_line_citation_under_a_long_heading_shares_the_heading() {
    // One page: `## aaa…` and `l` links `#L99`, each suggesting it.
    linear(
        "line citations under a long heading",
        CEIL_LINE,
        |k| {
            let mut body = format!("x\n## {}\n", "a".repeat(8_192 * k));
            for _ in 0..1_000 * k {
                body.push_str("[x](#L99)\n");
            }
            vec![md("home.md", "Home", &body)]
        },
        |site| {
            assert!(site.findings().iter().all(|f| matches!(
                f,
                Finding::BrokenAnchor {
                    suggestion: Some(_),
                    ..
                }
            )));
        },
    );
}

#[test]
fn a_word_overlap_suggestion_shares_the_heading() {
    // A heading of `w` words and links `#w<i>`, each suggesting it by
    // check-anchors' word overlap (or its prefix search, for `w0`).
    linear(
        "word-overlap suggestions of a long heading",
        CEIL_WORD,
        |k| {
            let n = 1_500 * k;
            let words: Vec<String> = (0..n).map(|i| format!("w{i}")).collect();
            let mut body = format!("## {}\n", words.join(" "));
            for i in 0..n {
                body.push_str(&format!("[x](#w{i})\n"));
            }
            vec![md("home.md", "Home", &body)]
        },
        |site| {
            let with = count(site, |f| {
                matches!(
                    f,
                    Finding::BrokenAnchor {
                        suggestion: Some(_),
                        ..
                    }
                )
            });
            assert_eq!(with, site.findings().len());
        },
    );
}

#[test]
fn breadcrumbs_share_a_long_section_title() {
    linear(
        "breadcrumbs under a section with a long title",
        CEIL_TITLE,
        |k| {
            let mut files = vec![md("sec.md", &"t".repeat(16_384 * k), "")];
            for i in 0..2_000 * k {
                files.push(md(&format!("sec/p{i}.md"), "P", ""));
            }
            files
        },
        |site| assert_eq!(site.pages().len(), site.nav().pages().len()),
    );
}

#[test]
fn deep_breadcrumbs_share_every_level() {
    // 63 nested sections (the deepest a page can be under), each with an index page of a 1 KiB title, and
    // many pages at the bottom: each page's 64 crumbs.
    linear(
        "breadcrumbs 64 deep",
        CEIL_DEEP,
        |k| {
            let mut files = Vec::new();
            let mut dir = "ddd".to_owned();
            for level in 0..63 {
                files.push(md(&format!("{dir}.md"), &"t".repeat(1_024 * k), ""));
                if level < 62 {
                    dir.push_str("/d");
                }
            }
            for i in 0..500 * k {
                files.push(md(&format!("{dir}/p{i}.md"), "P", ""));
            }
            files
        },
        |site| {
            let deepest = site.pages().values().map(|p| p.breadcrumbs.len()).max();
            assert_eq!(deepest, Some(64));
        },
    );
}

#[test]
fn links_to_a_redirect_stub_share_its_target() {
    // A stub whose `redirect_to` is long and names no page, and many links
    // to it: each is resolved once, and every finding shares it.
    linear(
        "links to a stub with a long redirect_to",
        CEIL_STUB,
        |k| {
            let mut files = vec![SourceFile::regular(
                "old.md",
                format!(
                    "---\ntitle: Old\nredirect_to: /{}\n---\n",
                    "a".repeat(16_384 * k)
                ),
            )];
            let mut body = String::new();
            for _ in 0..2_000 * k {
                body.push_str("[x](/old)\n");
            }
            files.push(md("home.md", "Home", &body));
            files
        },
        |site| {
            let stubs = count(site, |f| matches!(f, Finding::LinkToRedirectStub { .. }));
            assert!(stubs >= 2_000);
            assert_eq!(stubs + 1, site.findings().len(), "and the missing target");
        },
    );
}

#[test]
fn findings_on_a_page_with_a_long_path_share_it() {
    // A page at a 1,000-byte path with many same-page links to no id: each
    // finding names its file and the page.
    linear(
        "same-page broken anchors on a long path",
        CEIL_SAME,
        |k| {
            let path = format!("p{}", "a".repeat(999));
            let mut body = String::new();
            for _ in 0..4_000 * k {
                body.push_str("[x](#zz)\n");
            }
            vec![md(&format!("{path}.md"), "P", &body)]
        },
        |site| assert!(count(site, |f| matches!(f, Finding::BrokenAnchor { .. })) >= 4_000),
    );
}

#[test]
fn relative_links_to_their_own_page_share_its_path() {
    linear(
        "relative links to a page with a long path",
        CEIL_REL,
        |k| {
            let dir = format!("d{}", "a".repeat(990));
            let mut body = String::new();
            for _ in 0..4_000 * k {
                body.push_str("[x](x)\n");
            }
            vec![md(&format!("{dir}/x.md"), "X", &body)]
        },
        |site| assert!(count(site, |f| matches!(f, Finding::RelativeLink { .. })) >= 4_000),
    );
}

#[test]
fn duplicate_sec_ids_share_the_first_file() {
    linear(
        "duplicate @sec ids against a file with a long path",
        CEIL_DUP,
        |k| {
            let mut first = String::new();
            let mut second = String::new();
            for i in 0..2_000 * k {
                first.push_str(&format!("<!-- @sec id=s{i} -->\n## H{i}\n"));
                second.push_str(&format!("<!-- @sec id=s{i} -->\n## H{i}\n"));
            }
            vec![
                md(&format!("a{}.md", "a".repeat(999)), "A", &first),
                md("bbb.md", "B", &second),
            ]
        },
        |site| {
            assert_eq!(
                count(site, |f| matches!(f, Finding::DuplicateSecId { .. })),
                site.findings().len()
            )
        },
    );
}

#[test]
fn deep_directory_chains_share_their_page_paths() {
    // 200·k chains 62 directories deep, each path about 1,000 bytes, one
    // page at the bottom: every directory a distinct prefix.
    linear(
        "directory chains 62 deep",
        CEIL_CHAINS,
        |k| {
            (0..200 * k)
                .map(|i| {
                    let mut d = format!("c{i:05}");
                    for _ in 0..61 {
                        d.push_str("/sssssssssssssss");
                    }
                    md(&format!("{d}/p.md"), "P", "")
                })
                .collect()
        },
        |site| {
            let deepest = site.pages().values().map(|p| p.breadcrumbs.len()).max();
            assert_eq!(deepest, Some(63));
        },
    );
}

#[test]
fn many_files_in_distinct_directories_share_their_paths() {
    // 4,000·k files, none a page, each two directories of its own: every
    // directory a fold key and a spelling, each a prefix of its file's.
    linear(
        "files in distinct directories",
        CEIL_DIRS,
        |k| {
            (0..4_000 * k)
                .map(|i| SourceFile::regular(format!("d{i:05}/{}/f.txt", "e".repeat(40)), ""))
                .collect()
        },
        |site| assert_eq!(site.folded_dirs().count(), 2 * site.folded_paths().count()),
    );
}

#[test]
fn relative_links_to_a_file_that_is_no_page_share_it() {
    // A draft at a long path, and many relative links to it.
    linear(
        "relative links to a draft with a long path",
        CEIL_RELFILE,
        |k| {
            let dir = format!("d{}", "a".repeat(990));
            vec![
                SourceFile::regular(
                    format!("{dir}/x.md"),
                    "---\ntitle: X\npublished: false\n---\n",
                ),
                md(&format!("{dir}/y.md"), "Y", &"[x](x)\n".repeat(4_000 * k)),
            ]
        },
        |site| {
            assert!(
                count(site, |f| matches!(
                    f,
                    Finding::RelativeLink { to: None, .. }
                )) >= 4_000
            )
        },
    );
}

#[test]
fn links_to_a_reserved_file_share_why() {
    linear(
        "links to a reserved file with a long path",
        CEIL_RESERVED,
        |k| {
            let path = format!("_{}", "a".repeat(999));
            vec![
                md(&format!("{path}.md"), "R", ""),
                md("home.md", "H", &format!("[x](/{path})\n").repeat(400 * k)),
            ]
        },
        |site| {
            let n = count(site, |f| {
                matches!(
                    f,
                    Finding::BrokenPageLink {
                        why: NoPage::Reserved(_),
                        ..
                    }
                )
            });
            assert!(n >= 400);
        },
    );
}

fn site(files: Vec<SourceFile>) -> Site {
    Site::from_sources(files, &SiteConfig::default())
}

fn p(s: &str) -> PagePath {
    PagePath::new(s).unwrap()
}

fn addr(s: &str) -> usize {
    s.as_ptr() as usize
}

#[test]
fn breadcrumbs_and_directories_are_shared() {
    let s = site(vec![
        md("home.md", "H", ""),
        md("sec.md", "Sec", ""),
        md("sec/sub.md", "Sub", ""),
        md("sec/sub/a.md", "A", ""),
        md("sec/sub/b.md", "B", ""),
    ]);
    let a = s.page(&p("sec/sub/a")).unwrap();
    let b = s.page(&p("sec/sub/b")).unwrap();
    assert!(Arc::ptr_eq(&a.breadcrumbs, &b.breadcrumbs));
    // A directory is a prefix of a page path's own buffer, and its nav
    // section's directory and title are its crumb's.
    let keys: HashSet<usize> = s.pages().keys().map(|k| addr(k.as_str())).collect();
    let crumbs = &a.breadcrumbs;
    assert_eq!(crumbs.len(), 3);
    for c in &crumbs[1..] {
        assert!(keys.contains(&addr(&c.dir)), "{:?}", c.dir);
        let page = c.page.as_ref().unwrap();
        assert!(keys.contains(&addr(page.as_str())));
    }
    let NavEntry::Section(sec) = &s.nav().root.entries[0] else {
        panic!("{:?}", s.nav().root.entries);
    };
    let NavEntry::Section(sub) = sec
        .entries
        .iter()
        .find(|e| matches!(e, NavEntry::Section(_)))
        .unwrap()
    else {
        unreachable!()
    };
    assert_eq!(addr(&sec.dir), addr(&crumbs[1].dir));
    assert_eq!(addr(&sub.dir), addr(&crumbs[2].dir));
    assert!(Arc::ptr_eq(&sec.title, &crumbs[1].title));
    assert!(Arc::ptr_eq(&sub.title, &crumbs[2].title));
}

fn suggestions(s: &Site) -> Vec<&Suggestion> {
    s.findings()
        .iter()
        .filter_map(|f| match f {
            Finding::BrokenAnchor {
                suggestion: Some(x),
                ..
            } => Some(x),
            _ => None,
        })
        .collect()
}

#[test]
fn suggestions_and_heading_texts_are_shared() {
    // One heading suggested three ways (prefix, longer prefix, line
    // citation), from this page and another; and a dropped id twice.
    let s = site(vec![
        md(
            "home.md",
            "H",
            "## Alpha beta\n\n## Open\n\n[a](#alpha) [b](#alpha-beta-x) [c](#L5) [d](#open) [e](#open)\n",
        ),
        md("other.md", "O", "[f](/#alpha) [g](/home#alpha-beta-y)\n"),
    ]);
    let got = suggestions(&s);
    assert_eq!(got.len(), 5, "{:?}", s.findings());
    for x in &got[1..] {
        assert_eq!(&*x.id, "alpha-beta");
        assert!(Arc::ptr_eq(&x.id, &got[0].id));
        assert!(Arc::ptr_eq(&x.text, &got[0].text));
    }
    let dropped: Vec<&Arc<str>> = s
        .findings()
        .iter()
        .filter_map(|f| match f {
            Finding::BrokenAnchor {
                why: rubrica_site::AnchorProblem::DroppedId { text },
                ..
            } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(dropped.len(), 2);
    assert!(Arc::ptr_eq(dropped[0], dropped[1]));
}

#[test]
fn a_stubs_target_is_shared() {
    let s = site(vec![
        SourceFile::regular("old.md", "---\ntitle: Old\nredirect_to: /new#x\n---\n"),
        md("new.md", "New", "## X\n"),
        md("home.md", "H", "[a](/old) [b](/old#y)\n"),
    ]);
    let targets: Vec<&Arc<str>> = s
        .findings()
        .iter()
        .filter_map(|f| match f {
            Finding::LinkToRedirectStub { target, .. } => Some(target),
            _ => None,
        })
        .collect();
    assert_eq!(targets.len(), 2);
    assert!(Arc::ptr_eq(targets[0], targets[1]));
}

#[test]
fn page_paths_and_files_are_the_sites_own() {
    let s = site(vec![
        md(
            "home.md",
            "H",
            "[a](/sec/a) [b](/sec/a#nope) [c](sec/a) [d](/old)\n",
        ),
        md("sec.md", "Sec", "[h](/)\n"),
        md("sec/a.md", "A", "[s](/sec)\n"),
        SourceFile::regular("old.md", "---\ntitle: Old\nredirect_to: /sec\n---\n"),
    ]);
    let keys: HashMap<&str, usize> = s
        .pages()
        .keys()
        .map(|k| (k.as_str(), addr(k.as_str())))
        .collect();
    let own = |path: &PagePath| {
        assert_eq!(
            keys.get(path.as_str()),
            Some(&addr(path.as_str())),
            "{path} is a copy"
        )
    };
    for (key, page) in s.pages() {
        own(&page.path);
        assert_eq!(addr(key.as_str()), addr(page.path.as_str()));
        page.section.as_ref().map(own);
        if let Some(r) = &page.redirect_to {
            own(&r.to);
        }
        for l in &page.links {
            match &l.target {
                Target::Page { path, .. }
                | Target::Relative {
                    page: Some(path), ..
                } => own(path),
                _ => {}
            }
        }
    }
    for (to, froms) in s.all_backlinks() {
        own(to);
        froms.iter().for_each(own);
    }
    for f in s.findings() {
        let page = s.pages().values().find(|p| &*p.file == f.file()).unwrap();
        assert_eq!(addr(f.file()), addr(&page.file), "{f:?}");
        match f {
            Finding::BrokenAnchor { to, .. } | Finding::LinkToRedirectStub { stub: to, .. } => {
                own(to)
            }
            Finding::RelativeLink { to, reaches, .. } => {
                own(to.as_ref().unwrap());
                assert_eq!(addr(reaches), addr(&s.page(&p("sec/a")).unwrap().file));
            }
            _ => {}
        }
    }
    fn walk(sec: &NavSection, own: &dyn Fn(&PagePath)) {
        sec.index.as_ref().map(own);
        for e in &sec.entries {
            match e {
                NavEntry::Page { path, .. } => own(path),
                NavEntry::Section(sub) => walk(sub, own),
            }
        }
    }
    walk(&s.nav().root, &own);
}

#[test]
fn a_relative_links_file_and_a_reserved_why_are_shared() {
    let s = site(vec![
        md("_x.md", "X", ""),
        md("README.md", "R", ""),
        md(
            "home.md",
            "H",
            "[a](README) [b](README.md) [c](/_x) [d](/_x#y)\n",
        ),
    ]);
    let home = s.page(&p("home")).unwrap();
    let files: Vec<&Arc<str>> = home
        .links
        .iter()
        .filter_map(|l| match &l.target {
            Target::Relative { file, .. } => file.as_ref(),
            _ => None,
        })
        .collect();
    let reaches: Vec<&Arc<str>> = s
        .findings()
        .iter()
        .filter_map(|f| match f {
            Finding::RelativeLink { reaches, .. } => Some(reaches),
            _ => None,
        })
        .collect();
    assert_eq!((files.len(), reaches.len()), (2, 2));
    for f in files.iter().chain(&reaches) {
        assert!(Arc::ptr_eq(f, files[0]));
    }
    let whys: Vec<_> = s
        .findings()
        .iter()
        .filter_map(|f| match f {
            Finding::BrokenPageLink {
                why: NoPage::Reserved(r),
                ..
            } => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(whys.len(), 2);
    assert!(Arc::ptr_eq(whys[0], whys[1]));
}

#[test]
fn one_suggestion_index_is_alive_at_a_time() {
    // Ten target pages, and broken anchors into each in turn, twice over:
    // held until the links were all checked, ten indexes were alive at once.
    let mut files: Vec<SourceFile> = (0..10)
        .map(|i| {
            md(
                &format!("t{i}.md"),
                "T",
                &format!("## alpha {i}\n## beta {i}\n"),
            )
        })
        .collect();
    let mut body = String::new();
    for _ in 0..2 {
        for i in 0..10 {
            body.push_str(&format!("[x](/t{i}#alpha) [y](#nope-{i})\n"));
        }
    }
    body.push_str("## nope\n");
    files.push(md("home.md", "Home", &body));
    ops_reset();
    let s = site(files);
    assert_eq!(loose_indexes_peak(), 1);
    let ids: Vec<String> = suggestions(&s).iter().map(|x| x.id.to_string()).collect();
    let mut want: Vec<String> = (0..10).map(|i| format!("alpha-{i}")).collect();
    want.extend(want.clone());
    let mut got_alpha: Vec<String> = ids
        .iter()
        .filter(|i| i.starts_with("alpha"))
        .cloned()
        .collect();
    got_alpha.sort();
    want.sort();
    assert_eq!(got_alpha, want);
    assert_eq!(ids.iter().filter(|i| *i == "nope").count(), 20);
}

/// A small deterministic generator (xorshift64*).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn word(&mut self, len: usize, alphabet: &[u8]) -> String {
        (0..len)
            .map(|_| alphabet[(self.next() % alphabet.len() as u64) as usize] as char)
            .collect()
    }
}

/// The most bytes a suggestion index holds per byte of its ids: about
/// twice the most measured (36.8, 26.1 and 31.6 on the three shapes).
const INDEX_PER_ID_BYTE: f64 = 72.0;

#[test]
fn the_suggestion_index_is_linear_in_its_ids() {
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let shapes: Vec<(&str, Vec<String>)> = vec![
        (
            "1,000 random 1,000-letter ids",
            (0..1_000)
                .map(|_| rng.word(1_000, b"abcdefghijklmnopqrstuvwxyz"))
                .collect(),
        ),
        (
            "20,000 random 3-letter ids",
            (0..20_000)
                .map(|_| rng.word(3, b"abcdefghijklmnopqrstuvwxyz"))
                .collect(),
        ),
        (
            "2,000 ids of 100 random two-letter words",
            (0..2_000).map(|_| rng.word(300, b"ab-")).collect(),
        ),
    ];
    for (what, ids) in &shapes {
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let bytes: usize = refs.iter().map(|s| s.len()).sum();
        let held = loose_index_bytes(&refs);
        eprintln!(
            "index: {what}: {bytes} id bytes, {held} held ({:.1} per byte)",
            held as f64 / bytes as f64
        );
        assert!(
            held as f64 <= INDEX_PER_ID_BYTE * bytes as f64,
            "{what}: {held} for {bytes}"
        );
    }
}
