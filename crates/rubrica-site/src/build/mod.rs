//! `rubrica build`: a [`Site`] written out as a static site, as the
//! files a plain file server answers with (a static file server configured as
//! `try_files {path}.html {path} {path}/index.html`).
//!
//! Pure, as the rest of the crate: the caller reads the git tree and its
//! history ([`History`]) and writes the files [`build`] hands it, in a
//! fixed order, each path once. The layout (`out/` below):
//! - `index.html` (the home page) and `home.html`, the same bytes, so `/`
//!   and `/home` serve the same page (the corpus links `/home`; without
//!   it the server would look for `home/index.html`);
//! - `<path>.html` for every other published page, and `<path>.md`, its
//!   source as committed, for every published page (`home.md` too);
//! - `_search.html`, `_search.json` (the search index, under `_` so no page
//!   can shadow it: a page `search.json` would answer `/search.json`
//!   first), `_recent.html`, `_health` (`ok rubrica <version> <commit>`),
//!   `404.html`;
//! - `_assets/`: the stylesheet and the icon, embedded in the binary, and, with the
//!   `search-wasm` feature, the search page's `wasm-bindgen` loader and module. The loader is
//!   the one script any page names, on `_search.html` alone, under a Content-Security-Policy
//!   of its own; every other page has no script and `script-src 'none'`;
//! - the changelog's Atom feed, when [`BuildConfig::feed`] names one.
//!
//! Every name a page could have that would collide with these is reserved
//! (`_…`, a top-level `index` or `404`, a segment ending in `.md` or
//! `.html`): such files are no pages, and [`build`] refuses a site that has
//! one ([`refusals`]).
//!
//! What a page's body is: [`rubrica_render`]'s sanitized HTML, then the
//! theme pass ([`theme`]), which wraps each heading that has an id with its
//! `¶` anchor and each table in a scroller, after the sanitizer and by
//! exact tag match on its output, never by widening it.

pub mod config;
mod feed;
pub(crate) mod highlight;
pub(crate) mod html;
mod search;
mod templates;
pub mod theme;

/// Whether [`build`] writes the search page's wasm bundle (its `.wasm` and the
/// `wasm-bindgen` loader): whenever the `search-wasm` feature, on by default, embeds it. The
/// search page then names the loader and has its own Content-Security-Policy
/// ([`templates::CSP_SEARCH_WASM`]); without the feature `_search` is the pages list alone
/// and no page names a script.
pub(crate) const SEARCH_WASM: bool = cfg!(feature = "search-wasm");

use std::collections::BTreeMap;
use std::fmt;
use std::io;

use crate::{Finding, Page, Site};

pub use config::{
    BuildConfig, ConfigFile, FeedConfig, Footer, HeaderLink, RenderName, SiteSection,
};

/// How many commits `_recent.html` lists.
pub const RECENT_COMMITS: usize = 50;

/// How many of a commit's pages `_recent.html` lists; the rest are counted.
pub const RECENT_PAGES_PER_COMMIT: usize = 50;

/// How many nav entries each page lists; past it the tree ends with a link
/// to `/_search`, whose page lists every page once. So a page's size does
/// not grow with the site's (a 66-page wiki's tree has 74 entries).
pub const NAV_INLINE_LIMIT: usize = 500;

/// The most bytes of nav each page has: past them, as past
/// [`NAV_INLINE_LIMIT`] links, the tree ends with the link to `/_search`.
/// The check is before each entry, so a nav is at most this, one entry
/// (a title of at most [`MAX_TITLE_CHARS`] characters and an href of at
/// most [`crate::MAX_PATH_BYTES`] bytes, percent-encoded) and the closing
/// tags of at most [`crate::MAX_PATH_SEGMENTS`] open sections.
pub const NAV_MAX_BYTES: usize = 64 << 10;

/// The longest title the nav, the breadcrumbs and recent changes show, in
/// characters; a longer one is cut there and ends with `…` (a
/// title is repeated on every page's nav). The page's own `<title>` and
/// heading, and search, keep it whole.
pub const MAX_TITLE_CHARS: usize = 120;

/// The most bytes [`build`] writes: a site whose output would
/// be larger is refused ([`BuildError::OutputOverBudget`]), the file that
/// would cross it unwritten. With the repository's 128 MiB of content
/// ([`crate::MAX_REPO_BYTES`]), output is at most 16 times the largest
/// input. Each file is charged at least [`MIN_FILE_CHARGE`] bytes.
pub const MAX_OUTPUT_BYTES: u64 = 2 << 30;

