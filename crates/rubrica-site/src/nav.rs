//! The nav tree (the directory tree of pages) and breadcrumbs.
//!
//! A directory is a section; `a.md` beside `a/` is section `a`'s index page,
//! and `home.md` the root's. In a section, pages and subsections are ordered
//! by title (case-insensitively, then exactly, then by path); the index page
//! is the section's own link, shown first. Every page is in the tree once
//! (but redirect stubs, which the site leaves out of the tree).
//!
//! A section's title is shared by its nav entry and every breadcrumb that
//! names it ([`Arc`]), so breadcrumbs are linear in the pages, whatever the
//! titles' size and the tree's depth. A directory is a [`DirPath`], a prefix
//! of a page path's own buffer: the 63 directories above a page 64 deep are
//! one string, not 63 copies of its prefixes.

use std::borrow::Borrow;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;

use crate::links::tick;
use crate::source::PagePath;

/// A directory of pages (`architecture`; `""` for the root): a prefix of
/// the path of a page under it, sharing that path's buffer, so storing it
/// costs no string. Compared, ordered and hashed as the string.
#[derive(Clone)]
pub struct DirPath {
    buf: Arc<str>,
    len: usize,
}

impl DirPath {
    /// The root directory, `""`.
    pub fn root() -> Self {
        Self {
            buf: Arc::from(""),
            len: 0,
        }
    }

    /// The directory `page` is in, sharing its path's buffer.
    pub fn of(page: &PagePath) -> Self {
        Self {
            buf: page.shared().clone(),
            len: page.dir().len(),
        }
    }

    /// The directory as a string.
    pub fn as_str(&self) -> &str {
        &self.buf[..self.len]
    }

    /// The first `len` bytes of `buf`, which end a directory (at a `/`).
    pub(crate) fn prefix(buf: &Arc<str>, len: usize) -> Self {
        debug_assert_eq!(buf.as_bytes().get(len), Some(&b'/'));
        Self {
            buf: buf.clone(),
            len,
        }
    }

    /// The directory above this one (the root's is the root); the same
    /// buffer.
    pub fn parent(&self) -> Self {
        Self {
            buf: self.buf.clone(),
            len: self.as_str().rfind('/').unwrap_or(0),
        }
    }

    /// Whether this is the root.
    pub fn is_root(&self) -> bool {
        self.len == 0
    }

    /// The buffer it is a prefix of: for the identity tests.
    #[doc(hidden)]
    pub fn buffer(&self) -> &Arc<str> {
        &self.buf
    }
}

impl Deref for DirPath {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for DirPath {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq for DirPath {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for DirPath {}

impl PartialEq<str> for DirPath {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for DirPath {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<DirPath> for str {
    fn eq(&self, other: &DirPath) -> bool {
        self == other.as_str()
    }
}

impl PartialEq<DirPath> for &str {
    fn eq(&self, other: &DirPath) -> bool {
        *self == other.as_str()
    }
}

impl PartialOrd for DirPath {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DirPath {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl Hash for DirPath {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.as_str().hash(h)
    }
}

impl fmt::Debug for DirPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for DirPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The site's nav tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavTree {
    /// The root section (`dir` `""`, index `home`).
    pub root: NavSection,
}

/// A directory of pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavSection {
    /// The directory (`architecture`; `""` for the root).
    pub dir: DirPath,
    /// Its index page's title, else the directory's name (`"Home"` at the
    /// root without a home page).
    pub title: Arc<str>,
    /// The index page: `<dir>.md`, or `home.md` at the root.
    pub index: Option<PagePath>,
    /// Its pages and subsections, ordered by title.
    pub entries: Vec<NavEntry>,
}

/// An entry of a section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavEntry {
    /// A page that is no section's index.
    Page {
        /// The page.
        path: PagePath,
        /// Its title.
        title: String,
    },
    /// A subdirectory.
    Section(NavSection),
}

impl NavEntry {
    fn title(&self) -> &str {
        match self {
            Self::Page { title, .. } => title,
            Self::Section(s) => &s.title,
        }
    }
    fn key(&self) -> &str {
        match self {
            Self::Page { path, .. } => path.as_str(),
            Self::Section(s) => &s.dir,
        }
    }
}

impl NavTree {
    /// Every page in the tree, in order: each section's index, then its
    /// entries. Iterative.
    pub fn pages(&self) -> Vec<&PagePath> {
        let mut out = Vec::new();
        let mut stack: Vec<&NavSection> = vec![&self.root];
        while let Some(s) = stack.pop() {
            out.extend(s.index.iter());
            // Entries in order: pages now, sections when popped (pushed in
            // reverse so the first pops first).
            let mut subs = Vec::new();
            for e in &s.entries {
                match e {
                    NavEntry::Page { path, .. } => out.push(path),
                    NavEntry::Section(sub) => subs.push(sub),
                }
            }
            stack.extend(subs.into_iter().rev());
        }
        out
    }

    /// The number of entries: sections (the root included) and pages.
    pub fn len(&self) -> usize {
        let mut n = 0;
        let mut stack: Vec<&NavSection> = vec![&self.root];
        while let Some(s) = stack.pop() {
            n += 1 + usize::from(s.index.is_some());
            for e in &s.entries {
                match e {
                    NavEntry::Page { .. } => n += 1,
                    NavEntry::Section(sub) => stack.push(sub),
                }
            }
        }
        n
    }

