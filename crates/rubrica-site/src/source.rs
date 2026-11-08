//! Source files and the page paths and URLs made from them.
//!
//! A [`SourceFile`]'s path comes from a git tree, so it is untrusted: a
//! [`PagePath`] is only ever made by [`PagePath::new`], which refuses
//! anything that could name a file outside the corpus or a URL Wiki.js or a
//! browser would read differently. [`PagePath::new`] checks a path's shape
//! and characters only; whether a repository file is a page, and which page,
//! is [`page_path_for_new_file`]'s rule (the one [`crate::Site`] applies to
//! each file), and whether a new file may be added is
//! [`crate::Site::check_new_page`]'s.
//! Nothing in this crate touches a file system; the paths are keys.

use std::fmt;
use std::sync::Arc;

use unicode_normalization::UnicodeNormalization;

use crate::path_tables::{CASE_FOLD, FORMAT};

/// What a repository entry is, as git records it (a tree entry's mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileKind {
    /// A file (`100644` or `100755`; the mode is not kept, and the build
    /// never writes an executable bit).
    #[default]
    Regular,
    /// A symbolic link (`120000`): its bytes are the link's target path,
    /// never the target's content, and the site never reads either.
    Symlink,
    /// A submodule (`160000`): a commit of another repository.
    Submodule,
    /// Anything else a directory walk can meet (a FIFO, a socket, a device).
    Other,
    /// A regular file larger than the reader's cap (`rubrica check`'s
    /// `MAX_FILE_BYTES`): never read, so it holds no bytes.
    TooLarge,
    /// A git tree entry whose mode is none of a file's (`100644`,
    /// `100755`, `100664`), a link's (`120000`), a submodule's (`160000`)
    /// or a directory's (`040000`): `123456`, which gix would read as a
    /// link by its type bits. Never read.
    UnknownMode,
}

/// A file of the wiki repository, read by the caller (from a git tree or a
/// directory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// Repo-relative, `/`-separated (`architecture/topology.md`).
    pub path: String,
    /// The file's bytes (for a [`FileKind::Symlink`], its target path; for
    /// a submodule, nothing). Only a [`FileKind::Regular`] file's are read.
    pub bytes: Vec<u8>,
    /// The git blob id, when the caller has it.
    pub blob_id: Option<String>,
    /// What the entry is. Anything but [`FileKind::Regular`] is
    /// [`crate::Finding::NotARegularFile`] where the site would use it.
    pub kind: FileKind,
}

impl SourceFile {
    /// A regular file with no blob id.
    pub fn regular(path: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            bytes: bytes.into(),
            blob_id: None,
            kind: FileKind::Regular,
        }
    }
}

/// The longest repository path, in bytes, of any file (so a page's file,
/// `.md` included: its page path stops at 1,021 bytes). Wiki.js's
/// `varchar(255)` counts code points; this is a safety bound on every path,
/// not a Wiki.js rule (that one is [`crate::Finding::PathTooLongForWikijs`]).
pub const MAX_PATH_BYTES: usize = 1024;

/// The most segments a page path has, which bounds the depth of the nav tree
/// and of breadcrumbs.
pub const MAX_PATH_SEGMENTS: usize = 64;

/// The most repository entries a site is made from: every file, link and
/// submodule a reader hands over, ignored ones included (each costs path work, and nothing else capped
/// their number). Over it
/// the site is [`crate::Finding::RepositoryOverBudget`] and nothing else.
pub const MAX_REPO_FILES: usize = 250_000;

/// The most bytes of repository paths, summed over every entry
/// ([`MAX_REPO_FILES`]'s count). The directory check's work per file is
/// bounded by its path's bytes times its depth, so this bounds the whole
/// site's path work at 10 × [`MAX_PATH_SEGMENTS`] × this.
pub const MAX_REPO_PATH_BYTES: usize = 16 << 20;

/// The most bytes of file content the site is handed (pages, assets and the
/// `.md` files check-packages.cjs harvests), summed.
pub const MAX_REPO_BYTES: u64 = 128 << 20;

