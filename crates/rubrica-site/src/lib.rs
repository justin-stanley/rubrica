//! The corpus and the static build of a [Rubrica](https://github.com/justin-stanley/rubrica)
//! wiki: the page set and URLs, navigation, tables of contents, the link graph, the search
//! index and the templates, built from in-memory source files so that the static build and
//! a live server render identically.
//!
//! [`Site::from_sources`] is pure: the caller reads the files (from a git tree or a
//! directory) and this crate never touches a file system. [`build::build`] hands each output
//! file to a callback, so the caller decides where it goes. Use this crate to build a Rubrica
//! site from your own code; to build one from a git repository, use the
//! [`rubrica`](https://docs.rs/rubrica) command-line tool, which is this crate plus a git
//! reader.
//!
//! # Example
//!
//! ```rust
//! use rubrica_site::{Site, SiteConfig, SourceFile};
//!
//! let files = vec![
//!     SourceFile::regular("home.md", "---\ntitle: Home\n---\n# Home\n\nSee [notes](/notes).\n"),
//!     SourceFile::regular("notes.md", "---\ntitle: Notes\n---\n# Notes\n"),
//! ];
//! let site = Site::from_sources(files, &SiteConfig::default());
//!
//! assert_eq!(site.pages().len(), 2);
//! assert!(site.findings().is_empty()); // no dead links, no front matter problems
//! ```
//!
//! `examples/build.rs` goes on to write the site: `cargo run -p rubrica-site --example build`.
//!
//! # Features
//!
//! - `search-wasm` (on by default) embeds the search page's WebAssembly from
//!   [`rubrica-wasm-bundle`](https://docs.rs/rubrica-wasm-bundle). Build with
//!   `--no-default-features` to leave it out; the bundle's files must exist to build with it.
//!
//! # The other crates
//!
//! - [`rubrica-markdown`](https://docs.rs/rubrica-markdown) parses the Markdown.
//! - [`rubrica-render`](https://docs.rs/rubrica-render) renders each page to sanitized HTML.
//! - [`rubrica-wasm-bundle`](https://docs.rs/rubrica-wasm-bundle) is the prebuilt search script.
//! - [`rubrica`](https://docs.rs/rubrica) is the command-line tool.
//!
//! # Minimum supported Rust version
//!
//! Rust 1.94. A raise is a minor-version change.
//!
//! # Licence
//!
//! AGPL-3.0-only.
//!
//! # Bounds
//!
//! Every repository path, of any file, longer than [`MAX_PATH_BYTES`] or
//! of more than [`MAX_PATH_SEGMENTS`] segments is
//! [`Finding::PathTooLong`] before any other work on it, and takes part in
//! nothing else, so every bound below holds for every path.
//!
//! Its time is linear (or `n log n`) in the pages, their bytes and their
//! links, but for four factors: a page's breadcrumbs cost its path's
//! length times its depth (at most [`MAX_PATH_BYTES`] ×
//! [`MAX_PATH_SEGMENTS`], once per directory); any file's directory check
//! ([`Finding::DirectoryFileConflict`]) hashes each of its directories'
//! fold keys at most three times, and that directory's own spelling once
//! more where it is spelled otherwise than first; its own fold key twice;
//! and its path at most twice more (as a `.md` file, and to record a second
//! spelling). Each fold key is a prefix of the file's, which is at most
//! three times its bytes (full case folding over NFD triples a character's
//! bytes at most: `ΐ`), so at most 10 × [`MAX_PATH_BYTES`] ×
//! [`MAX_PATH_SEGMENTS`] bytes per file (`links::ops` counts each); a
//! relative link costs its
//! page's path length (at most [`MAX_PATH_BYTES`]) on top of its own, since
//! it is resolved against that path; and check-anchors' word-overlap search
//! spends at most its budget ([`SUGGESTION_BUDGET_PER_BYTE`]).
//!
//! Its memory is linear. What the [`Site`] holds: no per-link or per-page
//! structure copies a string of the page, heading, section, directory or
//! stub it points at; those are shared ([`std::sync::Arc`], [`PagePath`],
//! [`DirPath`], a prefix of a page's own path) and a finding's `fix:` is
//! built when asked for ([`Finding::fix`]). `tests/memory.rs` accounts it
//! (held memory, not peak). The peak while building adds to that the input
//! (each file's bytes until its render is made), the renders, each page's
//! heading ids and texts (one copy, for the anchor check) and one
//! suggestion index ([`links`]' `LooseIndex`, linear in one target page's
//! heading ids): the links are checked first, and the suggestions after,
//! one target page at a time, each index dropped before the next is built.
//! `tests/memory.rs` checks that one is alive at a time and the index's
//! size per id byte.

#![warn(missing_docs)]

pub mod build;
pub mod cache;
pub mod finding;
pub mod links;
mod loose_tables;
pub mod nav;
pub mod page;
mod path_tables;
pub mod source;
pub mod toc;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use rubrica_render::frontmatter::FrontMatterChecks;
use rubrica_render::markdown::classify;
use rubrica_render::sec::check_packages_markers;
use rubrica_render::{render_page, Highlighter, LinkKind, Profile, RenderFinding, RenderOptions};

pub use cache::{NoCache, RenderCache, RenderKey, RenderResult};
pub use finding::{AnchorProblem, Budget, CheckOptions, Finding, Suggestion, WikijsPathProblem};
pub use links::{loose, render_fragment, NoPage, Target};
pub use nav::{Crumb, DirPath, NavEntry, NavSection, NavTree};
pub use page::{Page, PageLink, Redirect};
pub use source::{
    page_path_for_new_file, BadOrigin, BadPath, FileKind, NotAPage, Origin, PagePath, RepoBudget,
    Reserved, SiteConfig, SourceFile, WikijsPage, ASSET_BUDGET_ERROR, ASSET_BUDGET_WARN,
    INVISIBLE_LETTERS, MAX_ASSET_BYTES, MAX_PATH_BYTES, MAX_PATH_SEGMENTS, MAX_REPO_BYTES,
    MAX_REPO_FILES, MAX_REPO_PATH_BYTES, MAX_REPO_TREE_BYTES,
};
pub use toc::TocEntry;

