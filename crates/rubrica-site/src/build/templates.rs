//! The page templates (askama 0.16, `templates/`), and the trees the build
//! writes as HTML itself (the nav and the TOC, which are recursive), every
//! value escaped by [`escape`] there and by askama's HTML escaper in the
//! templates.

use askama::Template;

use super::html::{escape, url};
use super::{MAX_TITLE_CHARS, NAV_INLINE_LIMIT, NAV_MAX_BYTES};
use crate::nav::{NavEntry, NavSection, NavTree};
use crate::source::PagePath;
use crate::toc::TocEntry;
use crate::Site;

/// The Content-Security-Policy of every page but the search page's when the build writes the
/// wasm bundle: no script and no connection, from anywhere. `style-src 'unsafe-inline'`
/// stays until the diagram work decides whether it is needed.
pub(crate) const CSP_NO_SCRIPT: &str = "default-src 'self'; script-src 'none'; connect-src 'none'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; object-src 'none'; base-uri 'none'; form-action 'self'";

/// The Content-Security-Policy of the search page when it carries the wasm bundle's loader
/// (and of that page alone). `script-src 'self'` is for the module script's own URL, which
/// the keyword alone would not admit; `'wasm-unsafe-eval'` is for instantiating the module;
/// `connect-src 'self'` is for the loader's fetch of the module and the page's of the index.
pub(crate) const CSP_SEARCH_WASM: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; object-src 'none'; base-uri 'none'; form-action 'self'";

/// A header link.
pub(crate) struct NavLink {
    pub label: String,
    pub href: String,
    pub current: bool,
}

/// The feed's `<link rel="alternate">`.
pub(crate) struct FeedLink {
    pub title: String,
    pub href: String,
}

/// What `base.html` needs.
pub(crate) struct Chrome {
    pub lang: String,
    pub csp: &'static str,
    pub site_name: String,
    pub title: String,
    pub description: Option<String>,
    pub canonical: Option<String>,
    pub feed: Option<FeedLink>,
    pub header_links: Vec<NavLink>,
    /// The footer's HTML, escaped by the build.
    pub footer: String,
    /// The search page writes its form, its results and the wasm bundle's loader: only
    /// when the build writes the bundle ([`super::SEARCH_WASM`], or the `search-wasm`
    /// feature's `build_with_search_wasm`).
    pub search_wasm: bool,
}

/// A breadcrumb.
pub(crate) struct CrumbLink {
    pub title: String,
    pub href: Option<String>,
}

/// A last-updated date.
pub(crate) struct Updated {
    pub iso: String,
    pub human: String,
}

#[derive(Template)]
#[template(path = "page.html")]
pub(crate) struct PageTpl<'a> {
    pub chrome: Chrome,
    pub nav: &'a str,
    pub toc: String,
    pub crumbs: Vec<CrumbLink>,
    pub title: &'a str,
    pub title_h1: bool,
    pub description: Option<&'a str>,
    pub updated: Option<Updated>,
    pub tags: &'a [String],
    pub edit: Option<String>,
    pub history: Option<String>,
    pub body: String,
}

/// A page listed by recent changes or search.
pub(crate) struct Listed {
    pub href: String,
    pub title: String,
}

/// A commit of recent changes.
pub(crate) struct RecentRow {
    pub iso: String,
    pub human: String,
    pub short: String,
    pub summary: String,
    pub pages: Vec<Listed>,
    pub more: usize,
}

#[derive(Template)]
#[template(path = "recent.html")]
pub(crate) struct RecentTpl {
    pub chrome: Chrome,
    pub commits: Vec<RecentRow>,
    pub complete: bool,
}

#[derive(Template)]
#[template(path = "search.html")]
pub(crate) struct SearchTpl {
    pub chrome: Chrome,
    pub pages: Vec<Listed>,
}

#[derive(Template)]
#[template(path = "notfound.html")]
pub(crate) struct NotFoundTpl {
    pub chrome: Chrome,
}

#[derive(Template)]
#[template(path = "redirect.html")]
pub(crate) struct RedirectTpl {
    pub lang: String,
    pub csp: &'static str,
    pub title: String,
    pub target: String,
    pub canonical: Option<String>,
}

/// `title`, cut at [`MAX_TITLE_CHARS`] characters with `…` when longer.
/// A cut closes the bidi isolates and embeddings it left open, so they end before the `…`
/// and what follows on the line.
pub(crate) fn short_title(title: &str) -> String {
    match title.char_indices().nth(MAX_TITLE_CHARS) {
        Some((at, _)) => format!("{}{}…", &title[..at], bidi_closers(&title[..at])),
        None => title.to_owned(),
    }
}