/// The largest tree object the git reader reads, by its object header's
/// size, before it is read (a cap on each tree, in the
/// tree built, on the way to `--subtree` and in the history). Any one tree
/// git writes in a repository within [`MAX_REPO_FILES`] and
/// [`MAX_REPO_PATH_BYTES`] is within it: the reader counts each of the
/// tree's entries as an entry and its name as path bytes (on the way to
/// `--subtree` too, where a name is counted with the path before it), and
/// an entry takes its name and at most 28 bytes more: a mode as git writes
/// it, of five or six octal digits (`40000`, `100644`), a space, a NUL and
/// a 20-byte id. That holds for SHA-1 only, the one hash Rubrica's gix
/// reads (a SHA-256 id is 32 bytes); gix also reads a mode of seven
/// digits, which git never writes: a tree made so is up to one byte an
/// entry larger, and may be refused.
pub const MAX_REPO_TREE_BYTES: u64 = 28 * MAX_REPO_FILES as u64 + MAX_REPO_PATH_BYTES as u64;

/// The largest asset: 2 MiB.
pub const MAX_ASSET_BYTES: u64 = 2 << 20;

/// The assets' total over which the repository is warned about (decision
/// 8): 25 MB.
pub const ASSET_BUDGET_WARN: u64 = 25_000_000;

/// The assets' total over which it is an error: 50 MB.
pub const ASSET_BUDGET_ERROR: u64 = 50_000_000;

/// A reader's running count of what it has handed over, against
/// [`MAX_REPO_FILES`], [`MAX_REPO_PATH_BYTES`] and [`MAX_REPO_BYTES`] (and,
/// for the git reader, the object budget it stopped on, if one:
/// [`RepoBudget::refuse`]). A
/// reader counts every entry and every byte it reads, and stops at the
/// first one that goes over: what it has then handed over is over the
/// budget, which the site reports, by at most one entry and one file
/// (a file is capped by the reader too), so a reader never holds much
/// more than the budget.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoBudget {
    /// Entries counted.
    pub files: usize,
    /// Their paths' bytes.
    pub path_bytes: usize,
    /// File bytes read.
    pub bytes: u64,
    /// A budget a reader refused on its own count ([`RepoBudget::refuse`]):
    /// which, at least how much, and its limit.
    pub refused: Option<(crate::Budget, u64, u64)>,
}

impl RepoBudget {
    /// Count one entry at `path`; whether the budget still holds.
    pub fn entry(&mut self, path: &str) -> bool {
        self.entry_len(path.len())
    }

    /// Count one entry whose path is `len` bytes (a name that is not UTF-8
    /// too); whether the budget still holds.
    pub fn entry_len(&mut self, len: usize) -> bool {
        self.files += 1;
        self.path_bytes = self.path_bytes.saturating_add(len);
        self.holds()
    }

    /// Count `n` bytes read; whether the budget still holds.
    pub fn read(&mut self, n: u64) -> bool {
        self.bytes = self.bytes.saturating_add(n);
        self.holds()
    }

    /// Record that the reader stopped on a budget it counts itself (the
    /// git reader's objects): `what`, at least `at_least`, over `limit`.
    /// The budget no longer holds.
    pub fn refuse(&mut self, what: crate::Budget, at_least: u64, limit: u64) {
        self.refused.get_or_insert((what, at_least, limit));
    }

    /// Whether every count is within its limit.
    pub fn holds(&self) -> bool {
        self.refused.is_none()
            && self.files <= MAX_REPO_FILES
            && self.path_bytes <= MAX_REPO_PATH_BYTES
            && self.bytes <= MAX_REPO_BYTES
    }

    /// A [`crate::Finding::RepositoryOverBudget`] for each count over its
    /// limit (none when the budget holds).
    pub fn findings(&self) -> Vec<crate::Finding> {
        use crate::Budget;
        [
            (Budget::Files, self.files as u64, MAX_REPO_FILES as u64),
            (
                Budget::PathBytes,
                self.path_bytes as u64,
                MAX_REPO_PATH_BYTES as u64,
            ),
            (Budget::Bytes, self.bytes, MAX_REPO_BYTES),
        ]
        .into_iter()
        .filter(|&(_, at_least, limit)| at_least > limit)
        .chain(self.refused.clone())
        .map(
            |(what, at_least, limit)| crate::Finding::RepositoryOverBudget {
                what,
                at_least,
                limit,
            },
        )
        .collect()
    }
}

/// A page's path: its source file's repo path without `.md`
/// (`architecture/topology`; the home page is `home`). Every segment is
/// non-empty, is not `.` or `..`, neither starts nor ends with whitespace,
/// and has no `/`, `\`, `%`, control character (C0, DEL, C1, U+2028,
/// U+2029), format character (Unicode's Cf: the byte-order mark, zero
/// widths and the bidi controls), invisible letter ([`INVISIBLE_LETTERS`]:
/// the Hangul fillers and the blank Braille pattern), or any of `?#"|<>:*`
/// (a query or fragment separator, or a character Wiki.js's `parsePath`
/// deletes from a link).
///
/// Cheap to clone: every copy shares one string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PagePath(Arc<str>);