use links::{AnchorIndex, HeadingRef, LooseIndex};
use source::FileRole;

/// A file under `assets/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// Repo-relative path (`assets/a/b.png`).
    pub path: String,
    /// Its bytes.
    pub bytes: Vec<u8>,
    /// The git blob id, when the caller gave one.
    pub blob_id: Option<String>,
}

/// The whole site: one snapshot of the corpus, which `build` writes out and
/// `serve` serves.
#[derive(Debug, Clone, PartialEq)]
pub struct Site {
    pages: BTreeMap<PagePath, Page>,
    nav: NavTree,
    backlinks: BTreeMap<PagePath, Vec<PagePath>>,
    assets: Vec<Asset>,
    redirects: BTreeMap<PagePath, Redirect>,
    findings: Vec<Finding>,
    /// Every repository path's [`source::fold_key`], to its file: for
    /// [`Site::check_new_page`].
    folded: HashMap<Arc<str>, Arc<str>>,
    /// Every directory of a repository path, folded (a prefix of its
    /// file's key in `folded`), to its first spelling (a prefix of that
    /// file's path).
    dirs: HashMap<DirPath, DirPath>,
    cfg: SiteConfig,
}

/// Why a new page file cannot be added to a site ([`Site::check_new_page`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewPageProblem {
    /// The file would be no page, whatever else is there.
    NotAPage(NotAPage),
    /// A file already there is the same name on a case- or
    /// normalisation-insensitive disk ([`Finding::DuplicateUrl`]), or the
    /// same path.
    DuplicateUrl {
        /// That file.
        other: Arc<str>,
    },
    /// It, or one of its directories, is one name on such a disk with a
    /// directory or file already there, spelled otherwise or of the other
    /// kind ([`Finding::DirectoryFileConflict`]).
    DirectoryFileConflict {
        /// A file there: the file itself, or one under that directory.
        other: Arc<str>,
    },
    /// It would be `a/index.md` with no page `a` ([`Finding::IndexPageWithoutSection`]).
    IndexPageWithoutSection,
}

/// Whether page `path` is `a/index` with no published page `a` (or under
/// `home/`, which has no index: `home.md` is the root's, as [`nav`] has it).
fn index_without_section(path: &PagePath, pages: &BTreeMap<PagePath, Page>) -> bool {
    let dir = path.dir();
    path.name() == "index"
        && !dir.is_empty()
        && (dir == PagePath::HOME || !PagePath::new(dir).is_ok_and(|d| pages.contains_key(&d)))
}

/// The file that a file at `path`, fold key `key`, is one name with on
/// APFS but not of its kind, or whose directory it spells otherwise
/// ([`Finding::DirectoryFileConflict`]), against `files` and `dirs` (as
/// [`Site`] holds them), and for a second spelling, the length of the
/// directory (`path[..len]`) so spelled. A fold key's `/`s are its
/// path's, one for one.
///
/// A directory spelled as in `reported` (second spellings already
/// reported) is passed over, and the deeper ones still checked, so one
/// spelling conflict hides no other. A directory whose
/// own segment is spelled as the first spelling's, under a parent spelled
/// otherwise, is that parent's conflict, not its own: passed over too.
fn dir_conflict(
    path: &str,
    key: &str,
    files: &HashMap<Arc<str>, Arc<str>>,
    dirs: &HashMap<DirPath, DirPath>,
    reported: &HashSet<DirPath>,
) -> Option<(Arc<str>, Option<usize>)> {
    // Each hash of a prefix of `key` counts its bytes (`links::ops`).
    links::tick(key.len() as u64);
    if let Some(d) = dirs.get(key) {
        return Some((d.buffer().clone(), None));
    }
    let cuts = path.match_indices('/').zip(key.match_indices('/'));
    for ((raw, _), (folded, _)) in cuts {
        links::tick(2 * folded as u64);
        if let Some(f) = files.get(&key[..folded]) {
            return Some((f.clone(), None));
        }
        if let Some(d) = dirs.get(&key[..folded]) {
            if d.as_str() != &path[..raw] && last_segment(d.as_str()) != last_segment(&path[..raw])
            {
                links::tick(raw as u64);
                if !reported.contains(&path[..raw]) {
                    return Some((d.buffer().clone(), Some(raw)));
                }
            }
        }
    }
    None
}

