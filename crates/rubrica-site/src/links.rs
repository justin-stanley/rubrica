//! Link targets: where a link written in a page goes, read as a browser
//! reads the href the writer writes for it, and the heading-id suggestion of
//! check-anchors.cjs for a fragment that names no heading.
//!
//! Hrefs come from untrusted Markdown. Resolution never touches a file
//! system and never yields a path with an empty, `.` or `..` segment: a
//! [`Target::Page`] is always a page of the site, and every other path it
//! reports is cleaned the same way, at the corpus root at most. The work
//! per link is linear in the href (and in the cited fragment), but a
//! relative link's, which is also linear in its page's path (the base it is
//! resolved against: at most [`MAX_PATH_BYTES`]); the suggestion index is
//! linear in the target page's headings, and only one is alive at a time
//! (`LooseIndex`); the one search that is not linear (check-anchors'
//! word-overlap fallback) runs under a budget that counts every posting it
//! visits. A target's headings are shared ([`Arc`]) by every finding that
//! names one, never copied per link.

use std::cell::{Cell, OnceCell};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use unicode_normalization::UnicodeNormalization;

use crate::loose_tables::{LETTER_OR_NUMBER, MARK};
use crate::source::{
    check_segment, BadPath, PagePath, Reserved, MAX_PATH_BYTES, MAX_PATH_SEGMENTS,
};

thread_local! {
    static OPS: Cell<u64> = const { Cell::new(0) };
    /// [`LooseIndex`]es alive on this thread, and the most at once.
    static LOOSE_LIVE: Cell<u64> = const { Cell::new(0) };
    static LOOSE_PEAK: Cell<u64> = const { Cell::new(0) };
}

/// Counts `n` units of link work (bytes of an href walked, trie steps,
/// postings visited, headings indexed) on this thread, for the
/// operation-count tests.
pub(crate) fn tick(n: u64) {
    OPS.with(|c| c.set(c.get().wrapping_add(n)));
}

/// The link work done on this thread since the last [`ops_reset`]. For the
/// operation-count tests.
#[doc(hidden)]
pub fn ops() -> u64 {
    OPS.with(|c| c.get())
}

/// Resets [`ops`] and [`loose_indexes_peak`].
#[doc(hidden)]
pub fn ops_reset() {
    OPS.with(|c| c.set(0));
    LOOSE_PEAK.with(|p| p.set(LOOSE_LIVE.with(Cell::get)));
}

/// The most suggestion indexes ([`LooseIndex`]) alive at once on this
/// thread since the last [`ops_reset`]. For the peak-memory test.
#[doc(hidden)]
pub fn loose_indexes_peak() -> u64 {
    LOOSE_PEAK.with(Cell::get)
}

/// Where a link goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A page of the site, and the fragment (percent-decoded), if any.
    Page {
        /// The page.
        path: PagePath,
        /// The fragment, without `#`.
        fragment: Option<String>,
    },
    /// `#fragment` on the linking page (`""` for a bare `#`).
    SamePage {
        /// The fragment, percent-decoded.
        fragment: String,
    },
    /// An internal path that names no page of the site.
    NoPage {
        /// The path as cleaned (`/`-joined segments, decoded); for
        /// [`NoPage::Invalid`], the href's path as a browser writes it
        /// ([`written_href`]: percent-encoded, so ASCII with no control
        /// character), since it cannot be cleaned.
        path: String,
        /// Why.
        why: NoPage,
    },
    /// Wiki.js's own routes: `/<letter>/…` or `/<letter>` (`/t/<tag>` is a
    /// tag page, `/a/…` the admin area). Not pages, and not checked.
    System {
        /// The href's path, as written.
        path: String,
    },
    /// A path whose last segment has a `.`, naming no page: a file
    /// (`/assets/x.png`, `/a/b.md`). Wiki.js classes such links
    /// `is-asset-link` and never checks them; the site checks them against
    /// its files.
    Asset {
        /// The cleaned path, without its leading `/`.
        path: String,
        /// Whether the site has the file: an asset, or a page's source
        /// (`<page>.md`, which the build writes beside the page).
        exists: bool,
    },
    /// A URL with a scheme, or protocol-relative (`//host/…`), that is not
    /// the wiki's own origin.
    External,
    /// A relative path, which Wiki.js resolves against the page as if it
    /// were a directory, and which `check` refuses when it reaches a `.md`
    /// file of the repository.
    Relative {
        /// The published page it names as a browser resolves it against the
        /// page's URL (`.md` dropped; a browser stays at the root however
        /// many `..` climb), if it names one.
        page: Option<PagePath>,
        /// The repository's `.md` file that path is (`<path>.md`, or the
        /// path itself when it ends in `.md`), page or not (a draft,
        /// `README.md`, a file under `scripts/`, a reserved path), if the
        /// repository has it. Shared by every link to it.
        file: Option<Arc<str>>,
    },
}