/// Why a string is not a [`PagePath`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadPath {
    /// Empty, or an empty segment (`a//b`, a leading or trailing `/`).
    EmptySegment,
    /// A `.` or `..` segment.
    DotSegment,
    /// A character no page path has.
    Character(char),
    /// A segment starting or ending with whitespace (Wiki.js trims the
    /// path it reads, so no link reaches such a page there).
    EdgeWhitespace,
    /// Longer than [`MAX_PATH_BYTES`].
    TooLong,
    /// More segments than [`MAX_PATH_SEGMENTS`].
    TooDeep,
    /// A percent-encoding whose bytes are not UTF-8 (links only).
    NotUtf8,
}

impl fmt::Display for BadPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySegment => write!(f, "an empty path segment"),
            Self::DotSegment => write!(f, "a `.` or `..` segment"),
            Self::Character(c) => write!(f, "the character {c:?}"),
            Self::EdgeWhitespace => write!(f, "a segment starting or ending with whitespace"),
            Self::TooLong => write!(f, "longer than {MAX_PATH_BYTES} bytes"),
            Self::TooDeep => write!(f, "more than {MAX_PATH_SEGMENTS} segments"),
            Self::NotUtf8 => write!(f, "a percent-encoding that is not UTF-8"),
        }
    }
}

fn in_ranges(table: &[(u32, u32)], c: char) -> bool {
    let c = c as u32;
    let k = table.partition_point(|&(_, hi)| hi < c);
    table.get(k).is_some_and(|&(lo, _)| lo <= c)
}

/// Whether `c` is a format character (general category Cf).
pub(crate) fn is_format(c: char) -> bool {
    in_ranges(FORMAT, c)
}

/// Characters that draw nothing but are no format character (Cf) nor
/// whitespace: the Hangul fillers (U+115F, U+1160, U+3164, U+FFA0, letters
/// to Unicode) and the blank Braille pattern (U+2800, a symbol). A page path
/// with one would look like another, so none has one.
pub const INVISIBLE_LETTERS: [char; 5] =
    ['\u{115F}', '\u{1160}', '\u{3164}', '\u{FFA0}', '\u{2800}'];

/// Whether `c` may appear in a page path's segment.
pub(crate) fn segment_char_ok(c: char) -> bool {
    !(c.is_control()
        || is_format(c)
        || INVISIBLE_LETTERS.contains(&c)
        || matches!(
            c,
            '/' | '\\'
                | '%'
                | '?'
                | '#'
                | '"'
                | '|'
                | '<'
                | '>'
                | ':'
                | '*'
                | '\u{2028}'
                | '\u{2029}'
        ))
}

/// Checks one segment.
pub(crate) fn check_segment(s: &str) -> Result<(), BadPath> {
    if s.is_empty() {
        return Err(BadPath::EmptySegment);
    }
    if s == "." || s == ".." {
        return Err(BadPath::DotSegment);
    }
    if let Some(c) = s.chars().find(|&c| !segment_char_ok(c)) {
        return Err(BadPath::Character(c));
    }
    // Every character JavaScript's `\s` (lodash's `trim`) takes but
    // U+FEFF, a format character refused above; U+0085 is a control.
    let edge = |c: Option<char>| c.is_some_and(char::is_whitespace);
    if edge(s.chars().next()) || edge(s.chars().next_back()) {
        return Err(BadPath::EdgeWhitespace);
    }
    Ok(())
}

impl PagePath {
    /// The home page's path.
    pub const HOME: &'static str = "home";

    /// A page path, its shape and characters checked. This is not the rule
    /// for which files are pages ([`page_path_for_new_file`] is): a
    /// `PagePath` may be reserved (`_x`, `t/x`, `index`) or ignored
    /// (`scripts/x`), and code that names a new page file must use
    /// [`crate::Site::check_new_page`] (that rule and the files already
    /// there), not this.
    ///
    /// # Errors
    ///
    /// A [`BadPath`] naming what is wrong with `s`.
    pub fn new(s: &str) -> Result<Self, BadPath> {
        if s.len() > MAX_PATH_BYTES {
            return Err(BadPath::TooLong);
        }
        let mut n = 0;
        for seg in s.split('/') {
            n += 1;
            if n > MAX_PATH_SEGMENTS {
                return Err(BadPath::TooDeep);
            }
            check_segment(seg)?;
        }
        Ok(Self(Arc::from(s)))
    }