/// What closes the bidi isolates and embeddings `s` leaves open, innermost
/// first, as UAX #9 matches them: a PDF (U+202C) ends the innermost
/// embedding or override of the current isolate, a PDI (U+2069) the
/// innermost isolate and everything opened in it; one with nothing to
/// close does nothing.
fn bidi_closers(s: &str) -> String {
    let mut open: Vec<char> = Vec::new();
    for c in s.chars() {
        match c {
            '\u{202A}' | '\u{202B}' | '\u{202D}' | '\u{202E}' => open.push('\u{202C}'),
            '\u{202C}' => {
                if open.last() == Some(&'\u{202C}') {
                    open.pop();
                }
            }
            '\u{2066}'..='\u{2068}' => open.push('\u{2069}'),
            '\u{2069}' => {
                if let Some(at) = open.iter().rposition(|&o| o == '\u{2069}') {
                    open.truncate(at);
                }
            }
            _ => {}
        }
    }
    open.into_iter().rev().collect()
}

/// A page's href: its URL, percent-encoded as an href.
pub(crate) fn href(path: &PagePath) -> String {
    url(&path.url())
}

/// The site's nav tree as HTML (`site/DESIGN.md`'s `nav-tree.html`, with
/// nested sections), `current` marked `aria-current="page"`. At most
/// [`NAV_INLINE_LIMIT`] links and [`NAV_MAX_BYTES`] bytes (checked before
/// each entry), each title cut at [`MAX_TITLE_CHARS`], then one link to
/// `/_search`, which lists every page: so each page's nav is bounded,
/// whatever the site's size.
pub(crate) fn nav_html(site: &Site, current: Option<&PagePath>) -> String {
    let tree: &NavTree = site.nav();
    let mut out = String::from("<ul class=\"tree\">");
    let mut links = 0usize;
    let link = |out: &mut String, path: &PagePath, title: &str, class: &str| {
        out.push_str("<a");
        out.push_str(class);
        out.push_str(" href=\"");
        out.push_str(&escape(&href(path)));
        out.push('"');
        if current == Some(path) {
            out.push_str(" aria-current=\"page\"");
        }
        out.push('>');
        out.push_str(&escape(&short_title(title)));
        out.push_str("</a>");
    };
    // Home first, labelled as its title says, then the root's entries.
    if let Some(home) = &tree.root.index {
        out.push_str("<li>");
        link(&mut out, home, &tree.root.title, " class=\"tree__section\"");
        out.push_str("</li>");
        links += 1;
    }
    // Pre-order over the entries, closing each section's list after it.
    enum Step<'t> {
        Entry(&'t NavEntry),
        Close,
    }
    let mut stack: Vec<Step> = tree.root.entries.iter().rev().map(Step::Entry).collect();
    let mut truncated = false;
    while let Some(step) = stack.pop() {
        match step {
            Step::Close => out.push_str("</ul></li>"),
            Step::Entry(e) => {
                if links >= NAV_INLINE_LIMIT || out.len() >= NAV_MAX_BYTES {
                    truncated = true;
                    // Close what is open, then stop.
                    let open = stack.iter().filter(|s| matches!(s, Step::Close)).count();
                    for _ in 0..open {
                        out.push_str("</ul></li>");
                    }
                    break;
                }
                match e {
                    NavEntry::Page { path, title } => {
                        out.push_str("<li>");
                        link(&mut out, path, title, "");
                        out.push_str("</li>");
                        links += 1;
                    }
                    NavEntry::Section(s) => {
                        out.push_str("<li>");
                        section_head(&mut out, s, &link, &mut links);
                        out.push_str("<ul>");
                        stack.push(Step::Close);
                        stack.extend(s.entries.iter().rev().map(Step::Entry));
                    }
                }
            }
        }
    }
    if truncated {
        out.push_str("<li><a class=\"tree__section\" href=\"/_search\">Every page</a></li>");
    }
    out.push_str("</ul>");
    out
}

fn section_head(
    out: &mut String,
    s: &NavSection,
    link: &impl Fn(&mut String, &PagePath, &str, &str),
    links: &mut usize,
) {
    match &s.index {
        Some(index) => {
            link(out, index, &s.title, " class=\"tree__section\"");
            *links += 1;
        }
        None => {
            out.push_str("<span class=\"tree__section\">");
            out.push_str(&escape(&short_title(&s.title)));
            out.push_str("</span>");
        }
    }
}

/// A page's table of contents as HTML: every heading, nested; one with no
/// id as text. A page whose headings all sit under one `h1` (the wiki's
/// pages: the title is the body's `h1`) lists what is under it.
pub(crate) fn toc_html(toc: &[TocEntry]) -> String {
    let entries = match toc {
        [only] if only.level == 1 => &only.children[..],
        all => all,
    };
    if entries.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    // Depth is at most six (heading levels), so recursion is bounded.
    fn list(out: &mut String, entries: &[TocEntry], class: &str) {
        out.push_str("<ul");
        out.push_str(class);
        out.push('>');
        for e in entries {
            out.push_str("<li>");
            match &e.id {
                Some(id) => {
                    out.push_str("<a href=\"#");
                    out.push_str(&escape(id));
                    out.push_str("\">");
                    out.push_str(&escape(&e.title));
                    out.push_str("</a>");
                }
                None => out.push_str(&escape(&e.title)),
            }
            if !e.children.is_empty() {
                list(out, &e.children, "");
            }
            out.push_str("</li>");
        }
        out.push_str("</ul>");
    }
    list(&mut out, entries, " class=\"toc\"");
    out
}
