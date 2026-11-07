//! One page of the site.

use std::sync::Arc;

use rubrica_render::{FrontMatter, Rendered};

use crate::links::Target;
use crate::nav::Crumb;
use crate::source::PagePath;
use crate::toc::TocEntry;

/// A published page: its source, front matter, render, and where it sits.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// Its path (`architecture/topology`; `home`).
    pub path: PagePath,
    /// Its URL (`/architecture/topology`; `/`).
    pub url: String,
    /// Its source file (`architecture/topology.md`).
    pub file: Arc<str>,
    /// The source file's git blob id, when the caller gave one.
    pub blob_id: Option<String>,
    /// Its front matter.
    pub front: FrontMatter,
    /// Its body, rendered (shared with the [`crate::RenderCache`] that gave
    /// it). Lines in it are the body's: add [`Page::line_offset`] for the
    /// file's.
    pub rendered: Arc<Rendered>,
    /// The lines before the body (the front matter's).
    pub line_offset: usize,
    /// Its table of contents (empty for a redirect stub).
    pub toc: Vec<TocEntry>,
    /// The sections above it, from the root; shared by the pages of its
    /// directory.
    pub breadcrumbs: Arc<[Crumb]>,
    /// The index page of the section it is in, if it is in one and that
    /// has one (`architecture` for `architecture/topology`).
    pub section: Option<PagePath>,
    /// Every link of the body, in order, with where it goes: `links[i]` is
    /// `rendered.links[i]` (whose `href` it is) resolved.
    pub links: Vec<PageLink>,
    /// For a redirect stub (`redirect_to:`), where it redirects, when that
    /// is a page and not a stub.
    pub redirect_to: Option<Redirect>,
}

/// Where a redirect stub sends its readers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    /// The page.
    pub to: PagePath,
    /// `redirect_to`'s fragment as written, without `#`, if it has one.
    pub fragment: Option<String>,
}

impl Redirect {
    /// The URL to send a reader to: the page's, then `#fragment`.
    pub fn url(&self) -> String {
        match &self.fragment {
            Some(f) => format!("{}#{f}", self.to.url()),
            None => self.to.url(),
        }
    }
}

/// A link of a page, resolved: the href is the render's
/// (`Page::rendered.links`, at the same index).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageLink {
    /// Its line in the file.
    pub line: usize,
    /// Where it goes.
    pub target: Target,
}