    /// The home page.
    pub fn home() -> Self {
        Self(Arc::from(Self::HOME))
    }

    /// The path as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The shared buffer every copy of this path points at.
    pub(crate) fn shared(&self) -> &Arc<str> {
        &self.0
    }

    /// Its segments.
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }

    /// Whether this is the home page (`home.md` at the root).
    pub fn is_home(&self) -> bool {
        &*self.0 == Self::HOME
    }

    /// The page's URL: `/` for the home page, `/<path>` otherwise. Not
    /// percent-encoded (a template encodes it where it writes it).
    pub fn url(&self) -> String {
        if self.is_home() {
            "/".to_owned()
        } else {
            format!("/{}", self.0)
        }
    }

    /// The directory the page is in (`architecture` for
    /// `architecture/topology`; `""` at the root).
    pub fn dir(&self) -> &str {
        self.0.rsplit_once('/').map_or("", |(d, _)| d)
    }

    /// The last segment.
    pub fn name(&self) -> &str {
        self.0.rsplit_once('/').map_or(&self.0, |(_, n)| n)
    }
}

impl fmt::Display for PagePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The wiki's own origin, as configured, checked: an
/// `http` or `https` scheme, a non-empty host (and port), nothing after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin(String);

/// Why a string is not an [`Origin`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadOrigin {
    /// Not `http://` or `https://`.
    Scheme,
    /// No host after the scheme.
    EmptyHost,
    /// A path, query, fragment, userinfo, whitespace or control character
    /// after the scheme (a trailing `/` included).
    NotJustAHost,
}

impl Origin {
    /// An origin, checked. Compared case-sensitively, as Wiki.js's
    /// html-core compares `WIKI.config.host` (`href.indexOf(host + '/') === 0`).
    ///
    /// # Errors
    ///
    /// A [`BadOrigin`] when `s` is not a bare `http(s)` origin.
    pub fn new(s: &str) -> Result<Self, BadOrigin> {
        let host = s
            .strip_prefix("https://")
            .or_else(|| s.strip_prefix("http://"))
            .ok_or(BadOrigin::Scheme)?;
        if host.is_empty() {
            return Err(BadOrigin::EmptyHost);
        }
        if host.chars().any(|c| {
            c.is_whitespace() || c.is_control() || matches!(c, '/' | '?' | '#' | '@' | '\\')
        }) {
            return Err(BadOrigin::NotJustAHost);
        }
        Ok(Self(s.to_owned()))
    }

    /// The origin as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Which files of the repository are pages, and how links are read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteConfig {
    /// Directory names skipped at any depth, as check-anchors.cjs's
    /// `SKIP_DIRS` skips them. Any segment starting with `.` (`.git`,
    /// `.github`, `.githooks`) is skipped as well, always.
    pub ignore_dirs: Vec<String>,
    /// Repo-relative files that are not pages (`README.md`).
    pub ignore_files: Vec<String>,
    /// The wiki's own origin: a link to `<origin>/…` is read as the
    /// internal link `/…`, as Wiki.js's html-core does before it classifies
    /// a link.
    pub wiki_origin: Option<Origin>,
    /// The checks that matter only while Wiki.js still imports the repo:
    /// its `varchar(255)` limits, its front-matter reading, and the pages
    /// it would import as another page or never serve
    /// ([`crate::Finding::PathDiffersInWikijs`]).
    pub wikijs_limits: bool,
    /// Which render profile the site's pages use (`RenderOptions::native()` or
    /// `wikijs_compat()`), through [`crate::render_options`].
    pub render: rubrica_render::Profile,
}

impl Default for SiteConfig {
    /// Neutral: `node_modules` is never a page directory, `README.md` is not
    /// a page, no origin is the wiki's own, Wiki.js's limits are off.
    fn default() -> Self {
        Self {
            ignore_dirs: vec!["node_modules".to_owned()],
            ignore_files: vec!["README.md".to_owned()],
            wiki_origin: None,
            wikijs_limits: false,
            render: rubrica_render::Profile::Native,
        }
    }
}