/// Why an internal path names no page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoPage {
    /// No source file is that page.
    Missing,
    /// The page has `published: false`: Rubrica does not publish it.
    Unpublished,
    /// The page's front matter is invalid, so it is not rendered.
    BadFrontMatter,
    /// The page's file is not UTF-8, so it is not rendered.
    NotUtf8,
    /// The path is reserved ([`crate::Finding::ReservedPagePath`]): its file
    /// is no page. Shared by every link to it.
    Reserved(Arc<Reserved>),
    /// The path can be no page's (`/a%00`, `/a%2F..%2F..`, a locale-only
    /// `/en/`, an invalid percent-encoding): [`BadPath`] says which.
    Invalid(BadPath),
}

/// An internal path, cleaned: `Ok(segments)` (empty for the home page), or
/// why it is no page path. `path` is the href's path as written (with its
/// leading `/`, without query or fragment).
///
/// Two passes, as the request travels:
/// 1. the browser's: the href the writer writes ([`written_href`]) has its dot
///    segments removed by the WHATWG URL parser (`.`, `..`, and their
///    `%2e` spellings, in any case; empty segments kept, so `/a//..` is
///    `/a/`);
/// 2. the server's (as Caddy's `try_files` does, and Wiki.js's `parsePath` does too): each segment
///    is percent-decoded (strict UTF-8, or the path is invalid) and the
///    result cleaned again (empty and `.` segments dropped, `..` popping),
///    so a decoded `%2F..%2F` cannot climb either.
///
/// Neither pass climbs above the root. Then a leading `en` segment followed
/// by another is Wiki.js's locale prefix and is dropped, and `home` is the
/// home page.
pub(crate) fn clean_internal(path: &str) -> Result<Vec<String>, BadPath> {
    let written = written_href(path);
    tick(written.len() as u64 + 1);
    let (first, _) = whatwg_path(Vec::new(), written.strip_prefix('/').unwrap_or(&written));
    let (mut out, _) = server_clean(first).ok_or(BadPath::NotUtf8)?;
    if out.first().is_some_and(|s| s == "en") {
        if out.len() == 1 {
            return Err(BadPath::EmptySegment);
        }
        out.remove(0);
    }
    if out.len() > MAX_PATH_SEGMENTS {
        return Err(BadPath::TooDeep);
    }
    if out.iter().map(|s| s.len() + 1).sum::<usize>() > MAX_PATH_BYTES + 1 {
        return Err(BadPath::TooLong);
    }
    for seg in &out {
        check_segment(seg)?;
    }
    Ok(out)
}