/// The last segment of a path.
fn last_segment(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Units of suggestion search the site may spend per source byte (plus
/// [`SUGGESTION_BUDGET_BASE`]): check-anchors' word-overlap fallback visits,
/// for each broken anchor, every heading of the target that shares a word
/// with it, which hostile input can make quadratic. Every posting visited
/// counts; when the budget would run out, that search is skipped and
/// [`Finding::SuggestionsBounded`] says so.
pub const SUGGESTION_BUDGET_PER_BYTE: u64 = 16;

/// The part of the suggestion budget that does not depend on the corpus.
pub const SUGGESTION_BUDGET_BASE: u64 = 1 << 20;

/// The directories the `@sec` harvest never reads, at any depth: `.git`,
/// `node_modules`, and every hidden directory (a forge's own, such as
/// `.github`; Wiki.js's `check-packages.cjs` named its forge's directory
/// here, Rubrica names none). Every path segment is tested, the file
/// name included, so a hidden file (`.notes.md`) is skipped as well.
const CHECK_PACKAGES_SKIP: [&str; 2] = [".git", "node_modules"];

/// Whether the `@sec` harvest reads the file at `path`.
fn check_packages_reads(path: &str) -> bool {
    path.ends_with(".md")
        && !path
            .split('/')
            .any(|s| CHECK_PACKAGES_SKIP.contains(&s) || s.starts_with('.'))
}

/// Whether [`Site::from_sources`] reads the bytes of a regular file at
/// `path` under `cfg`: a page source, an asset, or a `.md` file
/// `check-packages.cjs` harvests (`README.md`, `scripts/`'s notes; not
/// under `node_modules`, `.git` or a hidden directory). Of any other file the site
/// uses the path alone, so a reader need not open it (`rubrica check`
/// does not).
pub fn reads_bytes(path: &str, cfg: &SiteConfig) -> bool {
    check_packages_reads(path)
        || matches!(
            source::file_role(path, cfg),
            FileRole::Page(_) | FileRole::Asset
        )
}

/// Whether a symbolic link or submodule at `path`, were it a directory,
/// would hold pages or assets: a page could be at `<path>/page.md`, or it
/// is (under) `assets/`. Under an ignored or reserved directory it holds
/// none.
fn could_hold_site_files(path: &str, cfg: &SiteConfig) -> bool {
    matches!(
        source::file_role(&format!("{path}/page.md"), cfg),
        FileRole::Page(_)
    ) || matches!(
        source::file_role(&format!("{path}/file"), cfg),
        FileRole::Asset
    )
}

/// Wiki.js's routes that answer before its page route, as segment
/// patterns (`*` is any one segment; `**` any rest, none included).
const WIKIJS_ROUTES: [&[&str]; 8] = [
    &["login"],
    &["login", "*"],
    &["login", "*", "callback"],
    &["logout"],
    &["register"],
    &["verify", "*"],
    &["login-reset", "*"],
    &["healthz"],
];

/// Wiki.js's `reservedPaths` (`server/app/data.yml`).
const WIKIJS_RESERVED: [&str; 9] = [
    "login", "logout", "register", "verify", "favicons", "fonts", "img", "js", "svg",
];

/// How `page` (at `file`) behaves differently in Wiki.js.
fn wikijs_path_problems(file: &str, page: &PagePath) -> Vec<WikijsPathProblem> {
    let mut out = Vec::new();
    let imported = source::wikijs_page_path(file);
    if imported.explicit_locale || imported.path != page.as_str() {
        out.push(WikijsPathProblem::ImportedAs(imported));
    }
    let segs: Vec<&str> = page.segments().collect();
    let route = segs[0].eq_ignore_ascii_case("graphql")
        || WIKIJS_ROUTES.iter().any(|r| {
            r.len() == segs.len()
                && r.iter()
                    .zip(&segs)
                    .all(|(want, got)| *want == "*" || want.eq_ignore_ascii_case(got))
        });
    if route {
        out.push(WikijsPathProblem::Route);
    } else if WIKIJS_RESERVED.contains(&segs[0]) {
        out.push(WikijsPathProblem::EditorReserved);
    }
    out
}

/// What a `.md` file that is not a page is, for the links to it.
struct Resolver<'a> {
    pages: &'a BTreeMap<PagePath, Page>,
    not_pages: &'a BTreeMap<PagePath, NoPage>,
    assets: &'a BTreeSet<String>,
    /// Every repository path ending in `.md`, page or not.
    md_files: &'a HashSet<Arc<str>>,
    cfg: &'a SiteConfig,
}

impl Resolver<'_> {
    /// The page at `segs` (the site's own key, so every link to a page
    /// shares its path), or the joined path.
    fn page_at(&self, segs: &[String]) -> Result<PagePath, String> {
        let joined = if segs.is_empty() {
            PagePath::HOME.to_owned()
        } else {
            segs.join("/")
        };
        match PagePath::new(&joined)
            .ok()
            .and_then(|p| self.pages.get_key_value(&p))
        {
            Some((k, _)) => Ok(k.clone()),
            None => Err(joined),
        }
    }

    fn resolve(&self, from: &links::RelativeBase, href: &str) -> Target {
        match classify(href) {
            LinkKind::External => match &self.cfg.wiki_origin {
                // html-core's `href.indexOf(host + '/') === 0`: exact, case
                // included. The rest starts with `/`, which is no origin, so
                // this does not recur.
                Some(origin)
                    if href
                        .strip_prefix(origin.as_str())
                        .is_some_and(|rest| rest.starts_with('/')) =>
                {
                    self.resolve_local(from, &href[origin.as_str().len()..])
                }
                _ => Target::External,
            },
            _ => self.resolve_local(from, href),
        }
    }

    /// `href`, never read against the wiki's origin.
    fn resolve_local(&self, from: &links::RelativeBase, href: &str) -> Target {
        match classify(href) {
            LinkKind::Anchor(fragment) => Target::SamePage { fragment },
            LinkKind::External => Target::External,
            LinkKind::Relative => {
                let cut = href.find(['?', '#']).unwrap_or(href.len());
                if cut == 0 {
                    // `?x`, `?x#y` or nothing: this page, as `#y` is (a
                    // browser and Wiki.js stay on it; check-frontmatter.cjs
                    // has no path left to check).
                    let fragment = match href.find('#').map(|i| classify(&href[i..])) {
                        Some(LinkKind::Anchor(f)) => f,
                        _ => String::new(),
                    };
                    return Target::SamePage { fragment };
                }
                let Some(segs) = links::resolve_relative(from, &href[..cut]) else {
                    return Target::Relative {
                        page: None,
                        file: None,
                    };
                };
                // check-frontmatter.cjs's file: `<path>.md`, or the path
                // when it ends in `.md`.
                let joined = if segs.is_empty() {
                    PagePath::HOME.to_owned()
                } else {
                    segs.join("/")
                };
                let file = if joined.ends_with(".md") {
                    self.md_files.get(joined.as_str())
                } else {
                    self.md_files.get(format!("{joined}.md").as_str())
                }
                .cloned();
                let page = self.page_at(&segs).ok().or_else(|| {
                    let mut segs = segs;
                    let last = segs.last_mut()?;
                    *last = last.strip_suffix(".md")?.to_owned();
                    if last.is_empty() {
                        return None;
                    }
                    self.page_at(&segs).ok()
                });
                debug_assert!(page.is_none() || file.is_some());
                Target::Relative { page, file }
            }
            LinkKind::Internal { path, fragment } => {
                if links::is_system_path(&path) {
                    return Target::System { path };
                }
                let segs = match links::clean_internal(&path) {
                    Ok(s) => s,
                    Err(e) => {
                        return Target::NoPage {
                            path: links::written_href(&path),
                            why: NoPage::Invalid(e),
                        }
                    }
                };
                match self.page_at(&segs) {
                    Ok(path) => Target::Page { path, fragment },
                    Err(joined) => {
                        if let Some(why) = PagePath::new(&joined)
                            .ok()
                            .and_then(|p| self.not_pages.get(&p))
                        {
                            return Target::NoPage {
                                path: joined,
                                why: why.clone(),
                            };
                        }
                        if segs.last().is_some_and(|s| s.contains('.')) {
                            let exists = self.assets.contains(&joined)
                                || joined
                                    .strip_suffix(".md")
                                    .and_then(|stem| PagePath::new(stem).ok())
                                    .is_some_and(|p| self.pages.contains_key(&p));
                            return Target::Asset {
                                path: joined,
                                exists,
                            };
                        }
                        Target::NoPage {
                            path: joined,
                            why: NoPage::Missing,
                        }
                    }
                }
            }
        }
    }
}