/// Why a page path is reserved: a path Wiki.js, or the build's output
/// layout, gives its own meaning. Such a file is no page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reserved {
    /// A first segment starting with `_`: Rubrica's own files
    /// (`/_recent`, `/_assets/…`).
    Underscore,
    /// The top-level `index` or `404` (any case): the build writes the
    /// home page as `index.html` and the error page as `404.html`.
    OutputName,
    /// A segment ending in `.md` or `.html` (any case): the build writes
    /// `<page>.md` and `<page>.html`, so `a.md/b` would put a directory
    /// where page `a`'s raw copy is, and page `a.md` would answer `/a.md`
    /// before it.
    OutputExtension,
    /// The top-level `robots.txt` or `favicon.ico` (any case): URLs that
    /// browsers and crawlers ask every site for, so a page there would
    /// answer them (`/.well-known/…` needs no rule: a dot directory is
    /// never part of the site).
    WellKnown,
    /// Wiki.js's `parsePath` reads the page's URL as another page, or as
    /// none: a first segment of one UTF-16 unit is dropped (`/t/x`, `/é/x`,
    /// `/1/x`), then a first segment of locale shape is the locale (`/en/x`,
    /// `/ab-cd/x`, so `/1/ab/c` is page `c` in locale `ab`), and runs of
    /// dots are deleted (`a..b`). So no Wiki.js link reaches it.
    UnreachableInWikijs {
        /// What Wiki.js reads instead; `None` when `parsePath` throws (no
        /// segment is left).
        reads_as: Option<WikijsPage>,
    },
}

impl fmt::Display for Reserved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Underscore => write!(f, "a path starting with `_` is reserved for Rubrica's own files"),
            Self::OutputName => write!(f, "`index` and `404` are the build's own top-level files"),
            Self::OutputExtension => write!(f, "a segment ending in `.md` or `.html` collides with the build's `<page>.md` and `<page>.html`"),
            Self::WellKnown => write!(f, "`robots.txt` and `favicon.ico` are URLs browsers and crawlers ask every site for"),
            Self::UnreachableInWikijs { reads_as: Some(p) } => write!(f, "Wiki.js reads this page's URL as page `{}` in locale `{}`, so no link reaches it", p.path, p.locale),
            Self::UnreachableInWikijs { reads_as: None } => write!(f, "Wiki.js cannot read this page's URL as a page, so no link reaches it"),
        }
    }
}

/// A page as Wiki.js names it: (locale, path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikijsPage {
    /// The locale (`en` unless the path names one).
    pub locale: String,
    /// The page path.
    pub path: String,
    /// Whether the path named the locale.
    pub explicit_locale: bool,
}

/// Wiki.js's `WIKI.config.lang.code` for an imported Wiki.js wiki.
pub(crate) const WIKIJS_LOCALE: &str = "en";

/// Wiki.js's `localeSegmentRegex`, `^[A-Z]{2}(-[A-Z]{2})?$` with `i`
/// (`server/helpers/page.js`; ASCII only, as JavaScript's non-Unicode `i`
/// is): `parsePath` takes such a first segment as the locale, so `/ab/c` is
/// page `c` in locale `ab`.
pub(crate) fn is_locale_segment(s: &str) -> bool {
    let b = s.as_bytes();
    let letters = |r: &[u8]| r.iter().all(u8::is_ascii_alphabetic);
    match b.len() {
        2 => letters(b),
        5 => letters(&b[..2]) && b[2] == b'-' && letters(&b[3..]),
        _ => false,
    }
}

/// JavaScript's `\s`, which lodash's `_.trim` trims.
fn is_js_space(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{FEFF}'
}

