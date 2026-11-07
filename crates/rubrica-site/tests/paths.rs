//! Which files are pages, against Wiki.js 2.5.314's own path rules
//! (`server/helpers/page.js`: `parsePath`, `getPagePath`, `isReservedPath`;
//! `server/app/data.yml`'s `reservedPaths`; the routes in `server/controllers`)
//! and the build's output layout.

use rubrica_render::frontmatter::Severity;
use std::path::PathBuf;

use rubrica_site::source::{wikijs_page_path, wikijs_parse_path};
use rubrica_site::{
    page_path_for_new_file, BadPath, CheckOptions, FileKind, Finding, NewPageProblem, NoPage,
    NotAPage, PagePath, Reserved, Site, SiteConfig, SourceFile, WikijsPage, WikijsPathProblem,
    INVISIBLE_LETTERS, MAX_PATH_BYTES, MAX_PATH_SEGMENTS,
};
use serde::Deserialize;

fn md(path: &str) -> SourceFile {
    SourceFile::regular(path, format!("---\ntitle: {path}\n---\n"))
}

fn site(files: Vec<SourceFile>) -> Site {
    Site::from_sources(files, &SiteConfig::default())
}

fn limits() -> SiteConfig {
    SiteConfig {
        wikijs_limits: true,
        ..SiteConfig::default()
    }
}

fn pages(s: &Site) -> Vec<String> {
    s.pages().keys().map(|p| p.to_string()).collect()
}

fn reserved(s: &Site) -> Vec<(String, Reserved)> {
    s.findings()
        .iter()
        .filter_map(|f| match f {
            Finding::ReservedPagePath { file, why } => Some((file.to_string(), why.clone())),
            _ => None,
        })
        .collect()
}

fn reads(locale: &str, path: &str, explicit: bool) -> Reserved {
    Reserved::UnreachableInWikijs {
        reads_as: Some(WikijsPage {
            locale: locale.to_owned(),
            path: path.to_owned(),
            explicit_locale: explicit,
        }),
    }
}

#[test]
fn a_first_segment_of_one_utf16_unit_is_dropped_as_wikijs_drops_it() {
    // `parsePath`: `if (pathParts[0].length === 1) pathParts.shift()`, any
    // character, then the next segment is tested as a locale.
    let s = site(vec![
        md("1/xyz.md"),
        md("é/xyz.md"),
        md("-/xyz.md"),
        md("t/xyz.md"),
        md("1/ab/c.md"),
        md("x.md"),
        md("\u{1D49C}/xyz.md"),
        md("abc..d.md"),
        md("abc/xyz.md"),
    ]);
    // An astral letter is two UTF-16 units, so Wiki.js keeps it.
    assert_eq!(pages(&s), ["abc/xyz", "\u{1D49C}/xyz"]);
    assert_eq!(
        reserved(&s),
        [
            ("-/xyz.md".to_owned(), reads("en", "xyz", false)),
            ("1/ab/c.md".to_owned(), reads("ab", "c", true)),
            ("1/xyz.md".to_owned(), reads("en", "xyz", false)),
            ("abc..d.md".to_owned(), reads("en", "abcd", false)),
            ("t/xyz.md".to_owned(), reads("en", "xyz", false)),
            ("x.md".to_owned(), reads("en", "", false)),
            ("é/xyz.md".to_owned(), reads("en", "xyz", false)),
        ]
    );
    assert!(s
        .findings()
        .iter()
        .all(|f| f.severity(CheckOptions::default()) == Severity::Error));
}

#[test]
fn locale_shaped_first_segments_are_reserved_in_any_case() {
    let s = site(vec![
        md("en/x.md"),
        md("EN/x.md"),
        md("ab-CD/x.md"),
        md("en.md"),
        md("abc-de/x.md"),
    ]);
    assert_eq!(pages(&s), ["abc-de/x"]);
    assert_eq!(
        reserved(&s),
        [
            ("EN/x.md".to_owned(), reads("EN", "x", true)),
            ("ab-CD/x.md".to_owned(), reads("ab-CD", "x", true)),
            ("en.md".to_owned(), reads("en", "", true)),
            ("en/x.md".to_owned(), reads("en", "x", true)),
        ]
    );
}