thread_local! {
    static BASES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How many relative-link bases (a page's directory, split) were built on
/// this thread since [`relative_bases_reset`]: one per page whose links are
/// resolved, not one per link.
#[doc(hidden)]
pub fn relative_bases() -> u64 {
    BASES.with(|b| b.get())
}

/// Reset [`relative_bases`]'s count.
#[doc(hidden)]
pub fn relative_bases_reset() {
    BASES.with(|b| b.set(0));
}

/// A page's directory, split once, for all of its relative links (not
/// split again for each link).
pub(crate) struct RelativeBase<'a> {
    /// The page.
    pub page: &'a PagePath,
    /// Its directory's segments (none at the root).
    dir: Vec<&'a str>,
}

impl<'a> RelativeBase<'a> {
    /// `page`'s base. Counted ([`relative_bases`]).
    pub fn of(page: &'a PagePath) -> Self {
        BASES.with(|b| b.set(b.get() + 1));
        tick(page.as_str().len() as u64 + 1);
        Self {
            page,
            dir: page.dir().split('/').filter(|s| !s.is_empty()).collect(),
        }
    }
}

/// A relative href's path resolved against a page, as a browser resolves it
/// against the page's URL (`/a/b`'s base is `/a/`), then cleaned as
/// [`clean_internal`] cleans: the segments (empty for the root), neither
/// pass climbing above the root (a browser stays there, so `../../x` on
/// `/a/b` is `/x`); `None` if a percent-encoding is not UTF-8. The segments
/// need not make a page path. Its work is the href's length and the page's
/// path's: the base is split once per page ([`RelativeBase`]), but the
/// result still holds the page's directory, which is copied (and joined to
/// look the page up) per link.
pub(crate) fn resolve_relative(from: &RelativeBase, rel_path: &str) -> Option<Vec<String>> {
    tick(from.page.as_str().len() as u64 + 1);
    if rel_path.is_empty() {
        return Some(from.page.segments().map(str::to_owned).collect());
    }
    let written = written_href(rel_path);
    tick(written.len() as u64 + 1);
    let base = from.dir.iter().copied().map(Seg::Base).collect();
    let (first, _) = whatwg_path(base, &written);
    let (out, _) = server_clean(first)?;
    Some(out)
}

/// A path segment: of the base (a page path's, decoded already, never a dot
/// segment) or of the href (as written).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seg<'a> {
    Base(&'a str),
    Raw(&'a str),
}

/// The WHATWG URL parser's path state over `rest` (an href's path after its
/// leading `/`, or a relative one), from `base` (the base URL's path less
/// its last segment): a double-dot segment pops, a single-dot one is
/// skipped, and either adds an empty segment when it is the last; every
/// other segment, empty ones included, is kept. Also whether a pop found
/// nothing (the browser stays at the root).
fn whatwg_path<'a>(mut out: Vec<Seg<'a>>, rest: &'a str) -> (Vec<Seg<'a>>, bool) {
    let mut climbed = false;
    let parts: Vec<&str> = rest.split('/').collect();
    let n = parts.len();
    for (i, seg) in parts.into_iter().enumerate() {
        let last = i + 1 == n;
        match dot_kind(seg) {
            Dot::Double => {
                climbed |= out.pop().is_none();
                if last {
                    out.push(Seg::Raw(""));
                }
            }
            Dot::Single if last => out.push(Seg::Raw("")),
            Dot::Single => {}
            Dot::None => out.push(Seg::Raw(seg)),
        }
    }
    (out, climbed)
}

/// The server's pass: each segment percent-decoded (`None` if not UTF-8),
/// split again at a decoded `/`, and cleaned (empty and `.` dropped, `..`
/// popping, never above the root); also whether a pop found nothing.
fn server_clean(segs: Vec<Seg>) -> Option<(Vec<String>, bool)> {
    let mut out: Vec<String> = Vec::new();
    let mut climbed = false;
    for seg in segs {
        let decoded = match seg {
            Seg::Base(s) => {
                out.push(s.to_owned());
                continue;
            }
            Seg::Raw(s) => percent_decode_strict(s)?,
        };
        for part in decoded.split('/') {
            match part {
                "" | "." => {}
                ".." => climbed |= out.pop().is_none(),
                _ => out.push(part.to_owned()),
            }
        }
    }
    Some((out, climbed))
}

enum Dot {
    None,
    Single,
    Double,
}

/// WHATWG's single- and double-dot path segments.
fn dot_kind(seg: &str) -> Dot {
    let lower = seg.to_ascii_lowercase();
    match lower.as_str() {
        "." | "%2e" => Dot::Single,
        ".." | ".%2e" | "%2e." | "%2e%2e" => Dot::Double,
        _ => Dot::None,
    }
}

/// The href as a browser sees it in Rubrica's HTML, after the HTML attribute's entities
/// are read back: the destination as `rubrica_render::writer::escape_href` writes it
/// (the writer's own rule, so the two cannot drift).
#[doc(hidden)]
pub fn written_href(href: &str) -> String {
    rubrica_render::writer::escape_href(href)
}

/// The path of [`written_href`]`(href)` as the WHATWG URL parser leaves it
/// ([`whatwg_path`]), resolved against `http://x/<base>`: for the
/// differential with node's `URL`. An `href` starting with `/` is absolute.
#[doc(hidden)]
pub fn browser_pathname(base: &str, href: &str) -> String {
    let written = written_href(href);
    if written.is_empty() {
        return format!("/{base}");
    }
    let (from, rest) = match written.strip_prefix('/') {
        Some(rest) => (Vec::new(), rest),
        None => {
            let mut b: Vec<Seg> = base.split('/').map(Seg::Raw).collect();
            b.pop();
            (b, written.as_str())
        }
    };
    let (segs, _) = whatwg_path(from, rest);
    let parts: Vec<&str> = segs
        .iter()
        .map(|s| match s {
            Seg::Base(s) | Seg::Raw(s) => *s,
        })
        .collect();
    format!("/{}", parts.join("/"))
}

/// `%XX` decoded; `None` if the bytes are not UTF-8.
fn percent_decode_strict(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |at: usize| b.get(at).and_then(|&c| (c as char).to_digit(16));
        match (b[i], hex(i + 1), hex(i + 2)) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, _, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// Whether the href's path (as written, with its leading `/`) is one of
/// Wiki.js's system routes: html-core's `^\/[a-z]\/` or `^\/[a-z]$`, any
/// case. Tested on the raw href, before any decoding or dot-segment
/// removal, as html-core tests it (so `/%74/x` and `/./t/x` are not system
/// links, there or here).
pub(crate) fn is_system_path(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() >= 2 && b[1].is_ascii_alphabetic() && (b.len() == 2 || b[2] == b'/')
}

fn in_ranges(table: &[(u32, u32)], c: char) -> bool {
    let c = c as u32;
    table
        .binary_search_by(|&(lo, hi)| {
            if hi < c {
                std::cmp::Ordering::Less
            } else if lo > c {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// check-anchors.cjs's `loose()` (the Wiki.js-era
/// checker's): lowercase, NFC, marks and zero-widths
/// (U+200B..U+200F, U+FEFF) dropped, an `h-` before an ASCII digit dropped
/// at the start, every run of characters that are neither letters nor
/// numbers one `-`, and `-` trimmed from both ends. JavaScript's `\p{L}`,
/// `\p{N}` and `\p{M}` are tables generated from the node that runs the
/// script (`tools/gen-loose-tables.js`); the lowercasing is Rust's, the
/// normalisation `unicode-normalization`'s, both checked against node by
/// `fixtures/loose-differential.json`.
pub fn loose(s: &str) -> String {
    let lower = s.to_lowercase();
    let kept: String = lower
        .nfc()
        .filter(|&c| !in_ranges(MARK, c) && !matches!(c, '\u{200B}'..='\u{200F}' | '\u{FEFF}'))
        .collect();
    let rest = match kept.strip_prefix("h-") {
        Some(r) if r.as_bytes().first().is_some_and(u8::is_ascii_digit) => r,
        _ => &kept,
    };
    let mut out = String::with_capacity(rest.len());
    let mut run = false;
    for c in rest.chars() {
        if in_ranges(LETTER_OR_NUMBER, c) {
            out.push(c);
            run = false;
        } else if !run {
            out.push('-');
            run = true;
        }
    }
    out.trim_matches('-').to_owned()
}

/// check-anchors.cjs's `renderFragment()`: the id as it should be written
/// after `#`: as it is when it is printable ASCII, else `encodeURI` of it
/// with `#` as `%23`.
pub fn render_fragment(id: &str) -> String {
    if id.bytes().all(|b| (0x20..=0x7e).contains(&b)) {
        return id.to_owned();
    }
    let mut out = String::with_capacity(id.len() * 3);
    let mut buf = [0u8; 4];
    for c in id.chars() {
        let keep = c.is_ascii_alphanumeric() || ";,/?:@&=+$-_.!~*'()".contains(c);
        if keep {
            out.push(c);
        } else {
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

/// One heading of a target page, as the anchor check needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeadingRef {
    pub id: Arc<str>,
    pub dropped: bool,
    pub text: Arc<str>,
    /// The heading's line in the file.
    pub line: usize,
    pub level: u8,
}

/// A target page's ids, and the index check-anchors' `suggest()` searches.
#[derive(Debug)]
pub(crate) struct AnchorIndex {
    /// Every heading, in document order, dropped ones included.
    pub headings: Vec<HeadingRef>,
    /// Every id a fragment can reach: live heading ids and `@sec` anchors.
    pub ids: HashSet<Arc<str>>,
    /// A dropped heading's slug to its index (the first, as check-anchors'
    /// `find` takes it).
    pub dropped: HashMap<Arc<str>, usize>,
    /// The headings by (line, index), for line citations.
    by_line: OnceCell<Vec<(usize, usize)>>,
}

/// check-anchors' four searches over one target page's live headings'
/// `loose(id)`s: a byte trie for the first three, an inverted index of words
/// for the fourth. Its size is linear in those ids (`loose_index_bytes`);
/// the site builds one per target page that needs a suggestion and drops it
/// before building the next, so at most one is alive at a time
/// ([`loose_indexes_peak`]).
#[derive(Debug)]
pub(crate) struct LooseIndex {
    /// Trie nodes: (first heading through it, first heading ending at it).
    nodes: Vec<(u32, u32)>,
    edges: HashMap<(u32, u8), u32>,
    /// Word to (heading, occurrences), headings in order.
    words: HashMap<String, Vec<(u32, u32)>>,
}

const NONE: u32 = u32::MAX;

impl Drop for LooseIndex {
    fn drop(&mut self) {
        LOOSE_LIVE.with(|l| l.set(l.get() - 1));
    }
}

/// The bytes a [`LooseIndex`] of headings with these ids holds (its vectors'
/// and maps' capacities, entries and their strings), for the held-size test.
#[doc(hidden)]
pub fn loose_index_bytes(ids: &[&str]) -> usize {
    use std::mem::size_of;
    let headings: Vec<HeadingRef> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| HeadingRef {
            id: Arc::from(*id),
            dropped: false,
            text: Arc::from(""),
            line: i + 1,
            level: 2,
        })
        .collect();
    let ix = LooseIndex::build(&headings);
    // A hashbrown table holds one control byte and one slot per bucket.
    let table = |cap: usize, slot: usize| (cap + 1).next_power_of_two() * (slot + 1);
    ix.nodes.capacity() * size_of::<(u32, u32)>()
        + table(ix.edges.capacity(), size_of::<((u32, u8), u32)>())
        + table(ix.words.capacity(), size_of::<(String, Vec<(u32, u32)>)>())
        + ix.words
            .iter()
            .map(|(w, v)| w.capacity() + v.capacity() * size_of::<(u32, u32)>())
            .sum::<usize>()
}

impl LooseIndex {
    /// The index of `headings` (a target page's, in document order).
    pub(crate) fn build(headings: &[HeadingRef]) -> Self {
        LOOSE_LIVE.with(|l| {
            l.set(l.get() + 1);
            LOOSE_PEAK.with(|p| p.set(p.get().max(l.get())));
        });
        let mut ix = LooseIndex {
            nodes: vec![(NONE, NONE)],
            edges: HashMap::new(),
            words: HashMap::new(),
        };
        for (i, h) in headings.iter().enumerate() {
            if h.dropped {
                continue;
            }
            let i = i as u32;
            let l = loose(&h.id);
            tick(l.len() as u64 + 1);
            let mut node = 0u32;
            if ix.nodes[0].0 == NONE {
                ix.nodes[0].0 = i;
            }
            for &b in l.as_bytes() {
                let next = ix.nodes.len() as u32;
                let n = *ix.edges.entry((node, b)).or_insert(next);
                if n == next {
                    ix.nodes.push((i, NONE));
                }
                node = n;
            }
            if ix.nodes[node as usize].1 == NONE {
                ix.nodes[node as usize].1 = i;
            }
            let mut counts: HashMap<&str, u32> = HashMap::new();
            for w in l.split('-').filter(|w| !w.is_empty()) {
                *counts.entry(w).or_default() += 1;
            }
            for (w, n) in counts {
                ix.words.entry(w.to_owned()).or_default().push((i, n));
            }
        }
        ix
    }

    fn walk(&self, from: u32, bytes: &[u8]) -> Option<u32> {
        let mut node = from;
        for &b in bytes {
            tick(1);
            node = *self.edges.get(&(node, b))?;
        }
        Some(node)
    }
}

impl AnchorIndex {
    pub fn new(headings: Vec<HeadingRef>, sec_anchors: impl IntoIterator<Item = String>) -> Self {
        let mut ids = HashSet::new();
        let mut dropped = HashMap::new();
        for (i, h) in headings.iter().enumerate() {
            if h.dropped {
                dropped.entry(h.id.clone()).or_insert(i);
            } else {
                ids.insert(h.id.clone());
            }
        }
        ids.extend(sec_anchors.into_iter().map(Arc::from));
        tick(headings.len() as u64);
        Self {
            headings,
            ids,
            dropped,
            by_line: OnceCell::new(),
        }
    }

    /// Whether `fragment` (decoded) reaches an element: a live id, or the
    /// page's top (`""`, or `top` in any case when no element has that id,
    /// as HTML's "scroll to the fragment" does).
    pub fn reaches(&self, fragment: &str) -> bool {
        fragment.is_empty() || self.ids.contains(fragment) || fragment.eq_ignore_ascii_case("top")
    }

    /// The heading a line citation `#L<n>` falls under: the last heading on
    /// or before line `n` (check-anchors' loop), and whether it is the
    /// page's first heading and an `h1` (then the fragment should go).
    pub fn enclosing(&self, n: u64) -> Option<(&HeadingRef, bool)> {
        let by_line = self.by_line.get_or_init(|| {
            let mut v: Vec<(usize, usize)> = self
                .headings
                .iter()
                .enumerate()
                .map(|(i, h)| (h.line, i))
                .collect();
            v.sort_unstable();
            v
        });
        let k = by_line.partition_point(|&(line, _)| (line as u64) <= n);
        let &(_, i) = by_line.get(k.checked_sub(1)?)?;
        let first = by_line.first().map(|&(_, i)| i);
        let h = &self.headings[i];
        Some((h, Some(i) == first && h.level == 1))
    }
}

impl LooseIndex {
    /// check-anchors.cjs's `suggest()` (lines 314-333) over the headings
    /// this index was built from: the first live heading whose `loose(id)`
    /// is `loose(want)`; else the first that starts with it and a `-`; else
    /// the first that, with a `-`, starts it; else the one sharing the most
    /// words (counted with repeats, over the distinct words wanted), if they
    /// are at least 60% of the words wanted, the first of equals. `budget`
    /// is spent one unit per posting the last search visits; when it would
    /// run out, that search is skipped and the second value is true.
    pub fn suggest(&self, want: &str, budget: &mut u64) -> (Option<usize>, bool) {
        let want = loose(want);
        if want.is_empty() {
            return (None, false);
        }
        let ix = self;
        let w = want.as_bytes();
        if let Some(node) = ix.walk(0, w) {
            let (_, ends) = ix.nodes[node as usize];
            if ends != NONE {
                return (Some(ends as usize), false);
            }
            if let Some(dash) = ix.walk(node, b"-") {
                return (Some(ix.nodes[dash as usize].0 as usize), false);
            }
        }
        // The headings whose loose id, then `-`, starts `want`: the trie
        // nodes `want` passes through just before one of its `-`s.
        let mut best = NONE;
        let mut node = 0u32;
        for (k, &b) in w.iter().enumerate() {
            if b == b'-' && k > 0 {
                let ends = ix.nodes[node as usize].1;
                best = best.min(ends);
            }
            tick(1);
            match ix.edges.get(&(node, b)) {
                Some(&n) => node = n,
                None => break,
            }
        }
        if best != NONE {
            return (Some(best as usize), false);
        }
        let mut wanted: Vec<&str> = want.split('-').filter(|s| !s.is_empty()).collect();
        wanted.sort_unstable();
        wanted.dedup();
        let mut score: HashMap<u32, u64> = HashMap::new();
        for word in &wanted {
            let Some(postings) = ix.words.get(*word) else {
                continue;
            };
            let cost = postings.len() as u64;
            if cost > *budget {
                return (None, true);
            }
            *budget -= cost;
            tick(cost);
            for &(h, n) in postings {
                *score.entry(h).or_default() += u64::from(n);
            }
        }
        let size = wanted.len().max(1) as u64;
        let (h, s) = score.into_iter().fold((NONE, 0u64), |(bh, bs), (h, s)| {
            if s > bs || (s == bs && h < bh) {
                (h, s)
            } else {
                (bh, bs)
            }
        });
        (
            (h != NONE && s * 5 >= size * 3).then_some(h as usize),
            false,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_loose_tables_are_unicode_17() {
        assert_eq!(crate::loose_tables::UNICODE_VERSION, "17.0");
    }

    #[test]
    fn loose_collapses_what_separates_a_hand_written_slug_from_a_uslug_id() {
        assert_eq!(loose("capacity--memory"), "capacity-memory");
        assert_eq!(loose("h-4-nas-config"), "4-nas-config");
        assert_eq!(loose("h-x"), "h-x");
        assert_eq!(loose("\u{FE0F}-open-item"), "open-item");
        assert_eq!(loose("Ünïcodé\u{200B}Ω"), "ünïcodéω");
        assert_eq!(loose("--a__b--"), "a-b");
        assert_eq!(loose(""), "");
    }

    #[test]
    fn render_fragment_encodes_only_non_ascii() {
        assert_eq!(render_fragment("h-4-a"), "h-4-a");
        assert_eq!(render_fragment("\u{FE0F}-open#x"), "%EF%B8%8F-open%23x");
    }

    #[test]
    fn the_writer_escapes_hrefs_as_escape_href_does() {
        assert_eq!(
            written_href("/a b\\c%zz%41\0"),
            "/a%20b%5Cc%25zz%41%EF%BF%BD"
        );
        assert_eq!(written_href("/é"), "/%C3%A9");
        assert_eq!(written_href("/a&b'c"), "/a&b'c");
    }

    #[test]
    fn dot_segments_never_climb_above_the_root() {
        assert_eq!(clean_internal("/a/../../../b"), Ok(vec!["b".to_owned()]));
        assert_eq!(clean_internal("/a/%2E%2e/b"), Ok(vec!["b".to_owned()]));
        assert_eq!(
            clean_internal("/a/%2F..%2F..%2Fb"),
            Ok(vec!["b".to_owned()])
        );
        assert_eq!(
            clean_internal("/a//b/"),
            Ok(vec!["a".to_owned(), "b".to_owned()])
        );
        assert_eq!(clean_internal("/"), Ok(vec![]));
        assert_eq!(clean_internal("/en/a"), Ok(vec!["a".to_owned()]));
        assert_eq!(clean_internal("/en/"), Err(BadPath::EmptySegment));
        assert_eq!(clean_internal("/a%FF"), Err(BadPath::NotUtf8));
        assert_eq!(clean_internal("/a%5Cb"), Err(BadPath::Character('\\')));
        assert_eq!(clean_internal("/a\0"), Ok(vec!["a\u{FFFD}".to_owned()]));
    }

    #[test]
    fn relative_paths_resolve_against_the_pages_url() {
        let p = PagePath::new("a/b").unwrap();
        assert_eq!(
            resolve_relative(&RelativeBase::of(&p), "c"),
            Some(vec!["a".to_owned(), "c".to_owned()])
        );
        assert_eq!(
            resolve_relative(&RelativeBase::of(&p), "../c"),
            Some(vec!["c".to_owned()])
        );
        // A browser stays at the root: node's `new URL('../../c',
        // 'http://h/a/b').pathname` is `/c`.
        assert_eq!(
            resolve_relative(&RelativeBase::of(&p), "../../c"),
            Some(vec!["c".to_owned()])
        );
        assert_eq!(
            resolve_relative(&RelativeBase::of(&p), "./%2e%2e/%2E%2E/c"),
            Some(vec!["c".to_owned()])
        );
        assert_eq!(resolve_relative(&RelativeBase::of(&p), ".."), Some(vec![]));
        assert_eq!(resolve_relative(&RelativeBase::of(&p), "%FF"), None);
        let home = PagePath::home();
        assert_eq!(
            resolve_relative(&RelativeBase::of(&home), "x/y"),
            Some(vec!["x".to_owned(), "y".to_owned()])
        );
    }

    fn heading(id: &str, line: usize) -> HeadingRef {
        HeadingRef {
            id: Arc::from(id),
            dropped: false,
            text: Arc::from(id),
            line,
            level: 2,
        }
    }

    #[test]
    fn suggest_runs_check_anchors_four_searches_in_order() {
        let ix = LooseIndex::build(&[
            heading("capacity-memory", 1),
            heading("h-4-nas-config-backup", 2),
            heading("\u{FE0F}-open-item", 3),
            heading("deploy", 4),
            heading("one-two-three-four", 5),
        ]);
        let mut b = u64::MAX;
        assert_eq!(ix.suggest("capacity--memory", &mut b).0, Some(0));
        assert_eq!(ix.suggest("4-nas-config-backup", &mut b).0, Some(1));
        assert_eq!(ix.suggest("4-nas", &mut b).0, Some(1));
        assert_eq!(ix.suggest("open-item", &mut b).0, Some(2));
        assert_eq!(ix.suggest("deploy-the-thing", &mut b).0, Some(3));
        assert_eq!(ix.suggest("four-three-two-x", &mut b).0, Some(4));
        assert_eq!(ix.suggest("four-x-y", &mut b).0, None);
        assert_eq!(ix.suggest("--", &mut b).0, None);
    }
}