/// Wiki.js 2.5.314's `parsePath(rawPath)` (`server/helpers/page.js`), for a
/// raw path with no `%` (every [`PagePath`]'s URL), where `qs.unescape`
/// changes nothing: trimmed; one leading `/` off; the first of
/// `[\x00-\x1f\x80-\x9f\\"|<>:*?]` deleted; `''` is `home`; every `\`, then
/// every `//`, then every run of two or more dots deleted; split at `/`,
/// keeping the parts that trimmed are not empty, `.` or `..`; a first part
/// one UTF-16 unit long dropped; then a first part of locale shape taken
/// as the locale. `None` where it throws (no part is left before the
/// drop).
///
/// Its domain is a raw path with no `%` (`qs.unescape` is not modelled);
/// `fixtures/wikijs-path-differential.json` checks it there.
#[doc(hidden)]
pub fn wikijs_parse_path(raw: &str) -> Option<WikijsPage> {
    let mut s = raw.trim_matches(is_js_space).to_owned();
    if let Some(rest) = s.strip_prefix('/') {
        s = rest.to_owned();
    }
    let unsafe_char = |c: char| {
        matches!(c as u32, 0x00..=0x1f | 0x80..=0x9f)
            || matches!(c, '\\' | '"' | '|' | '<' | '>' | ':' | '*' | '?')
    };
    if let Some((i, c)) = s.char_indices().find(|&(_, c)| unsafe_char(c)) {
        s.replace_range(i..i + c.len_utf8(), "");
    }
    if s.is_empty() {
        s = PagePath::HOME.to_owned();
    }
    let s = s.replace('\\', "").replace("//", "");
    // `/\.\.+/g`: every run of two or more dots.
    let mut cleaned = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '.' && chars.peek() == Some(&'.') {
            while chars.peek() == Some(&'.') {
                chars.next();
            }
            continue;
        }
        cleaned.push(c);
    }
    let mut parts: Vec<&str> = cleaned
        .split('/')
        .filter(|p| {
            let t = p.trim_matches(is_js_space);
            !t.is_empty() && t != ".." && t != "."
        })
        .collect();
    if parts.first()?.encode_utf16().count() == 1 {
        parts.remove(0);
    }
    let explicit_locale = parts.first().is_some_and(|p| is_locale_segment(p));
    let locale = if explicit_locale {
        parts.remove(0).to_owned()
    } else {
        WIKIJS_LOCALE.to_owned()
    };
    Some(WikijsPage {
        locale,
        path: parts.join("/"),
        explicit_locale,
    })
}

/// Wiki.js's `getPagePath(filePath)` (`server/helpers/page.js`), the page a
/// git import makes of a file: every `.` before the extension deleted
/// (`_.initial(fpath.split('.')).join('')`, so `v2.1.md` is page `v21`),
/// then a first folder of locale shape taken as the locale
/// (`localeFolderRegex`, so `a.b/c.md` is page `c` in locale `ab`).
///
/// Its domain is a page file's path: a `.md` file whose stem is a
/// [`PagePath`], so no control character (a line terminator would end the
/// regex's `(.*)`, which this port does not model), `\` (which
/// `getPagePath` reads as `/` on Windows only), `%` or the other characters
/// [`PagePath::new`] refuses. On any other string it is not Wiki.js's
/// function. `fixtures/wikijs-path-differential.json` checks it there.
#[doc(hidden)]
pub fn wikijs_page_path(file: &str) -> WikijsPage {
    let parts: Vec<&str> = file.split('.').collect();
    let joined = parts[..parts.len().saturating_sub(1)].concat();
    if let Some((first, rest)) = joined.split_once('/') {
        if is_locale_segment(first) {
            return WikijsPage {
                locale: first.to_owned(),
                path: rest.to_owned(),
                explicit_locale: true,
            };
        }
    }
    WikijsPage {
        locale: WIKIJS_LOCALE.to_owned(),
        path: joined,
        explicit_locale: false,
    }
}

/// The canonical caseless key of `s`, `NFD(casefold(NFD(s)))` with full
/// case folding: equal for names APFS (case- and normalisation-insensitive)
/// takes as one (`ſ` and `s`, the Kelvin sign and `k`, `é` and `e` +
/// U+0301).
pub(crate) fn fold_key(s: &str) -> String {
    let mut folded = String::with_capacity(s.len());
    for c in s.nfd() {
        match CASE_FOLD.binary_search_by_key(&(c as u32), |&(k, _)| k) {
            Ok(i) => folded.push_str(CASE_FOLD[i].1),
            Err(_) => folded.push(c),
        }
    }
    folded.nfd().collect()
}

/// What a repository file is to the site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileRole {
    /// A page source, with its path.
    Page(PagePath),
    /// A file under `assets/`.
    Asset,
    /// A `.md` file under `assets/`.
    MdUnderAssets,
    /// Not part of the site (ignored, or neither a page nor an asset).
    Ignored,
    /// A `.md` file whose path is not a page path.
    BadPath(BadPath),
    /// A `.md` file whose path is reserved, and that path.
    Reserved(PagePath, Reserved),
}

/// The top-level directory of uploaded files; reserved, so no page
/// is under it.
pub const ASSETS_DIR: &str = "assets";