#[test]
fn the_builds_own_names_are_reserved() {
    let s = site(vec![
        md("index.md"),
        md("Index.md"),
        md("404.md"),
        md("abc.md/x.md"),
        md("docs.HTML/x.md"),
        md("notes.html.md"),
        md("abc/index.md"),
        md("abc.md"),
        md("index/x.md"),
    ]);
    assert_eq!(pages(&s), ["abc", "abc/index", "index/x"]);
    let got: Vec<(String, Reserved)> = reserved(&s);
    assert_eq!(
        got,
        [
            ("404.md".to_owned(), Reserved::OutputName),
            ("Index.md".to_owned(), Reserved::OutputName),
            ("abc.md/x.md".to_owned(), Reserved::OutputExtension),
            ("docs.HTML/x.md".to_owned(), Reserved::OutputExtension),
            ("index.md".to_owned(), Reserved::OutputName),
            ("notes.html.md".to_owned(), Reserved::OutputExtension),
        ]
    );
}

#[test]
fn an_index_page_without_its_section_page_is_an_error() {
    let s = site(vec![
        md("abc/index.md"),
        md("xyz/index.md"),
        md("xyz.md"),
        SourceFile::regular("draft/index.md", "---\ntitle: I\n---\n"),
        SourceFile::regular("draft.md", "---\ntitle: D\npublished: false\n---\n"),
    ]);
    let got: Vec<(&str, Severity)> = s
        .findings()
        .iter()
        .filter(|f| matches!(f, Finding::IndexPageWithoutSection { .. }))
        .map(|f| (f.file(), f.severity(CheckOptions::default())))
        .collect();
    assert_eq!(
        got,
        [
            ("abc/index.md", Severity::Error),
            ("draft/index.md", Severity::Error)
        ]
    );
    // Still pages: links to `/abc/index` reach them.
    assert!(s.page(&PagePath::new("abc/index").unwrap()).is_some());
}

#[test]
fn whitespace_at_a_segment_edge_is_a_bad_path() {
    let s = site(vec![
        md("abc /x.md"),
        md("abc/ x.md"),
        md("abc\u{3000}.md"),
        md("abc\u{a0}/x.md"),
        md("ab c.md"),
    ]);
    assert_eq!(pages(&s), ["ab c"]);
    let bad: Vec<(&str, BadPath)> = s
        .findings()
        .iter()
        .filter_map(|f| match f {
            Finding::BadSourcePath { file, why } => Some((&**file, *why)),
            _ => None,
        })
        .collect();
    assert_eq!(
        bad,
        [
            ("abc /x.md", BadPath::EdgeWhitespace),
            ("abc/ x.md", BadPath::EdgeWhitespace),
            ("abc\u{a0}/x.md", BadPath::EdgeWhitespace),
            ("abc\u{3000}.md", BadPath::EdgeWhitespace),
        ]
    );
}

#[test]
fn percent_and_format_characters_are_bad_paths() {
    let s = site(vec![
        md("abc%41.md"),
        md("abc\u{200B}d.md"),
        md("abc\u{202E}d.md"),
        md("abc\u{2066}d.md"),
        md("abc\u{ad}d.md"),
        md("abc\u{e0001}.md"),
        md("abc\u{200d}d.md"),
    ]);
    assert!(s.pages().is_empty());
    let chars: Vec<BadPath> = s
        .findings()
        .iter()
        .filter_map(|f| match f {
            Finding::BadSourcePath { why, .. } => Some(*why),
            _ => None,
        })
        .collect();
    assert_eq!(
        chars,
        [
            BadPath::Character('%'),
            BadPath::Character('\u{ad}'),
            BadPath::Character('\u{200B}'),
            BadPath::Character('\u{200d}'),
            BadPath::Character('\u{202E}'),
            BadPath::Character('\u{2066}'),
            BadPath::Character('\u{e0001}'),
        ]
    );
}