/// `Lnnn`, check-anchors' line citation (`/^L(\d+)$/`).
fn line_citation(raw_fragment: &str) -> Option<u64> {
    let digits = raw_fragment.strip_prefix('L')?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then(|| {
        digits.bytes().fold(0u64, |n, d| {
            n.saturating_mul(10).saturating_add(u64::from(d - b'0'))
        })
    })
}

/// `redirect_to`'s fragment as written, without `#`.
fn raw_fragment(href: &str) -> Option<&str> {
    href.find('#').map(|i| &href[i + 1..])
}

/// The options every page of a site renders with: the profile's preset. `build`
/// and `check` both render through `Site::from_sources*`, so they cannot
/// disagree. Fenced code with a known language is highlighted at build time.
pub fn render_options(profile: Profile) -> RenderOptions {
    match profile {
        Profile::Native => RenderOptions::native(),
        Profile::WikijsCompat => RenderOptions::wikijs_compat(),
    }
    .highlighter(Some(Highlighter(build::highlight::syntect_html)))
}

impl Site {
    /// The site of `files` under `cfg`. Pure: both `build` and `serve` call
    /// it. Files are taken in path order; a path given twice keeps its
    /// first file. Every page is rendered ([`NoCache`]).
    pub fn from_sources(files: Vec<SourceFile>, cfg: &SiteConfig) -> Site {
        Self::from_sources_with(files, cfg, &mut NoCache)
    }