/// What a file costs the output budget at least, whatever its length:
/// a block of disk, which a tiny file takes too.
pub const MIN_FILE_CHARGE: u64 = 4096;

/// A commit, as the pages show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// The full id, in hex.
    pub id: String,
    /// The committer's time, in seconds since the epoch.
    pub time: i64,
    /// The committer's offset from UTC, in seconds east.
    pub offset: i32,
    /// The message's first line.
    pub summary: String,
}

impl Commit {
    fn datetime(&self) -> chrono::DateTime<chrono::FixedOffset> {
        let utc = chrono::DateTime::from_timestamp(self.time, 0).unwrap_or_default();
        // An offset chrono cannot hold (a day or more) is read as UTC.
        let offset = chrono::FixedOffset::east_opt(self.offset)
            .unwrap_or_else(|| chrono::FixedOffset::east_opt(0).expect("UTC"));
        utc.with_timezone(&offset)
    }

    /// The time as RFC 3339 in the committer's offset (git's `%cI`).
    pub fn iso(&self) -> String {
        self.datetime().format("%Y-%m-%dT%H:%M:%S%:z").to_string()
    }

    /// The date in the committer's offset (`2026-10-25`).
    pub fn date(&self) -> String {
        self.datetime().format("%Y-%m-%d").to_string()
    }

    /// The id's first 7 characters.
    pub fn short(&self) -> &str {
        &self.id[..self.id.len().min(7)]
    }
}

/// A commit of `_recent.html`, with the page files it changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentCommit {
    /// The commit.
    pub commit: Commit,
    /// The files of published pages it changed, in path order, at most
    /// [`RECENT_PAGES_PER_COMMIT`].
    pub files: Vec<String>,
    /// How many more it changed, left out of `files`.
    pub more: usize,
}

/// What the build needs from git: the commit it builds and the history
/// behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History {
    /// The commit built.
    pub commit: Commit,
    /// For each page file, the newest commit that changed it.
    pub last: BTreeMap<String, Commit>,
    /// The newest commits that changed a page, newest first, at most
    /// [`RECENT_COMMITS`].
    pub recent: Vec<RecentCommit>,
    /// The feed source's versions, newest first: each commit that changed
    /// it, with the file as that commit has it.
    pub feed_versions: Vec<(Commit, Vec<u8>)>,
    /// Whether the walk read all of the history it needed; when a budget
    /// stopped it, older dates are missing.
    pub complete: bool,
}

/// Why [`build`] wrote nothing, or stopped.
#[derive(Debug)]
pub enum BuildError {
    /// The site has findings it cannot be built with ([`refusals`]), or no
    /// published page.
    Refused,
    /// The configuration does not fit the site (the feed's source is no
    /// page, its URL collides with a page).
    Config(String),
    /// The output would be larger than its budget ([`MAX_OUTPUT_BYTES`]):
    /// nothing past it is written, and the caller removes what was.
    OutputOverBudget {
        /// The budget, in bytes.
        limit: u64,
    },
    /// Reading a source or writing a file failed.
    Io(io::Error),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused => f.write_str("the site has findings it cannot be built with"),
            Self::Config(s) => f.write_str(s),
            Self::OutputOverBudget { limit } => {
                write!(f, "the site would be more than {limit} bytes")
            }
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Refused | Self::Config(_) | Self::OutputOverBudget { .. } => None,
        }
    }
}

impl From<io::Error> for BuildError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// What [`build`] wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Built {
    /// Files written.
    pub files: usize,
    /// Published pages.
    pub pages: usize,
    /// Bytes written.
    pub bytes: u64,
}

/// The findings `site` cannot be built with ([`Finding::refuses_build`]).
pub fn refusals(site: &Site) -> Vec<&Finding> {
    site.findings()
        .iter()
        .filter(|f| f.refuses_build())
        .collect()
}