/// What `path` is, under `cfg`.
pub(crate) fn file_role(path: &str, cfg: &SiteConfig) -> FileRole {
    if cfg.ignore_files.iter().any(|f| f == path) {
        return FileRole::Ignored;
    }
    let mut segs = path.split('/');
    let dirs = segs.clone().count().saturating_sub(1);
    for seg in segs.by_ref().take(dirs) {
        if seg.starts_with('.') || cfg.ignore_dirs.iter().any(|d| d == seg) {
            return FileRole::Ignored;
        }
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    if name.starts_with('.') {
        return FileRole::Ignored;
    }
    let under_assets = path.starts_with("assets/");
    let Some(stem) = path.strip_suffix(".md") else {
        return if under_assets {
            FileRole::Asset
        } else {
            FileRole::Ignored
        };
    };
    if under_assets {
        return FileRole::MdUnderAssets;
    }
    let page = match PagePath::new(stem) {
        Ok(p) => p,
        Err(e) => return FileRole::BadPath(e),
    };
    match reserved(&page) {
        Some(why) => FileRole::Reserved(page, why),
        None => FileRole::Page(page),
    }
}

/// Why `page` is reserved, if it is.
fn reserved(page: &PagePath) -> Option<Reserved> {
    if page.as_str().starts_with('_') {
        return Some(Reserved::Underscore);
    }
    if !page.as_str().contains('/') && matches!(fold_key(page.as_str()).as_str(), "index" | "404") {
        return Some(Reserved::OutputName);
    }
    if !page.as_str().contains('/')
        && matches!(
            fold_key(page.as_str()).as_str(),
            "robots.txt" | "favicon.ico"
        )
    {
        return Some(Reserved::WellKnown);
    }
    if page.segments().any(|s| {
        let k = fold_key(s);
        k.ends_with(".md") || k.ends_with(".html")
    }) {
        return Some(Reserved::OutputExtension);
    }
    match wikijs_parse_path(&format!("/{page}")) {
        Some(p) if !p.explicit_locale && p.path == page.as_str() => None,
        reads_as => Some(Reserved::UnreachableInWikijs { reads_as }),
    }
}

/// Why a repository path is no page, for [`page_path_for_new_file`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotAPage {
    /// Ignored (`README.md`, under `scripts/` or a dot directory, a dot
    /// file) or not Markdown.
    Ignored,
    /// A file under `assets/` (uploaded files, no pages).
    UnderAssets,
    /// Its path is no page path.
    BadPath(BadPath),
    /// Its path is reserved.
    Reserved(Reserved),
}

/// Whether repository path `path`, of any file, is over the limits:
/// longer than [`MAX_PATH_BYTES`] ([`BadPath::TooLong`]) or of more than
/// [`MAX_PATH_SEGMENTS`] segments ([`BadPath::TooDeep`]).
/// [`crate::Site::from_sources`] checks it before any other work on the
/// path ([`crate::Finding::PathTooLong`]).
pub(crate) fn path_over_limits(path: &str) -> Option<BadPath> {
    if path.len() > MAX_PATH_BYTES {
        Some(BadPath::TooLong)
    } else if path.bytes().filter(|&b| b == b'/').count() >= MAX_PATH_SEGMENTS {
        Some(BadPath::TooDeep)
    } else {
        None
    }
}