#[test]
fn urls_equal_under_full_case_folding_are_duplicates() {
    // APFS takes these as one name: full case folding over NFD.
    let s = site(vec![
        md("stuff.md"),
        md("\u{17F}tuff.md"),
        md("key.md"),
        md("\u{212A}ey.md"),
        md("strasse.md"),
        md("Stra\u{df}e.md"),
        md("other.md"),
    ]);
    let dups: Vec<(String, String, Severity)> = s
        .findings()
        .iter()
        .filter_map(|f| match f {
            Finding::DuplicateUrl { file, other } => Some((
                file.to_string(),
                other.to_string(),
                f.severity(CheckOptions::default()),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        dups,
        [
            (
                "strasse.md".to_owned(),
                "Stra\u{df}e.md".to_owned(),
                Severity::Error
            ),
            (
                "\u{17F}tuff.md".to_owned(),
                "stuff.md".to_owned(),
                Severity::Error
            ),
            (
                "\u{212A}ey.md".to_owned(),
                "key.md".to_owned(),
                Severity::Error
            ),
        ]
    );
}

fn wikijs(s: &Site) -> Vec<(String, WikijsPathProblem, Severity)> {
    s.findings()
        .iter()
        .filter_map(|f| match f {
            Finding::PathDiffersInWikijs { file, why } => Some((
                file.to_string(),
                why.clone(),
                f.severity(CheckOptions::default()),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn wikijs_limits_report_pages_wikijs_imports_elsewhere_never_serves_or_cannot_edit() {
    let files = vec![
        md("v2.1.md"),
        md("abc.d/xyz.md"),
        md("a.b/xyz.md"),
        md("login.md"),
        md("Login/google.md"),
        md("login/x/callback.md"),
        md("login/x/y.md"),
        md("logout.md"),
        md("logout/x.md"),
        md("verify/tok.md"),
        md("verify.md"),
        md("login-reset/tok.md"),
        md("healthz.md"),
        md("graphql.md"),
        md("graphql/x.md"),
        md("fonts/x.md"),
        md("Fonts/y.md"),
        md("register.md"),
        md("plain/page.md"),
    ];
    // Without the limits, only `Login/` and `Fonts/`, each a second
    // spelling of a directory, reported once each (at its first file).
    let conflicts: Vec<String> = Site::from_sources(files.clone(), &SiteConfig::default())
        .findings()
        .iter()
        .map(|f| match f {
            Finding::DirectoryFileConflict { file, .. } => file.to_string(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(conflicts, ["fonts/x.md", "login/x/callback.md"]);
    let s = Site::from_sources(files, &limits());
    let w = |locale: &str, path: &str, explicit: bool| {
        WikijsPathProblem::ImportedAs(WikijsPage {
            locale: locale.to_owned(),
            path: path.to_owned(),
            explicit_locale: explicit,
        })
    };
    use Severity::{Error, Warning};
    use WikijsPathProblem::{EditorReserved, Route};
    assert_eq!(
        wikijs(&s),
        [
            ("Login/google.md".to_owned(), Route, Error),
            ("a.b/xyz.md".to_owned(), w("ab", "xyz", true), Error),
            ("abc.d/xyz.md".to_owned(), w("en", "abcd/xyz", false), Error),
            ("fonts/x.md".to_owned(), EditorReserved, Warning),
            ("graphql.md".to_owned(), Route, Error),
            ("graphql/x.md".to_owned(), Route, Error),
            ("healthz.md".to_owned(), Route, Error),
            ("login-reset/tok.md".to_owned(), Route, Error),
            ("login.md".to_owned(), Route, Error),
            ("login/x/callback.md".to_owned(), Route, Error),
            ("login/x/y.md".to_owned(), EditorReserved, Warning),
            ("logout.md".to_owned(), Route, Error),
            ("logout/x.md".to_owned(), EditorReserved, Warning),
            ("register.md".to_owned(), Route, Error),
            ("v2.1.md".to_owned(), w("en", "v21", false), Error),
            ("verify.md".to_owned(), EditorReserved, Warning),
            ("verify/tok.md".to_owned(), Route, Error),
        ]
    );
}

#[test]
fn the_new_file_validator_is_the_sites_rule() {
    let cfg = SiteConfig {
        ignore_dirs: vec!["scripts".to_owned(), "node_modules".to_owned()],
        ..SiteConfig::default()
    };
    let ok = |p: &str| page_path_for_new_file(p, &cfg);
    assert_eq!(ok("abc/def.md"), Ok(PagePath::new("abc/def").unwrap()));
    assert_eq!(ok("home.md"), Ok(PagePath::home()));
    assert_eq!(ok("README.md"), Err(NotAPage::Ignored));
    assert_eq!(ok("scripts/x.md"), Err(NotAPage::Ignored));
    assert_eq!(ok("abc/.x.md"), Err(NotAPage::Ignored));
    assert_eq!(ok("abc.txt"), Err(NotAPage::Ignored));
    assert_eq!(ok("assets/x.md"), Err(NotAPage::UnderAssets));
    assert_eq!(ok("assets/x.png"), Err(NotAPage::UnderAssets));
    assert_eq!(ok("_x.md"), Err(NotAPage::Reserved(Reserved::Underscore)));
    assert_eq!(
        ok("index.md"),
        Err(NotAPage::Reserved(Reserved::OutputName))
    );
    assert!(matches!(
        ok("t/x.md"),
        Err(NotAPage::Reserved(Reserved::UnreachableInWikijs { .. }))
    ));
    assert_eq!(
        ok("abc /x.md"),
        Err(NotAPage::BadPath(BadPath::EdgeWhitespace))
    );
    // PagePath::new checks shape and characters only.
    assert!(PagePath::new("t/x").is_ok());
    assert!(PagePath::new("_x").is_ok());
    // And it is the site's rule, on every generated path.
    let pieces = [
        "a", "abc", "t", "en", "_x", "index", "x.md", "é", ".", "..", " ", "%", "assets",
        "scripts", "home", "a b", "\u{200B}", "README", "abc.d",
    ];
    let mut n = 0u64;
    for a in pieces {
        for b in pieces {
            for path in [format!("{a}.md"), format!("{a}/{b}.md"), format!("{a}/{b}")] {
                let site = Site::from_sources(vec![md(&path)], &cfg);
                let page = site.pages().keys().next().cloned();
                assert_eq!(ok(&path).ok(), page, "{path:?}");
                n += 1;
            }
        }
    }
    assert!(n > 1_000);
}

#[test]
fn an_index_page_under_home_has_no_section_page() {
    // `home.md` is the root's index, so a `home/` directory has none (as the
    // nav has it): `home/index.md` beside `home.md` is still an index page
    // without its section's, and a static file server would answer `/home` with it.
    let s = site(vec![md("home.md"), md("home/index.md"), md("home/x.md")]);
    let got: Vec<&str> = s
        .findings()
        .iter()
        .filter(|f| matches!(f, Finding::IndexPageWithoutSection { .. }))
        .map(|f| f.file())
        .collect();
    assert_eq!(got, ["home/index.md"]);
}

#[test]
fn invisible_letters_are_bad_paths() {
    let files: Vec<SourceFile> = INVISIBLE_LETTERS
        .iter()
        .map(|c| md(&format!("abc{c}d.md")))
        .collect();
    let s = site(files);
    assert!(s.pages().is_empty());
    let mut got: Vec<BadPath> = s
        .findings()
        .iter()
        .filter_map(|f| match f {
            Finding::BadSourcePath { why, .. } => Some(*why),
            _ => None,
        })
        .collect();
    got.sort_by_key(|b| format!("{b:?}"));
    let mut want: Vec<BadPath> = ['\u{115F}', '\u{1160}', '\u{3164}', '\u{FFA0}', '\u{2800}']
        .iter()
        .map(|&c| BadPath::Character(c))
        .collect();
    want.sort_by_key(|b| format!("{b:?}"));
    assert_eq!(got, want);
    assert!(PagePath::new("a\u{3164}").is_err());
}

#[test]
fn every_repository_file_counts_for_duplicate_urls() {
    // On APFS these are one name, whatever the files are: an ignored
    // README.md and a page readme.md, a page and a file that is no page,
    // two assets.
    let s = site(vec![
        md("README.md"),
        md("readme.md"),
        md("a.md"),
        SourceFile::regular("A.MD", "x"),
        SourceFile::regular("assets/X.png", "x"),
        SourceFile::regular("assets/x.png", "y"),
        md("other.md"),
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
            ("a.md".to_owned(), "A.MD".to_owned()),
            ("assets/x.png".to_owned(), "assets/X.png".to_owned()),
            ("readme.md".to_owned(), "README.md".to_owned()),
        ]
    );
}

#[test]
fn a_link_to_a_reserved_file_says_why() {
    let s = site(vec![
        md("_x.md"),
        SourceFile::regular("home.md", "---\ntitle: H\n---\n[a](/_x) [b](/nope)\n"),
    ]);
    let whys: Vec<(String, NoPage)> = s
        .findings()
        .iter()
        .filter_map(|f| match f {
            Finding::BrokenPageLink { path, why, .. } => Some((path.clone(), why.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        whys,
        [
            (
                "_x".to_owned(),
                NoPage::Reserved(std::sync::Arc::new(Reserved::Underscore))
            ),
            ("nope".to_owned(), NoPage::Missing),
        ]
    );
}

#[test]
fn check_new_page_is_the_whole_rule_against_the_site() {
    let s = site(vec![
        md("Guide.md"),
        md("README.md"),
        md("home.md"),
        md("sec/x.md"),
        md("abc.md"),
    ]);
    let other = |f: &str| {
        Err(NewPageProblem::DuplicateUrl {
            other: std::sync::Arc::from(f),
        })
    };
    assert_eq!(s.check_new_page("guide.md"), other("Guide.md"));
    assert_eq!(s.check_new_page("readme.md"), other("README.md"));
    assert_eq!(s.check_new_page("sec/x.md"), other("sec/x.md"));
    assert_eq!(
        s.check_new_page("sec/index.md"),
        Err(NewPageProblem::IndexPageWithoutSection)
    );
    assert_eq!(
        s.check_new_page("home/index.md"),
        Err(NewPageProblem::IndexPageWithoutSection)
    );
    assert_eq!(
        s.check_new_page("abc/index.md"),
        Ok(PagePath::new("abc/index").unwrap())
    );
    assert_eq!(
        s.check_new_page("README.md"),
        Err(NewPageProblem::NotAPage(NotAPage::Ignored))
    );
    assert_eq!(
        s.check_new_page("new/page.md"),
        Ok(PagePath::new("new/page").unwrap())
    );
    // And it is what the site does with the file added, on every generated
    // pair: a page with no DuplicateUrl or IndexPageWithoutSection naming it.
    let pieces = [
        "a", "A", "abc", "Abc", "index", "home", "t", "_x", "README", "x.md", "é", "e\u{301}",
        "sec", "SEC",
    ];
    let mut n = 0;
    for a in pieces {
        for b in pieces {
            for existing in [
                format!("{a}.md"),
                format!("{a}/{b}.md"),
                a.to_owned(),
                format!("{a}/{b}"),
            ] {
                for new in [
                    format!("{b}.md"),
                    format!("{b}/{a}.md"),
                    format!("{a}/index.md"),
                ] {
                    let before = site(vec![md(&existing)]);
                    let after = site(vec![md(&existing), md(&new)]);
                    let page = page_path_for_new_file(&new, &SiteConfig::default()).ok();
                    let clean = page.as_ref().is_some_and(|p| after.page(p).is_some())
                        && !after.findings().iter().any(|f| match f {
                            Finding::DuplicateUrl { file, other } => {
                                **file == *new || **other == *new
                            }
                            Finding::IndexPageWithoutSection { file } => **file == *new,
                            Finding::DirectoryFileConflict { file, other } => {
                                **file == *new || **other == *new
                            }
                            _ => false,
                        });
                    assert_eq!(
                        before.check_new_page(&new).is_ok(),
                        clean,
                        "{existing:?} then {new:?}: {:?}",
                        before.check_new_page(&new)
                    );
                    n += 1;
                }
            }
        }
    }
    assert!(n > 2_000);
}

#[test]
fn a_directory_and_a_file_of_one_name_conflict() {
    // Git cannot hold a blob and a tree of one name, nor APFS two names one
    // apart in case or normalisation: a file `notes` (or `Notes`) beside
    // `notes/x.md`, `xyz.md` beside `xyz.md/y.md`, and a directory spelled
    // two ways (`assets` and `Assets`, `é` and `e` + U+0301).
    for (existing, new) in [
        ("notes", "notes/x.md"),
        ("Notes", "notes/x.md"),
        ("xyz.md/y.md", "xyz.md"),
        ("assets/x.png", "Assets/x.md"),
        ("docs/\u{e9}/b.md", "docs/e\u{301}/c.md"),
        ("docs/b/c.md", "Docs/b.md"),
    ] {
        let s = site(vec![md(existing)]);
        assert_eq!(
            s.check_new_page(new),
            Err(NewPageProblem::DirectoryFileConflict {
                other: std::sync::Arc::from(existing)
            }),
            "{existing:?} then {new:?}"
        );
        let both = site(vec![md(existing), md(new)]);
        let conflicts: Vec<(String, String)> = both
            .findings()
            .iter()
            .filter_map(|f| match f {
                Finding::DirectoryFileConflict { file, other } => {
                    Some((file.to_string(), other.to_string()))
                }
                _ => None,
            })
            .collect();
        let (first, second) = if existing < new {
            (existing, new)
        } else {
            (new, existing)
        };
        assert_eq!(
            conflicts,
            [(second.to_owned(), first.to_owned())],
            "{existing:?} and {new:?}"
        );
        assert_eq!(
            both.findings()
                .iter()
                .find(|f| matches!(f, Finding::DirectoryFileConflict { .. }))
                .map(|f| f.severity(CheckOptions::default())),
            Some(Severity::Error)
        );
    }
    // A second spelling is reported once, at its first file, not once per
    // file under it.
    let s = site(vec![
        md("Assets/x.md"),
        md("assets/a.png"),
        md("assets/b/c.png"),
        md("assets/d.png"),
    ]);
    let conflicts: Vec<(&str, &str)> = s
        .findings()
        .iter()
        .filter_map(|f| match f {
            Finding::DirectoryFileConflict { file, other } => Some((&**file, &**other)),
            _ => None,
        })
        .collect();
    assert_eq!(conflicts, [("assets/a.png", "Assets/x.md")]);
    // One directory spelled one way, a file beside it, a file with its
    // name and another extension: no conflict.
    let s = site(vec![md("notes/a.md"), md("notes.md"), md("notes.txt")]);
    assert_eq!(
        s.check_new_page("notes/b.md"),
        Ok(PagePath::new("notes/b").unwrap())
    );
    assert!(!s
        .findings()
        .iter()
        .any(|f| matches!(f, Finding::DirectoryFileConflict { .. })));
}

/// A small xorshift generator, so the generated sites are the same on
/// every run.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

#[test]
fn one_spelling_conflict_does_not_hide_another() {
    // A second spelling was reported once per directory by
    // suppressing every later conflict whose first hit was that spelling,
    // which hid a file-against-directory conflict deeper down
    // (`assets/b/c.png` against the file `assets/B`). Only that spelling is
    // suppressed now; deeper prefixes are still checked.
    let conflicts = |s: &Site| -> Vec<(String, String)> {
        s.findings()
            .iter()
            .filter_map(|f| match f {
                Finding::DirectoryFileConflict { file, other } => {
                    Some((file.to_string(), other.to_string()))
                }
                _ => None,
            })
            .collect()
    };
    let s = site(vec![
        md("Assets/x.md"),
        SourceFile::regular("assets/B", "x"),
        SourceFile::regular("assets/b/c.png", "x"),
    ]);
    assert_eq!(
        conflicts(&s),
        [
            ("assets/B".to_owned(), "Assets/x.md".to_owned()),
            ("assets/b/c.png".to_owned(), "assets/B".to_owned()),
        ]
    );
    // A deeper directory spelled as the first spelling's, under the second
    // spelling of its parent, is that one conflict, not another per level.
    let s = site(vec![
        md("A/b/x.md"),
        md("a/B/w.md"),
        md("a/b/y.md"),
        md("a/b/c/z.md"),
    ]);
    assert_eq!(
        conflicts(&s),
        [("a/B/w.md".to_owned(), "A/b/x.md".to_owned())]
    );
    // A second spelling of a deeper directory under an already reported one
    // is its own conflict.
    let s = site(vec![
        md("A/b/x.md"),
        md("a/b/y.md"),
        md("a/c/q.md"),
        md("a/C/z.md"),
    ]);
    assert_eq!(
        conflicts(&s),
        [
            ("a/C/z.md".to_owned(), "A/b/x.md".to_owned()),
            ("a/c/q.md".to_owned(), "a/C/z.md".to_owned()),
        ]
    );
}

#[test]
fn check_new_page_is_a_rebuild_on_every_conflict_free_site() {
    // 0 to 4 files already there, of 1 to 3 segments: folds (`a` and `A`,
    // `ß`, `ss` and `SS`, `k` and the Kelvin sign, `é` and `e` + U+0301),
    // `.png` and bare files, reserved names, README and `scripts/`. The
    // claim is for sites with no DirectoryFileConflict: on one that has
    // one, `check_new_page` answers for the new file against the files
    // there, where a rebuild may name only the conflict already there.
    let pieces = [
        "a", "A", "ß", "ss", "SS", "k", "\u{212A}", "\u{e9}", "e\u{301}", "index", "_x", "t",
        "README", "scripts", "assets", "x.md", "home",
    ];
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let path = |rng: &mut Rng, ext: &str| {
        let n = 1 + rng.below(3);
        let segs: Vec<&str> = (0..n).map(|_| pieces[rng.below(pieces.len())]).collect();
        segs.join("/") + ext
    };
    let (mut n, mut ok, mut dirs, mut spellings) = (0, 0, 0, 0);
    while n < 100_000 {
        let existing: Vec<String> = (0..rng.below(5))
            .map(|_| {
                let ext = [".md", ".md", ".png", ""][rng.below(4)];
                path(&mut rng, ext)
            })
            .collect();
        let ext = if rng.below(8) == 0 { ".png" } else { ".md" };
        let new = path(&mut rng, ext);
        let before = site(existing.iter().map(|p| md(p)).collect());
        if before
            .findings()
            .iter()
            .any(|f| matches!(f, Finding::DirectoryFileConflict { .. }))
        {
            continue;
        }
        n += 1;
        let after = site(existing.iter().chain([&new]).map(|p| md(p)).collect());
        let page = page_path_for_new_file(&new, &SiteConfig::default()).ok();
        let clean = page.as_ref().is_some_and(|p| after.page(p).is_some())
            && !after.findings().iter().any(|f| match f {
                Finding::DuplicateUrl { file, other }
                | Finding::DirectoryFileConflict { file, other } => {
                    **file == *new || **other == *new
                }
                Finding::IndexPageWithoutSection { file } => **file == *new,
                _ => false,
            });
        let answer = before.check_new_page(&new);
        assert_eq!(
            answer.is_ok(),
            clean,
            "{existing:?} then {new:?}: {answer:?}, {:?}",
            after.findings()
        );
        ok += usize::from(clean);
        // The refusals with no DuplicateUrl anywhere: a file against a
        // directory, or a directory spelled a second way.
        if let Err(NewPageProblem::DirectoryFileConflict { other }) = &answer {
            if !after
                .findings()
                .iter()
                .any(|f| matches!(f, Finding::DuplicateUrl { .. }))
            {
                if new.starts_with(&format!("{other}/")) || other.starts_with(&format!("{new}/")) {
                    dirs += 1;
                } else {
                    spellings += 1;
                }
            }
        }
    }
    eprintln!("paths: {n} conflict-free sites, {ok} accepted, {dirs} file-against-directory and {spellings} second-spelling refusals");
    assert!(
        ok > 10_000 && dirs > 50 && spellings > 150,
        "{ok} {dirs} {spellings}"
    );
}

#[test]
fn a_path_at_each_limit_is_accepted_and_one_over_is_refused() {
    // Every repository path, of any kind, is held to MAX_PATH_BYTES and
    // MAX_PATH_SEGMENTS before anything else: at the limit it is used, one
    // byte or one segment over it is `PathTooLong` and nothing else.
    let long = |bytes: usize, tail: &str| "p".repeat(bytes - tail.len()) + tail;
    let deep = |segments: usize, tail: &str| "ddd/".repeat(segments - 1) + tail;
    let cases: [(String, bool); 6] = [
        (long(MAX_PATH_BYTES, ".md"), true),
        (long(MAX_PATH_BYTES + 1, ".md"), false),
        (deep(MAX_PATH_SEGMENTS, "p.md"), true),
        (deep(MAX_PATH_SEGMENTS + 1, "p.md"), false),
        (format!("assets/{}", long(MAX_PATH_BYTES - 7, ".png")), true),
        (
            format!("assets/{}", long(MAX_PATH_BYTES - 6, ".png")),
            false,
        ),
    ];
    for (path, fits) in cases {
        let (bytes, segments) = (path.len(), path.split('/').count());
        let s = site(vec![md(&path)]);
        let new = site(vec![]).check_new_page(&path);
        let alone = page_path_for_new_file(&path, &SiteConfig::default());
        if fits {
            assert!(
                s.findings().is_empty(),
                "{bytes} {segments}: {:?}",
                s.findings()
            );
            assert_eq!(s.pages().len() + s.assets().len(), 1);
            if path.ends_with(".md") {
                assert!(new.is_ok() && alone.is_ok(), "{new:?}");
            }
        } else {
            let shown = path[..200].to_owned() + "…";
            assert_eq!(
                s.findings(),
                [Finding::PathTooLong {
                    file: std::sync::Arc::from(shown),
                    bytes,
                    segments
                }]
            );
            assert_eq!(s.pages().len() + s.assets().len(), 0);
            let why = if bytes > MAX_PATH_BYTES {
                BadPath::TooLong
            } else {
                BadPath::TooDeep
            };
            if path.ends_with(".md") {
                assert_eq!(new, Err(NewPageProblem::NotAPage(NotAPage::BadPath(why))));
                assert_eq!(alone, Err(NotAPage::BadPath(why)));
            }
        }
    }
    // Of any kind, and twice: `PathTooLong` each time, no NotARegularFile
    // nor DuplicateUrl. The shown prefix ends at a character boundary.
    let over = "é".repeat(MAX_PATH_BYTES);
    let link = SourceFile {
        kind: FileKind::Symlink,
        ..SourceFile::regular(over.clone(), "x")
    };
    let s = site(vec![link, md(&over), md(&over)]);
    let shown = "é".repeat(100) + "…";
    assert_eq!(s.findings().len(), 3, "{:?}", s.findings());
    assert!(s.findings().iter().all(|f| *f
        == Finding::PathTooLong {
            file: std::sync::Arc::from(shown.as_str()),
            bytes: 2 * MAX_PATH_BYTES,
            segments: 1
        }));
}

/// A `parsePath` row: the raw path, and (locale, path, explicit locale),
/// or `None` where it throws.
type ParseRow = (String, Option<(String, String, bool)>);

#[derive(Deserialize)]
struct PathDifferential {
    page_js_sha256: String,
    lodash_sha256: String,
    parse_path: Vec<ParseRow>,
    get_page_path: Vec<(String, String, String)>,
}

#[test]
fn parse_path_and_get_page_path_match_wikijs_on_every_differential_row() {
    // fixtures/wikijs-path-differential.json: Wiki.js 2.5.314's own
    // `parsePath` and `getPagePath`, verbatim from the container, with
    // lodash 4.17.21 (tools/gen-wikijs-path-differential.js).
    let file = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wikijs-path-differential.json"
    ));
    let bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    let d: PathDifferential = serde_json::from_slice(&bytes).expect("the differential parses");
    assert_eq!(
        d.page_js_sha256,
        "26e161f799d218e179b67f2c775461a8d6560f7f48e05f21472c9bcf3133e9c9"
    );
    assert_eq!(
        d.lodash_sha256,
        "4c04561befdf653aef017a42ac5addf68ea943cdfca6bdee5ce04e04e8139f54"
    );
    assert!(d.parse_path.len() >= 20_000 && d.get_page_path.len() >= 10_000);
    let mut bad = Vec::new();
    for (raw, want) in &d.parse_path {
        let got = wikijs_parse_path(raw).map(|p| (p.locale, p.path, p.explicit_locale));
        if got != *want {
            bad.push(format!(
                "  parsePath({raw:?}): wikijs {want:?}, rubrica {got:?}"
            ));
        }
    }
    for (f, locale, path) in &d.get_page_path {
        let got = wikijs_page_path(f);
        if (&got.locale, &got.path) != (locale, path) {
            bad.push(format!(
                "  getPagePath({f:?}): wikijs ({locale:?}, {path:?}), rubrica {got:?}"
            ));
        }
    }
    assert!(bad.is_empty(), "{}", bad[..bad.len().min(20)].join("\n"));
}

#[test]
fn findings_do_not_depend_on_the_order_files_come_in() {
    // `PathTooLong` was pushed before the files
    // were sorted, so its order was the caller's (a directory walk's); and
    // its 200-byte cut lets two different paths show the same prefix. The
    // same files in two orders give the same findings, in the same order.
    let long = |tail: &str| format!("{}{tail}", "x".repeat(MAX_PATH_BYTES));
    let deep = |tail: &str| format!("{}/{tail}", vec!["d"; 70].join("/"));
    let files = vec![
        md("home.md"),
        md(&long("a.md")),
        md(&long("bb.md")),
        SourceFile {
            kind: FileKind::Symlink,
            ..SourceFile::regular(long("ccc.md"), "x")
        },
        md(&deep("x.md")),
        md(&deep("y/z.md")),
        md("Home.md"),
        md("dup/a.md"),
        md("Dup/b.md"),
    ];
    let fwd = site(files.clone());
    let rev = site(files.into_iter().rev().collect());
    assert_eq!(fwd.findings(), rev.findings());
    let too_long = fwd
        .findings()
        .iter()
        .filter(|f| matches!(f, Finding::PathTooLong { .. }))
        .count();
    assert_eq!(too_long, 5, "{:?}", fwd.findings());
}