/// Write `site` out: every file of the layout above, through `write`
/// (a path relative to `out/`, `/`-separated, and the bytes), each once, in
/// a fixed order; `source` gives a page's file as committed (for
/// `<path>.md`). Refuses, writing nothing, a site with [`refusals`] or no
/// published home page (so none at all). The same site, configuration and history give the same
/// files, byte for byte.
///
/// # Errors
///
/// [`BuildError::Refused`] for a site with [`refusals`] or no home page,
/// [`BuildError::Config`] for a configuration that does not fit the site,
/// [`BuildError::OutputOverBudget`] when the output passes [`MAX_OUTPUT_BYTES`], and
/// [`BuildError::Io`] when `source` or `write` fails. On any error the caller removes
/// whatever `write` already received.
pub fn build(
    site: &Site,
    cfg: &BuildConfig,
    history: &History,
    source: &mut dyn FnMut(&Page) -> io::Result<Vec<u8>>,
    write: &mut dyn FnMut(&str, &[u8]) -> io::Result<()>,
) -> Result<Built, BuildError> {
    build_bounded(site, cfg, history, source, write, MAX_OUTPUT_BYTES)
}

/// [`build`] with the search page's wasm bundle ([`rubrica_wasm_bundle`]): its loader and
/// `.wasm` under `_assets/`, and on `_search.html` alone the search form and the one
/// `<script type="module">` that loads them. With the feature on (the default), [`build`]
/// does the same; this is its explicit form.
#[cfg(feature = "search-wasm")]
pub fn build_with_search_wasm(
    site: &Site,
    cfg: &BuildConfig,
    history: &History,
    source: &mut dyn FnMut(&Page) -> io::Result<Vec<u8>>,
    write: &mut dyn FnMut(&str, &[u8]) -> io::Result<()>,
) -> Result<Built, BuildError> {
    build_site(site, cfg, history, source, write, MAX_OUTPUT_BYTES, true)
}

/// [`build`], writing at most `max_output` bytes ([`MAX_OUTPUT_BYTES`]'s
/// place, for tests): the file that would take the total past it is not
/// written, and the build is [`BuildError::OutputOverBudget`].
pub fn build_bounded(
    site: &Site,
    cfg: &BuildConfig,
    history: &History,
    source: &mut dyn FnMut(&Page) -> io::Result<Vec<u8>>,
    write: &mut dyn FnMut(&str, &[u8]) -> io::Result<()>,
    max_output: u64,
) -> Result<Built, BuildError> {
    build_site(site, cfg, history, source, write, max_output, SEARCH_WASM)
}