/// The page a file at repo path `path` (`a/b.md`) is, under `cfg`: the rule
/// [`crate::Site::from_sources`] applies to one file alone (the path's
/// limits, ignored files and directories, `assets/`, the path's shape and
/// characters, and the reserved paths). It is not the whole rule for a new file, which also
/// depends on the files already there ([`crate::Finding::DuplicateUrl`],
/// [`crate::Finding::IndexPageWithoutSection`]): the editor and
/// renames name new files through [`crate::Site::check_new_page`],
/// which applies both.
///
/// # Errors
///
/// [`NotAPage`] with the reason the file is no page.
pub fn page_path_for_new_file(path: &str, cfg: &SiteConfig) -> Result<PagePath, NotAPage> {
    if let Some(why) = path_over_limits(path) {
        return Err(NotAPage::BadPath(why));
    }
    match file_role(path, cfg) {
        FileRole::Page(p) => Ok(p),
        FileRole::Ignored => Err(NotAPage::Ignored),
        FileRole::Asset | FileRole::MdUnderAssets => Err(NotAPage::UnderAssets),
        FileRole::BadPath(e) => Err(NotAPage::BadPath(e)),
        FileRole::Reserved(_, r) => Err(NotAPage::Reserved(r)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_tables_are_unicode_17() {
        assert_eq!(crate::path_tables::UNICODE_VERSION, "17.0.0");
    }

    fn reads(raw: &str) -> Option<(String, String, bool)> {
        wikijs_parse_path(raw).map(|p| (p.locale, p.path, p.explicit_locale))
    }

    fn en(path: &str) -> Option<(String, String, bool)> {
        Some(("en".to_owned(), path.to_owned(), false))
    }

    #[test]
    fn parse_path_is_wikijs_parse_path() {
        assert_eq!(reads("/abc/d"), en("abc/d"));
        assert_eq!(reads("/a/b"), en("b"));
        assert_eq!(reads("/"), en("home"));
        assert_eq!(reads("/t/x"), en("x"));
        assert_eq!(reads("/1/xyz"), en("xyz"));
        assert_eq!(reads("/é/xyz"), en("xyz"));
        assert_eq!(reads("/-/xyz"), en("xyz"));
        // An astral letter is two UTF-16 units: kept.
        assert_eq!(reads("/\u{1D49C}/x"), en("\u{1D49C}/x"));
        assert_eq!(
            reads("/1/ab/c"),
            Some(("ab".to_owned(), "c".to_owned(), true))
        );
        assert_eq!(
            reads("/EN-gb/c"),
            Some(("EN-gb".to_owned(), "c".to_owned(), true))
        );
        assert_eq!(reads("/abc..d/e"), en("abcd/e"));
        assert_eq!(
            reads("/a..b/c"),
            Some(("ab".to_owned(), "c".to_owned(), true))
        );
        assert_eq!(reads("/abc/.../d"), en("abc/d"));
        assert_eq!(reads("/abc/d "), en("abc/d"));
        // The first unsafe character only.
        assert_eq!(reads("/abc:d:e"), en("abcd:e"));
        // Nothing left to drop: `pathParts[0].length` throws.
        assert_eq!(reads("/./"), None);
        // One segment of one unit: dropped, the path is empty.
        assert_eq!(reads("/x"), en(""));
    }

    #[test]
    fn get_page_path_deletes_dots_and_reads_a_locale_folder() {
        let p = |f: &str| {
            let w = wikijs_page_path(f);
            (w.locale, w.path)
        };
        assert_eq!(p("a/b.md"), ("en".to_owned(), "a/b".to_owned()));
        assert_eq!(p("v2.1.md"), ("en".to_owned(), "v21".to_owned()));
        assert_eq!(p("a.b/c.md"), ("ab".to_owned(), "c".to_owned()));
        assert_eq!(p("fr/c.md"), ("fr".to_owned(), "c".to_owned()));
    }

    #[test]
    fn the_fold_is_full_case_folding_over_nfd() {
        assert_eq!(fold_key("ſ"), fold_key("s"));
        assert_eq!(fold_key("\u{212A}"), fold_key("k"));
        assert_eq!(fold_key("Straße"), fold_key("STRASSE"));
        assert_eq!(fold_key("caf\u{e9}"), fold_key("CAFE\u{301}"));
        assert_ne!(fold_key("a"), fold_key("b"));
    }

    #[test]
    fn segments_refuse_percent_format_characters_and_edge_whitespace() {
        assert_eq!(check_segment("a%b"), Err(BadPath::Character('%')));
        assert_eq!(
            check_segment("a\u{200B}b"),
            Err(BadPath::Character('\u{200B}'))
        );
        assert_eq!(
            check_segment("a\u{202E}b"),
            Err(BadPath::Character('\u{202E}'))
        );
        assert_eq!(
            check_segment("a\u{2066}"),
            Err(BadPath::Character('\u{2066}'))
        );
        assert_eq!(check_segment("a "), Err(BadPath::EdgeWhitespace));
        assert_eq!(check_segment("\u{3000}a"), Err(BadPath::EdgeWhitespace));
        assert_eq!(check_segment("a\u{a0}"), Err(BadPath::EdgeWhitespace));
        assert_eq!(check_segment("a b"), Ok(()));
    }

    #[test]
    fn an_origin_is_a_scheme_and_a_host() {
        assert!(Origin::new("https://wiki.example").is_ok());
        assert!(Origin::new("http://h:8080").is_ok());
        assert_eq!(Origin::new(""), Err(BadOrigin::Scheme));
        assert_eq!(Origin::new("https://"), Err(BadOrigin::EmptyHost));
        assert_eq!(Origin::new("https://h/"), Err(BadOrigin::NotJustAHost));
        assert_eq!(Origin::new("ftp://h"), Err(BadOrigin::Scheme));
    }

    #[test]
    fn the_default_config_names_no_site() {
        assert!(SiteConfig::default().wiki_origin.is_none());
        assert_eq!(SiteConfig::default().ignore_dirs, ["node_modules"]);
    }
}