    /// [`Site::from_sources`], taking each page's render from `cache` when
    /// it has one for the file's content ([`RenderKey`]: its blob id, or the
    /// SHA-256 of its bytes) and offering it every render it makes (shared:
    /// a cached render is not copied).
    pub fn from_sources_with(
        mut files: Vec<SourceFile>,
        cfg: &SiteConfig,
        cache: &mut impl RenderCache,
    ) -> Site {
        let opts = render_options(cfg.render);
        // The repository's budgets first, counting every entry, path byte
        // and file byte handed over (positive exclusion: nothing is left
        // out of the count). Over one, nothing else is done: no sort, no
        // fold, no page.
        let mut findings = Vec::new();
        let mut budget = source::RepoBudget::default();
        for f in &files {
            budget.entry(&f.path);
            budget.read(f.bytes.len() as u64);
        }
        if !budget.holds() {
            findings.extend(budget.findings());
            drop(files);
            return Site {
                pages: BTreeMap::new(),
                nav: NavTree {
                    root: NavSection {
                        dir: DirPath::root(),
                        title: Arc::from("Home"),
                        index: None,
                        entries: Vec::new(),
                    },
                },
                backlinks: BTreeMap::new(),
                assets: Vec::new(),
                redirects: BTreeMap::new(),
                findings,
                folded: HashMap::new(),
                dirs: HashMap::new(),
                cfg: cfg.clone(),
            };
        }
        // In path order before anything is reported, so the findings do not
        // depend on the order the caller lists the files in (a directory
        // walk's). Then positive exclusion: a path over the limits is
        // reported, from a bounded prefix of it, and takes part in nothing
        // else.
        files.sort_by(|a, b| a.path.cmp(&b.path));
        files.retain(|f| {
            if source::path_over_limits(&f.path).is_none() {
                return true;
            }
            let mut cut = 200.min(f.path.len());
            while !f.path.is_char_boundary(cut) {
                cut -= 1;
            }
            let ellipsis = if cut < f.path.len() { "…" } else { "" };
            findings.push(Finding::PathTooLong {
                file: Arc::from(format!("{}{ellipsis}", &f.path[..cut])),
                bytes: f.path.len(),
                segments: f.path.split('/').count(),
            });
            false
        });
        let total_bytes: u64 = files
            .iter()
            .map(|f| f.bytes.len() as u64 + f.path.len() as u64)
            .sum();
        let mut pages: BTreeMap<PagePath, Page> = BTreeMap::new();
        // `published: false`: not pages, but their links and anchors are
        // checked (as check-anchors.cjs checks every file).
        let mut drafts: BTreeMap<PagePath, Page> = BTreeMap::new();
        let mut not_pages: BTreeMap<PagePath, NoPage> = BTreeMap::new();
        let mut assets = Vec::new();
        // check-packages.cjs's harvest of every file it reads, in path order.
        let mut harvested: Vec<(Arc<str>, Vec<rubrica_render::sec::HarvestedMarker>)> = Vec::new();
        // The file lines of each render's own `DuplicateSecId`s.
        let mut render_dups: HashSet<(Arc<str>, usize)> = HashSet::new();
        // Every repository path, case- and normalisation-folded, for
        // DuplicateUrl: two files one name on APFS, whatever they are.
        let mut folded: HashMap<Arc<str>, Arc<str>> = HashMap::new();
        let mut folded_dirs: HashMap<DirPath, DirPath> = HashMap::new();
        // The second spellings of directories already reported, each once.
        let mut spelled_again: HashSet<DirPath> = HashSet::new();
        let mut md_files: HashSet<Arc<str>> = HashSet::new();
        let mut asset_bytes: u64 = 0;
        let mut previous: Option<Arc<str>> = None;
        for f in files {
            let file: Arc<str> = Arc::from(f.path.as_str());
            if previous.as_deref() == Some(f.path.as_str()) {
                findings.push(Finding::DuplicateUrl {
                    other: file.clone(),
                    file,
                });
                continue;
            }
            previous = Some(file.clone());
            let key: Arc<str> = Arc::from(source::fold_key(&f.path));
            if let Some((other, raw)) =
                dir_conflict(&f.path, &key, &folded, &folded_dirs, &spelled_again)
            {
                if let Some(raw) = raw {
                    links::tick(raw as u64);
                    spelled_again.insert(DirPath::prefix(&file, raw));
                }
                findings.push(Finding::DirectoryFileConflict {
                    file: file.clone(),
                    other,
                });
            }
            for ((raw, _), (at, _)) in f.path.match_indices('/').zip(key.match_indices('/')) {
                links::tick(at as u64);
                folded_dirs
                    .entry(DirPath::prefix(&key, at))
                    .or_insert_with(|| DirPath::prefix(&file, raw));
            }
            links::tick(key.len() as u64);
            match folded.entry(key) {
                std::collections::hash_map::Entry::Occupied(e) => {
                    findings.push(Finding::DuplicateUrl {
                        file: file.clone(),
                        other: e.get().clone(),
                    })
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(file.clone());
                }
            }
            if f.path.ends_with(".md") {
                links::tick(f.path.len() as u64);
                md_files.insert(file.clone());
            }
            let role = source::file_role(&f.path, cfg);
            if f.kind != source::FileKind::Regular {
                // Never read: not its bytes (a link's target path), nor
                // what it points at; a file over the reader's cap has none.
                // A link or submodule the site has no use for as a file is
                // still reported where, as a directory, it would hide pages
                // or assets.
                let directory = role == FileRole::Ignored
                    && matches!(
                        f.kind,
                        source::FileKind::Symlink | source::FileKind::Submodule
                    )
                    && could_hold_site_files(&f.path, cfg);
                if role != FileRole::Ignored || directory {
                    findings.push(match f.kind {
                        source::FileKind::TooLarge => Finding::FileTooLarge { file },
                        kind => Finding::NotARegularFile {
                            file,
                            kind,
                            directory,
                        },
                    });
                }
                continue;
            }
            if check_packages_reads(&f.path) {
                // As node's `readFileSync(f, 'utf8')` reads it.
                let markers = check_packages_markers(&String::from_utf8_lossy(&f.bytes));
                if !markers.is_empty() {
                    harvested.push((file.clone(), markers));
                }
            }
            let path = match role {
                FileRole::Ignored => continue,
                FileRole::Asset => {
                    if f.bytes.len() as u64 > MAX_ASSET_BYTES {
                        findings.push(Finding::AssetTooLarge {
                            file: file.clone(),
                            bytes: f.bytes.len() as u64,
                        });
                    }
                    asset_bytes += f.bytes.len() as u64;
                    assets.push(Asset {
                        path: f.path,
                        bytes: f.bytes,
                        blob_id: f.blob_id,
                    });
                    continue;
                }
                FileRole::MdUnderAssets => {
                    findings.push(Finding::MdUnderAssets { file });
                    continue;
                }
                FileRole::BadPath(why) => {
                    findings.push(Finding::BadSourcePath { file, why });
                    continue;
                }
                FileRole::Reserved(page, why) => {
                    not_pages.insert(page, NoPage::Reserved(Arc::new(why.clone())));
                    findings.push(Finding::ReservedPagePath { file, why });
                    continue;
                }
                FileRole::Page(p) => p,
            };
            if cfg.wikijs_limits {
                let chars = path.as_str().chars().count();
                if chars > 255 {
                    findings.push(Finding::PathTooLongForWikijs {
                        file: file.clone(),
                        chars,
                    });
                }
                for why in wikijs_path_problems(&f.path, &path) {
                    findings.push(Finding::PathDiffersInWikijs {
                        file: file.clone(),
                        why,
                    });
                }
            }
            let render_key = RenderKey::of(&f.bytes, f.blob_id.as_deref());
            let Ok(src) = String::from_utf8(f.bytes) else {
                findings.push(Finding::NotUtf8 { file });
                not_pages.insert(path, NoPage::NotUtf8);
                continue;
            };
            let render = match cache.get(&render_key) {
                Some(r) => r,
                None => {
                    let r = render_page(&src, &opts)
                        .map(|(front, rendered, offset)| (front, Arc::new(rendered), offset));
                    cache.put(render_key, &r);
                    r
                }
            };
            drop(src);
            let (front, rendered, line_offset) = match render {
                Ok(r) => r,
                Err(error) => {
                    findings.push(Finding::BadFrontMatter { file, error });
                    not_pages.insert(path, NoPage::BadFrontMatter);
                    continue;
                }
            };
            for finding in front.findings(FrontMatterChecks {
                wikijs_limits: cfg.wikijs_limits,
            }) {
                findings.push(Finding::FrontMatter {
                    file: file.clone(),
                    finding,
                });
            }
            for finding in &rendered.findings {
                let line = match finding {
                    RenderFinding::SecInFrontMatter { line, .. } => *line,
                    other => other.line() + line_offset,
                };
                if matches!(finding, RenderFinding::DuplicateSecId { .. }) {
                    render_dups.insert((file.clone(), line));
                }
                findings.push(Finding::Render {
                    file: file.clone(),
                    line,
                    finding: finding.clone(),
                });
            }
            // A redirect stub is no reading page: no TOC, and not in the nav.
            let toc = if front.redirect_to.is_some() {
                Vec::new()
            } else {
                toc::toc(&rendered.headings, line_offset)
            };
            let published = front.published;
            let page = Page {
                url: path.url(),
                path: path.clone(),
                file,
                blob_id: f.blob_id,
                front,
                rendered,
                line_offset,
                toc,
                breadcrumbs: Arc::from([]),
                section: None,
                links: Vec::new(),
                redirect_to: None,
            };
            if published {
                pages.insert(path, page);
            } else {
                not_pages.insert(path.clone(), NoPage::Unpublished);
                drafts.insert(path, page);
            }
        }
        if asset_bytes > ASSET_BUDGET_WARN {
            findings.push(Finding::AssetBudget { bytes: asset_bytes });
        }
        let asset_paths: BTreeSet<String> = assets.iter().map(|a| a.path.clone()).collect();

        // `a/index.md` with no page `a`: a static file server answers `/a` with it.
        for (path, page) in &pages {
            if index_without_section(path, &pages) {
                findings.push(Finding::IndexPageWithoutSection {
                    file: page.file.clone(),
                });
            }
        }

        // Links, anchors and redirects, on pages and drafts alike.
        let indexes: BTreeMap<&PagePath, AnchorIndex> = pages
            .iter()
            .chain(drafts.iter())
            .map(|(p, page)| {
                // A dropped heading's slug, which `Heading::id` does not
                // keep, is the slugger's again, fed the page's headings in
                // the same order.
                let mut slugs = rubrica_render::slug::Slugs::new();
                let headings = page
                    .rendered
                    .headings
                    .iter()
                    .map(|h| {
                        let slug = slugs.heading_id(&h.text).id;
                        HeadingRef {
                            id: Arc::from(h.id.clone().unwrap_or(slug)),
                            dropped: h.id.is_none(),
                            text: Arc::from(h.text.as_str()),
                            line: h.line + page.line_offset,
                            level: h.level,
                        }
                    })
                    .collect();
                let secs = page
                    .rendered
                    .headings
                    .iter()
                    .filter_map(|h| h.sec.as_ref().map(|s| s.anchor()));
                (p, AnchorIndex::new(headings, secs))
            })
            .collect();
        let resolver = Resolver {
            pages: &pages,
            not_pages: &not_pages,
            assets: &asset_paths,
            md_files: &md_files,
            cfg,
        };
        // Each published stub's target, resolved once: its page's URL and
        // `redirect_to`'s fragment, or `redirect_to` as written.
        let stubs: BTreeMap<PagePath, Arc<str>> = pages
            .iter()
            .filter_map(|(p, page)| {
                let r = page.front.redirect_to.as_deref()?;
                let target = match resolver.resolve(&links::RelativeBase::of(p), r) {
                    Target::Page { path, .. } => match raw_fragment(r) {
                        Some(f) => format!("{}#{f}", path.url()),
                        None => path.url(),
                    },
                    _ => r.to_owned(),
                };
                Some((p.clone(), Arc::from(target)))
            })
            .collect();
        // The broken anchors whose suggestion is check-anchors' search: the
        // finding's index and its target, searched after the links, one
        // target page at a time.
        let mut searches: Vec<(&PagePath, usize)> = Vec::new();
        let mut resolved: BTreeMap<PagePath, (Vec<PageLink>, Option<Redirect>)> = BTreeMap::new();
        let all = pages
            .iter()
            .map(|p| (p, true))
            .chain(drafts.iter().map(|p| (p, false)));
        for ((path, page), published) in all {
            let file = &page.file;
            let mut out = Vec::with_capacity(page.rendered.links.len());
            // The page's directory, split once for all its relative links.
            let base = links::RelativeBase::of(path);
            for link in &page.rendered.links {
                let line = link.line + page.line_offset;
                let target = resolver.resolve(&base, &link.href);
                let href = &link.href;
                match &target {
                    Target::NoPage { path: p, why } => findings.push(Finding::BrokenPageLink {
                        file: file.clone(),
                        line,
                        href: href.clone(),
                        path: p.clone(),
                        why: why.clone(),
                    }),
                    Target::Asset { exists: false, .. } => {
                        findings.push(Finding::BrokenAssetLink {
                            file: file.clone(),
                            line,
                            href: href.clone(),
                        })
                    }
                    Target::Relative {
                        page,
                        file: Some(reaches),
                    } => findings.push(Finding::RelativeLink {
                        file: file.clone(),
                        line,
                        href: href.clone(),
                        reaches: reaches.clone(),
                        to: page.clone(),
                    }),
                    Target::Page { path: to, .. }
                        if stubs.contains_key(to) && !stubs.contains_key(path) =>
                    {
                        findings.push(Finding::LinkToRedirectStub {
                            file: file.clone(),
                            line,
                            href: href.clone(),
                            stub: to.clone(),
                            target: stubs[to].clone(),
                        });
                    }
                    _ => {}
                }
                let anchor = match &target {
                    Target::Page {
                        path: to,
                        fragment: Some(f),
                    } if !stubs.contains_key(to) => Some((to, f, false)),
                    Target::SamePage { fragment } => Some((path, fragment, true)),
                    _ => None,
                };
                if let Some((to, fragment, same_page)) = anchor {
                    let (to, ix) = indexes.get_key_value(to).expect("a page or draft");
                    if !ix.reaches(fragment) {
                        let hash = href.find('#').unwrap_or(href.len());
                        let raw_fragment = href.get(hash + 1..).unwrap_or("");
                        let (why, suggestion) = if let Some(&i) = ix.dropped.get(fragment.as_str())
                        {
                            let text = ix.headings[i].text.clone();
                            (AnchorProblem::DroppedId { text }, None)
                        } else if let Some(n) = line_citation(raw_fragment) {
                            match ix.enclosing(n) {
                                None | Some((_, true)) => (
                                    AnchorProblem::LineCitation {
                                        n,
                                        drop_fragment: true,
                                    },
                                    None,
                                ),
                                // check-anchors would suggest a heading DOMPurify
                                // strips the id of; Rubrica suggests nothing.
                                Some((h, false)) => (
                                    AnchorProblem::LineCitation {
                                        n,
                                        drop_fragment: false,
                                    },
                                    (!h.dropped).then(|| Suggestion {
                                        id: h.id.clone(),
                                        text: h.text.clone(),
                                        line: h.line,
                                    }),
                                ),
                            }
                        } else {
                            // Filled in below, with the other searches of
                            // this target.
                            searches.push((*to, findings.len()));
                            (AnchorProblem::NoSuchId, None)
                        };
                        findings.push(Finding::BrokenAnchor {
                            file: file.clone(),
                            line,
                            href: href.clone(),
                            to: (*to).clone(),
                            same_page,
                            fragment: fragment.clone(),
                            why,
                            suggestion,
                        });
                    }
                }
                out.push(PageLink { line, target });
            }
            let mut redirect = None;
            if let Some(r) = &page.front.redirect_to {
                let line = page
                    .front
                    .key_lines
                    .get("redirect_to")
                    .copied()
                    .unwrap_or(1);
                match resolver.resolve(&base, r) {
                    Target::Page { path: to, .. } if to == *path || stubs.contains_key(&to) => {
                        findings.push(Finding::RedirectChain {
                            file: file.clone(),
                            line,
                            target: to,
                        })
                    }
                    Target::Page { path: to, .. } => {
                        redirect = Some(Redirect {
                            to,
                            fragment: raw_fragment(r).map(str::to_owned),
                        })
                    }
                    _ => findings.push(Finding::RedirectTargetMissing {
                        file: file.clone(),
                        line,
                        target: r.clone(),
                    }),
                }
            }
            if published {
                resolved.insert(path.clone(), (out, redirect));
            }
        }
        // check-anchors' search, one target page at a time (its links in
        // order), each target's index dropped before the next is built, so
        // one is alive at a time. The budget is spent in that order, and a
        // search it skips is reported once, at the first such link in the
        // order the links are checked: published pages, then drafts, each
        // in path order, and a page's links in order.
        searches.sort_by(|a, b| a.0.cmp(b.0));
        let mut budget = SUGGESTION_BUDGET_PER_BYTE
            .saturating_mul(total_bytes)
            .saturating_add(SUGGESTION_BUDGET_BASE);
        let mut first_lost: Option<usize> = None;
        for group in searches.chunk_by(|a, b| a.0 == b.0) {
            let ix = &indexes[group[0].0];
            let loose_ix = LooseIndex::build(&ix.headings);
            let mut memo: HashMap<String, (Option<usize>, bool)> = HashMap::new();
            for &(_, at) in group {
                let Finding::BrokenAnchor {
                    fragment,
                    suggestion,
                    ..
                } = &mut findings[at]
                else {
                    unreachable!("a search is for a broken anchor");
                };
                let (hit, lost) = match memo.get(fragment.as_str()) {
                    Some(r) => *r,
                    None => {
                        let r = loose_ix.suggest(fragment, &mut budget);
                        memo.insert(fragment.clone(), r);
                        r
                    }
                };
                if lost {
                    first_lost = Some(first_lost.map_or(at, |f| f.min(at)));
                }
                *suggestion = hit.map(|i| {
                    let h = &ix.headings[i];
                    Suggestion {
                        id: h.id.clone(),
                        text: h.text.clone(),
                        line: h.line,
                    }
                });
            }
            drop(loose_ix);
        }
        if let Some(at) = first_lost {
            let (file, line) = match &findings[at] {
                Finding::BrokenAnchor { file, line, .. } => (file.clone(), *line),
                _ => unreachable!("a search is for a broken anchor"),
            };
            findings.push(Finding::SuggestionsBounded { file, line });
        }
        drop(indexes);
        drop(drafts);

        // The link graph.
        let mut backlinks: BTreeMap<PagePath, Vec<PagePath>> = BTreeMap::new();
        let mut redirects = BTreeMap::new();
        for (from, (links, redirect)) in &resolved {
            for l in links {
                if let Target::Page { path, .. } = &l.target {
                    backlinks
                        .entry(path.clone())
                        .or_default()
                        .push(from.clone());
                }
            }
            if let Some(to) = redirect {
                redirects.insert(from.clone(), to.clone());
            }
        }
        for v in backlinks.values_mut() {
            v.sort();
            v.dedup();
            v.shrink_to_fit();
        }

        // `@sec` ids are repo-wide over every file check-packages.cjs
        // reads, and it reports each use after the first once. A marker
        // its page's render already reports (a second use within the page)
        // is not reported again.
        let mut first: HashMap<&str, (&Arc<str>, usize)> = HashMap::new();
        for (file, markers) in &harvested {
            for m in markers {
                let Some(sec) = &m.marker else { continue };
                match first.get(sec.id.as_str()) {
                    Some(&(first_file, first_line)) => {
                        if !render_dups.contains(&(file.clone(), m.line)) {
                            findings.push(Finding::DuplicateSecId {
                                file: file.clone(),
                                line: m.line,
                                id: sec.id.clone(),
                                first_file: first_file.clone(),
                                first_line,
                            });
                        }
                    }
                    None => {
                        first.insert(&sec.id, (file, m.line));
                    }
                }
            }
        }

        // Nav, breadcrumbs, sections; redirect stubs are left out of the
        // tree (and so are no section's index).
        let in_nav = || {
            pages
                .iter()
                .filter(|(p, _)| !stubs.contains_key(*p))
                .map(|(p, page)| (p, page.front.title.as_str()))
        };
        let dirs = nav::dirs(in_nav());
        let nav = nav::nav_tree(in_nav(), &dirs);
        let mut crumbs: HashMap<DirPath, Arc<[Crumb]>> = HashMap::new();
        for (path, page) in pages.iter_mut() {
            let dir = path.dir();
            if !path.is_home() {
                page.breadcrumbs = match crumbs.get(dir) {
                    Some(c) => c.clone(),
                    None => {
                        let c = nav::breadcrumbs(dir, &dirs);
                        crumbs.insert(DirPath::of(path), c.clone());
                        c
                    }
                };
            }
            page.section = (!dir.is_empty())
                .then(|| dirs.get(dir).and_then(|d| d.index.clone()))
                .flatten();
            if let Some((links, redirect)) = resolved.remove(path) {
                page.links = links;
                page.redirect_to = redirect;
            }
        }

        findings.sort_by(|a, b| (a.file(), a.line()).cmp(&(b.file(), b.line())));
        Site {
            pages,
            nav,
            backlinks,
            assets,
            redirects,
            findings,
            folded,
            dirs: folded_dirs,
            cfg: cfg.clone(),
        }
    }