/// [`build_bounded`], with the search bundle when `search_wasm` (only
/// `build_with_search_wasm` passes true).
fn build_site(
    site: &Site,
    cfg: &BuildConfig,
    history: &History,
    source: &mut dyn FnMut(&Page) -> io::Result<Vec<u8>>,
    write: &mut dyn FnMut(&str, &[u8]) -> io::Result<()>,
    max_output: u64,
    search_wasm: bool,
) -> Result<Built, BuildError> {
    if !refusals(site).is_empty() || !site.pages().keys().any(|p| p.is_home()) {
        return Err(BuildError::Refused);
    }
    cfg.validate().map_err(BuildError::Config)?;
    let by_file: BTreeMap<&str, &Page> = site.pages().values().map(|p| (&*p.file, p)).collect();
    let feed_page = match &cfg.feed {
        None => None,
        Some(f) => {
            let page = by_file.get(f.source.as_str()).ok_or_else(|| {
                BuildError::Config(format!("feed_source {} is no published page", f.source))
            })?;
            // The feed's file must not be shadowed by a page's
            // `{path}.html`, on any disk.
            let key = crate::source::fold_key(&f.url[1..]);
            if site
                .pages()
                .keys()
                .any(|p| crate::source::fold_key(p.as_str()) == key)
            {
                return Err(BuildError::Config(format!(
                    "feed_url {} is a page's URL too",
                    f.url
                )));
            }
            Some((f, *page))
        }
    };
    let mut built = Built::default();
    let mut charged = 0u64;
    let mut put = |path: &str, bytes: &[u8]| -> Result<(), BuildError> {
        // Checked before the file is written, so the output never passes
        // the budget; a file is charged at least a block.
        let len = bytes.len() as u64;
        let total = charged.saturating_add(len.max(MIN_FILE_CHARGE));
        if total > max_output {
            return Err(BuildError::OutputOverBudget { limit: max_output });
        }
        charged = total;
        built.files += 1;
        built.bytes += len;
        Ok(write(path, bytes)?)
    };
    let feed_link = feed_page.map(|(f, _)| templates::FeedLink {
        title: format!("{} releases", cfg.name),
        href: f.url.clone(),
    });
    let chrome = |title: String, description: Option<&str>, url: Option<&str>| templates::Chrome {
        lang: cfg.lang.clone(),
        csp: templates::CSP_NO_SCRIPT,
        site_name: cfg.name.clone(),
        title,
        description: description.map(str::to_owned),
        canonical: match (&cfg.base_url, url) {
            (Some(b), Some(u)) => Some(format!("{b}{}", html::url(u))),
            _ => None,
        },
        feed: feed_link.as_ref().map(|f| templates::FeedLink {
            title: f.title.clone(),
            href: f.href.clone(),
        }),
        header_links: cfg
            .header_links
            .iter()
            .map(|l| templates::NavLink {
                label: l.label.clone(),
                href: l.href.clone(),
                current: Some(l.href.as_str()) == url,
            })
            .collect(),
        footer: footer(cfg, history),
        search_wasm,
    };
    let render = |t: &dyn askama::DynTemplate| -> io::Result<Vec<u8>> {
        t.dyn_render()
            .map(String::into_bytes)
            .map_err(|e| io::Error::other(format!("a template: {e}")))
    };

    for page in site.pages().values() {
        built.pages += 1;
        let stub = page
            .redirect_to
            .as_ref()
            .filter(|_| page.front.redirect_to.is_some());
        let html = if let Some(r) = stub {
            render(&templates::RedirectTpl {
                lang: cfg.lang.clone(),
                csp: templates::CSP_NO_SCRIPT,
                title: format!("{} · {}", page.front.title, cfg.name),
                target: html::url(&r.url()),
                canonical: cfg
                    .base_url
                    .as_ref()
                    .map(|b| format!("{b}{}", html::url(&r.to.url()))),
            })?
        } else {
            page_html(site, cfg, history, page, &chrome)?
        };
        if page.path.is_home() {
            put("index.html", &html)?;
        }
        put(&format!("{}.html", page.path.as_str()), &html)?;
        put(&format!("{}.md", page.path.as_str()), &source(page)?)?;
    }

    let listed = |p: &Page| templates::Listed {
        href: templates::href(&p.path),
        title: p.front.title.clone(),
    };
    // Recent changes repeats titles: each cut at MAX_TITLE_CHARS.
    let listed_short = |p: &Page| templates::Listed {
        href: templates::href(&p.path),
        title: templates::short_title(&p.front.title),
    };
    // The one page that names a script has the one policy that admits it: the policy follows
    // the tag.
    let mut search_chrome = chrome(format!("Search · {}", cfg.name), None, Some("/_search"));
    if search_wasm {
        search_chrome.csp = templates::CSP_SEARCH_WASM;
    }
    let search = templates::SearchTpl {
        chrome: search_chrome,
        pages: site
            .pages()
            .values()
            .filter(|p| p.front.redirect_to.is_none())
            .map(listed)
            .collect(),
    };
    put("_search.html", &render(&search)?)?;
    put("_search.json", &search::index(site))?;
    let recent = templates::RecentTpl {
        chrome: chrome(
            format!("Recent changes · {}", cfg.name),
            None,
            Some("/_recent"),
        ),
        commits: history
            .recent
            .iter()
            .map(|r| {
                let pages: Vec<&Page> = r
                    .files
                    .iter()
                    .filter_map(|f| by_file.get(f.as_str()).copied())
                    .collect();
                templates::RecentRow {
                    iso: r.commit.iso(),
                    human: r.commit.date(),
                    short: r.commit.short().to_owned(),
                    summary: r.commit.summary.clone(),
                    more: r.more + pages.len().saturating_sub(RECENT_PAGES_PER_COMMIT),
                    pages: pages
                        .into_iter()
                        .take(RECENT_PAGES_PER_COMMIT)
                        .map(listed_short)
                        .collect(),
                }
            })
            .collect(),
        complete: history.complete,
    };
    put("_recent.html", &render(&recent)?)?;
    let not_found = templates::NotFoundTpl {
        chrome: chrome(format!("Not found · {}", cfg.name), None, None),
    };
    put("404.html", &render(&not_found)?)?;
    put(
        "_health",
        format!(
            "ok rubrica {} {}\n",
            env!("CARGO_PKG_VERSION"),
            history.commit.id
        )
        .as_bytes(),
    )?;
    if let Some((f, page)) = feed_page {
        put(
            &f.url[1..],
            feed::feed(
                page,
                cfg,
                f,
                history,
                &crate::render_options(site.cfg.render),
            )?
            .as_bytes(),
        )?;
    }
    for (path, bytes) in ASSETS {
        put(path, bytes)?;
    }
    #[cfg(feature = "search-wasm")]
    if search_wasm {
        for (path, bytes) in SEARCH_ASSETS {
            put(path, bytes)?;
        }
    }
    Ok(built)
}