    /// Whether the tree has no page.
    pub fn is_empty(&self) -> bool {
        self.root.index.is_none() && self.root.entries.is_empty()
    }
}

/// A directory's title and index page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Dir {
    pub title: Arc<str>,
    pub index: Option<PagePath>,
}

/// Every directory that holds a page (at any depth), the root included,
/// keyed by its [`DirPath`] (shared by its nav section and breadcrumbs).
pub(crate) fn dirs<'a>(
    pages: impl Iterator<Item = (&'a PagePath, &'a str)> + Clone,
) -> BTreeMap<DirPath, Dir> {
    let titles: BTreeMap<&str, (&PagePath, &str)> =
        pages.clone().map(|(p, t)| (p.as_str(), (p, t))).collect();
    let mut out: BTreeMap<DirPath, Dir> = BTreeMap::new();
    let mut add = |key: DirPath| {
        let dir = key.as_str();
        if out.contains_key(dir) {
            return false;
        }
        // `home.md` is the root's index, so a `home/` directory has none.
        let index_path = match dir {
            "" => Some(PagePath::HOME),
            PagePath::HOME => None,
            d => Some(d),
        };
        // The index is the site's own path, shared, not a copy.
        let found = index_path.and_then(|p| titles.get(p).copied());
        let index = found.map(|(p, _)| p.clone());
        let title: Arc<str> = match found.map(|(_, t)| t) {
            Some(t) => Arc::from(t),
            None if dir.is_empty() => Arc::from("Home"),
            None => Arc::from(dir.rsplit('/').next().unwrap_or(dir)),
        };
        tick(1);
        out.insert(key, Dir { title, index });
        true
    };
    add(DirPath::root());
    for (p, _) in pages {
        // Ancestors, deepest first, each a prefix of the page's own path;
        // stop at the first one already known (its own ancestors are then
        // known too), so this is linear in the path bytes.
        let mut d = DirPath::of(p);
        while !d.is_root() && add(d.clone()) {
            d = d.parent();
        }
    }
    out
}

fn sort_entries(entries: &mut [NavEntry]) {
    entries.sort_by(|a, b| {
        (a.title().to_lowercase(), a.title(), a.key()).cmp(&(
            b.title().to_lowercase(),
            b.title(),
            b.key(),
        ))
    });
}

/// The nav tree of `pages` (path, title), given their [`dirs`].
pub(crate) fn nav_tree<'a>(
    pages: impl Iterator<Item = (&'a PagePath, &'a str)>,
    dirs: &BTreeMap<DirPath, Dir>,
) -> NavTree {
    let mut entries: BTreeMap<&str, Vec<NavEntry>> =
        dirs.keys().map(|d| (d.as_str(), Vec::new())).collect();
    for (p, title) in pages {
        tick(1);
        // A section's index is the section's own link, not an entry.
        let is_index = p.is_home()
            || dirs
                .get(p.as_str())
                .is_some_and(|d| d.index.as_ref() == Some(p));
        if !is_index {
            entries
                .get_mut(p.dir())
                .expect("its directory")
                .push(NavEntry::Page {
                    path: p.clone(),
                    title: title.to_owned(),
                });
        }
    }
    // Deepest directories first, so each is finished before its parent,
    // which takes it as an entry; no recursion, and no map but `entries`.
    let mut order: Vec<&DirPath> = dirs.keys().collect();
    order.sort_by_key(|d| {
        std::cmp::Reverse(if d.is_empty() {
            0
        } else {
            d.matches('/').count() + 1
        })
    });
    for d in order {
        let mut es = entries.remove(d.as_str()).unwrap_or_default();
        sort_entries(&mut es);
        // A chain of directories has one entry each: a vector grown by
        // pushes would hold four.
        es.shrink_to_fit();
        let info = &dirs[d];
        let section = NavSection {
            dir: d.clone(),
            title: info.title.clone(),
            index: info.index.clone(),
            entries: es,
        };
        if d.is_root() {
            return NavTree { root: section };
        }
        entries
            .get_mut(parent(d))
            .expect("a directory's parent is a directory")
            .push(NavEntry::Section(section));
    }
    unreachable!("the root is a directory")
}

fn parent(dir: &str) -> &str {
    dir.rsplit_once('/').map_or("", |(a, _)| a)
}

/// One step of a page's breadcrumbs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crumb {
    /// The directory (`""` for the root), shared with its nav section and
    /// every page under it.
    pub dir: DirPath,
    /// Its title, shared with the section's nav entry.
    pub title: Arc<str>,
    /// Its index page, the crumb's link, if it has one.
    pub page: Option<PagePath>,
}

/// The breadcrumbs of the pages in directory `dir`: the root, then each
/// directory above them, from the top; not the page itself. (The home page
/// has none: the site gives it none.) Every page of a directory shares one.
pub(crate) fn breadcrumbs(dir: &str, dirs: &BTreeMap<DirPath, Dir>) -> Arc<[Crumb]> {
    // A directory that holds only redirect stubs is in no tree: no crumb.
    let crumb = |d: &str| {
        tick(1);
        let (key, info) = dirs.get_key_value(d)?;
        Some(Crumb {
            dir: key.clone(),
            title: info.title.clone(),
            page: info.index.clone(),
        })
    };
    let mut out: Vec<Crumb> = crumb("").into_iter().collect();
    if !dir.is_empty() {
        let mut end = 0;
        for seg in dir.split('/') {
            end += seg.len();
            out.extend(crumb(&dir[..end]));
            end += 1;
        }
    }
    out.into()
}