    /// Whether a new file at repo path `path` (`a/b.md`) may be added to
    /// this site as a page, and which page: the whole rule, the file's own
    /// ([`page_path_for_new_file`], under the site's [`SiteConfig`]) and
    /// against the files already there (no file one name apart on APFS,
    /// [`Finding::DuplicateUrl`]; no file or directory one name with it or
    /// its directories but of the other kind or spelled otherwise,
    /// [`Finding::DirectoryFileConflict`]; not `a/index.md` without a page
    /// `a`, [`Finding::IndexPageWithoutSection`]). On a site with no
    /// [`Finding::DirectoryFileConflict`], it accepts exactly the files that
    /// a rebuild with the file added makes a page of, with no such finding
    /// naming it (`tests/paths.rs`); on one that has one, it answers for
    /// the new file against the files there, where a rebuild may name only
    /// the conflict already there. The editor and renames
    /// (against the site without the old file) name new files
    /// through this.
    ///
    /// # Errors
    ///
    /// A [`NewPageProblem`]: the path is no page ([`NotAPage`]), or the page would clash with
    /// a file or directory already there.
    pub fn check_new_page(&self, path: &str) -> Result<PagePath, NewPageProblem> {
        let page = page_path_for_new_file(path, &self.cfg).map_err(NewPageProblem::NotAPage)?;
        let key = source::fold_key(path);
        if let Some(other) = self.folded.get(key.as_str()) {
            return Err(NewPageProblem::DuplicateUrl {
                other: other.clone(),
            });
        }
        if let Some((other, _)) =
            dir_conflict(path, &key, &self.folded, &self.dirs, &HashSet::new())
        {
            return Err(NewPageProblem::DirectoryFileConflict { other });
        }
        if index_without_section(&page, &self.pages) {
            return Err(NewPageProblem::IndexPageWithoutSection);
        }
        Ok(page)
    }