/// The search page's bundle, as `rubrica-wasm-bundle` holds it: the loader `_search.html`
/// names, and the module it fetches beside it.
#[cfg(feature = "search-wasm")]
const SEARCH_ASSETS: &[(&str, &[u8])] = &[
    (
        "_assets/rubrica-search.js",
        rubrica_wasm_bundle::LOADER.as_bytes(),
    ),
    ("_assets/rubrica-search_bg.wasm", rubrica_wasm_bundle::WASM),
];

/// The site's own files: the stylesheet and the icon.
const ASSETS: &[(&str, &[u8])] = &[
    (
        "_assets/rubrica.css",
        include_bytes!("../../theme/rubrica.css"),
    ),
    ("_assets/icon.svg", include_bytes!("../../theme/icon.svg")),
];

/// The site's own files, path in the output and bytes, as the binary embeds
/// them: for the test that compares what a build writes with them.
#[doc(hidden)]
pub fn theme_assets() -> &'static [(&'static str, &'static [u8])] {
    ASSETS
}

/// The footer's HTML: `site.toml`'s facts when it has them (`Rubrica 0.1 ·
/// AGPL-3.0-only · source`), else what built the site.
fn footer(cfg: &BuildConfig, history: &History) -> String {
    let f = &cfg.footer;
    if f.version.is_some() || f.license.is_some() || f.source_url.is_some() {
        let mut parts = vec![html::escape(&match &f.version {
            Some(v) => format!("{} {v}", cfg.name),
            None => cfg.name.clone(),
        })];
        if let Some(l) = &f.license {
            parts.push(html::escape(l));
        }
        if let Some(s) = &f.source_url {
            parts.push(format!("<a href=\"{}\">source</a>", html::escape(s)));
        }
        parts.join(" · ")
    } else {
        html::escape(&format!(
            "Built by Rubrica {} from {}.",
            env!("CARGO_PKG_VERSION"),
            history.commit.short()
        ))
    }
}

/// The page chrome for a title, a description and a URL.
type ChromeFor<'a> = dyn Fn(String, Option<&str>, Option<&str>) -> templates::Chrome + 'a;

/// A page's HTML.
fn page_html(
    site: &Site,
    cfg: &BuildConfig,
    history: &History,
    page: &Page,
    chrome: &ChromeFor,
) -> io::Result<Vec<u8>> {
    let themed = theme::theme_body(&page.rendered.html, &page.rendered.headings);
    let nav = templates::nav_html(site, Some(&page.path));
    let crumbs = page
        .breadcrumbs
        .iter()
        .map(|c| templates::CrumbLink {
            title: templates::short_title(&c.title),
            href: c.page.as_ref().map(templates::href),
        })
        .collect();
    let file = html::url(&page.file);
    let tpl = templates::PageTpl {
        chrome: chrome(
            format!("{} · {}", page.front.title, cfg.name),
            page.front.description.as_deref(),
            Some(&page.url),
        ),
        nav: &nav,
        toc: templates::toc_html(&page.toc),
        crumbs,
        title: &page.front.title,
        title_h1: !page.rendered.headings.iter().any(|h| h.level == 1),
        description: page.front.description.as_deref(),
        updated: history.last.get(&*page.file).map(|c| templates::Updated {
            iso: c.iso(),
            human: c.date(),
        }),
        tags: &page.front.tags,
        edit: cfg.edit_url.as_ref().map(|e| format!("{e}{file}")),
        history: cfg.history_url.as_ref().map(|h| format!("{h}{file}")),
        body: themed.html,
    };
    askama::Template::render(&tpl)
        .map(String::into_bytes)
        .map_err(|e| io::Error::other(format!("a page template: {e}")))
}