    /// Every repository path's fold key and file: for the memory test.
    #[doc(hidden)]
    pub fn folded_paths(&self) -> impl Iterator<Item = (&str, &Arc<str>)> {
        self.folded.iter().map(|(k, v)| (&**k, v))
    }

    /// Every directory's fold key, to its spelling (a prefix of a file
    /// under it): for the memory test.
    #[doc(hidden)]
    pub fn folded_dirs(&self) -> impl Iterator<Item = (&DirPath, &DirPath)> {
        self.dirs.iter()
    }

    /// Every published page, by path.
    pub fn pages(&self) -> &BTreeMap<PagePath, Page> {
        &self.pages
    }

    /// One page.
    pub fn page(&self, path: &PagePath) -> Option<&Page> {
        self.pages.get(path)
    }

    /// The nav tree.
    pub fn nav(&self) -> &NavTree {
        &self.nav
    }

    /// The pages that link to `path`, sorted, each once (itself included if
    /// it links to itself).
    pub fn backlinks(&self, path: &PagePath) -> &[PagePath] {
        self.backlinks.get(path).map_or(&[], Vec::as_slice)
    }

    /// The whole inverse link graph.
    pub fn all_backlinks(&self) -> &BTreeMap<PagePath, Vec<PagePath>> {
        &self.backlinks
    }

    /// The files under `assets/`.
    pub fn assets(&self) -> &[Asset] {
        &self.assets
    }

    /// Redirect stubs and where they redirect.
    pub fn redirects(&self) -> &BTreeMap<PagePath, Redirect> {
        &self.redirects
    }

    /// Everything wrong, by file and line.
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }
}

#[cfg(test)]
mod tests {
    use super::check_packages_reads;

    #[test]
    fn hidden_directories_are_skipped_by_the_harvest() {
        assert!(!check_packages_reads(".github/a.md"));
        assert!(!check_packages_reads("a/.hidden/b.md"));
        assert!(!check_packages_reads(".notes.md"));
        assert!(check_packages_reads("scripts/notes.md"));
        assert!(!check_packages_reads("node_modules/x.md"));
    }
}
