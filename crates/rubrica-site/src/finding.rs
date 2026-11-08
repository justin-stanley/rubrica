//! What is wrong with the corpus, for `rubrica check` to report.
//!
//! A finding never copies a string of the page it points at: a heading's
//! id and text, a stub's target and a page path are shared ([`Arc`],
//! [`PagePath`]), and the `fix:` text is built when it is asked for
//! ([`Finding::fix`]). So the findings are linear in the links, whatever
//! the targets hold.

use std::sync::Arc;

use rubrica_render::frontmatter::{FrontMatterError, FrontMatterFinding, Severity};
use rubrica_render::RenderFinding;

use crate::links::{render_fragment, NoPage};
use crate::source::{BadPath, FileKind, PagePath, Reserved, WikijsPage};

/// A heading a broken anchor could have meant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// Its id.
    pub id: Arc<str>,
    /// Its text.
    pub text: Arc<str>,
    /// Its line in the target's file.
    pub line: usize,
}

/// Why a fragment reaches nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorProblem {
    /// No element of the target has that id.
    NoSuchId,
    /// The heading with that slug is rendered with no id (DOMPurify's
    /// clobber list): nothing can link to it.
    DroppedId {
        /// The heading's text.
        text: Arc<str>,
    },
    /// `#L<n>`, a source-line citation: nothing has such an id.
    LineCitation {
        /// The line cited.
        n: u64,
        /// The line is above every heading but the target's first `h1`, or
        /// above every heading: the fix is to drop the fragment.
        drop_fragment: bool,
    },
}

/// Why a page path behaves differently in Wiki.js, with
/// [`crate::SiteConfig::wikijs_limits`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WikijsPathProblem {
    /// Wiki.js's git import makes another page of the file
    /// (`getPagePath` deletes every `.` before the extension, so `v2.1.md`
    /// is page `v21`, and `a.b/c.md` page `c` in locale `ab`): links to the
    /// page's path find nothing there.
    ImportedAs(WikijsPage),
    /// One of Wiki.js's own routes answers the page's URL first (`/login`,
    /// `/login/<x>`, `/login/<x>/callback`, `/logout`, `/register`,
    /// `/verify/<x>`, `/login-reset/<x>`, `/healthz`, `/graphql…`; any
    /// ASCII case, as Express matches): Wiki.js never shows the page.
    Route,
    /// The first segment is one of Wiki.js's `reservedPaths` (`login`,
    /// `logout`, `register`, `verify`, `favicons`, `fonts`, `img`, `js`,
    /// `svg`; `server/app/data.yml`): Wiki.js shows the page but its editor
    /// refuses to open it (`isReservedPath`).
    EditorReserved,
}

/// Which repository budget a site is over ([`Finding::RepositoryOverBudget`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Budget {
    /// Entries: [`crate::MAX_REPO_FILES`].
    Files,
    /// Path bytes: [`crate::MAX_REPO_PATH_BYTES`].
    PathBytes,
    /// File bytes read: [`crate::MAX_REPO_BYTES`].
    Bytes,
    /// One tree object's bytes (the git reader's):
    /// [`crate::MAX_REPO_TREE_BYTES`].
    TreeBytes,
    /// One commit or tag object's bytes (the git reader's: 1 MiB).
    CommitBytes,
    /// Every git object's bytes the build reads, summed (the git
    /// reader's: 4 GiB).
    ObjectBytes,
    /// One git object that cannot be rebuilt from its delta chain within
    /// the git reader's allocation limit (64 MiB, for one object's
    /// reconstruction): `object` names it.
    DeltaBase {
        /// The object: its kind, its id and, for a file or directory of
        /// the tree built, its path.
        object: Arc<str>,
    },
    /// One git object whose delta chain is deeper than git writes (the git
    /// reader's: 4095): `object` names it.
    DeltaDepth {
        /// The object, as [`Budget::DeltaBase`] names it.
        object: Arc<str>,
    },
}

/// How `rubrica check` is run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CheckOptions {
    /// `--strict`: a dead anchor is an error (check-anchors.cjs's
    /// `--strict`), and a warning fails the check.
    pub strict: bool,
}

/// One finding. Every one names the file and the line (1-based, in the
/// file) it is about.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Finding {
    /// An internal link to a path that is no page.
    BrokenPageLink {
        /// The linking file.
        file: Arc<str>,
        /// The link's line.
        line: usize,
        /// The href as written.
        href: String,
        /// The cleaned path it names; for [`NoPage::Invalid`], the href's
        /// path as a browser writes it (percent-encoded: ASCII, no control
        /// character).
        path: String,
        /// Why it is no page.
        why: NoPage,
    },
    /// A link whose fragment reaches no element of its target page.
    BrokenAnchor {
        /// The linking file.
        file: Arc<str>,
        /// The link's line.
        line: usize,
        /// The href as written.
        href: String,
        /// The target page (the linking page itself for `#x`).
        to: PagePath,
        /// Whether the link is `#x` on the same page.
        same_page: bool,
        /// The fragment, percent-decoded.
        fragment: String,
        /// Why it reaches nothing.
        why: AnchorProblem,
        /// What check-anchors.cjs would suggest instead.
        suggestion: Option<Suggestion>,
    },
    /// A relative link (`../x`, `x`) to a `.md` file of the repository:
    /// Wiki.js resolves it against the page as a directory (`/<page>/x`), a
    /// browser against the page's directory, so the two reach different
    /// pages (`check-frontmatter.cjs`'s rule, which asks only whether the
    /// file exists: a draft, `README.md`, a file under `scripts/` or with
    /// bad front matter count). A relative link to no `.md` file is not a
    /// finding.
    RelativeLink {
        /// The linking file.
        file: Arc<str>,
        /// The link's line.
        line: usize,
        /// The href as written.
        href: String,
        /// The `.md` file a browser's reading reaches.
        reaches: Arc<str>,
        /// That file's page, when it is a published one (the fix).
        to: Option<PagePath>,
    },
    /// A link to a file (a path with a `.` in its last segment) the site
    /// does not have. Wiki.js never checks these.
    BrokenAssetLink {
        /// The linking file.
        file: Arc<str>,
        /// The link's line.
        line: usize,
        /// The href as written.
        href: String,
    },
    /// An `@sec` id another file already uses (check-packages.cjs's ids are
    /// repo-wide, over every `.md` it reads).
    DuplicateSecId {
        /// The file of the second use.
        file: Arc<str>,
        /// Its line.
        line: usize,
        /// The id.
        id: String,
        /// The file of the first use (in path order).
        first_file: Arc<str>,
        /// Its line.
        first_line: usize,
    },
    /// A front-matter block that does not parse: the file is not a page.
    BadFrontMatter {
        /// The file.
        file: Arc<str>,
        /// The parser's error (its line is the file's).
        error: FrontMatterError,
    },
    /// A front-matter finding (`varchar(255)`, Wiki.js's reading of
    /// `published` and tags), with [`crate::SiteConfig::wikijs_limits`].
    FrontMatter {
        /// The file.
        file: Arc<str>,
        /// The finding (its line is the file's).
        finding: FrontMatterFinding,
    },
    /// A finding of the page's render (`@sec`, footnotes, raw HTML, bounds).
    Render {
        /// The file.
        file: Arc<str>,
        /// The finding's line in the file.
        line: usize,
        /// The finding (its own line is the body's, except for
        /// [`RenderFinding::SecInFrontMatter`]).
        finding: RenderFinding,
    },
    /// Two files whose URLs differ only in case or Unicode normalisation
    /// (full case folding over NFD, as APFS compares names): one file on a
    /// case- or normalisation-insensitive disk, and two pages to Wiki.js.
    /// Or the same path twice. `rubrica build` refuses to build on it.
    DuplicateUrl {
        /// The later file (in path order).
        file: Arc<str>,
        /// The earlier one.
        other: Arc<str>,
    },
    /// A file and a directory of the repository that are one name on a
    /// case- or normalisation-insensitive disk (the fold of
    /// [`Finding::DuplicateUrl`]): a file `notes` (or `Notes`) beside
    /// `notes/x.md`, `xyz.md` beside `xyz.md/y.md`; or one directory spelled
    /// two ways (`assets/x.png` beside `Assets/x.md`), reported once, at the
    /// second spelling's first file. Git cannot hold a blob and a tree of
    /// one name, nor APFS two such names; a second spelling, which git
    /// holds, merges silently on APFS (a checkout puts both directories'
    /// files in one), which is why it is refused too. A submodule counts as
    /// a file here, though a checkout makes it a directory: a submodule
    /// `Notes` beside `notes/x.md` is one directory spelled two ways on
    /// APFS, reported as this. `rubrica build` refuses to build on it.
    DirectoryFileConflict {
        /// The later file (in path order).
        file: Arc<str>,
        /// The earlier one it conflicts with.
        other: Arc<str>,
    },
    /// A reserved page path: no page. `rubrica build` refuses to build on it.
    ReservedPagePath {
        /// The file.
        file: Arc<str>,
        /// Why.
        why: Reserved,
    },
    /// `a/index.md` with no page `a`: the static server answers `/a` with
    /// it (`try_files {path}.html {path} {path}/index.html`), while
    /// to Wiki.js, and to the link check, `/a` is no page and the page is
    /// `/a/index`. `rubrica build` refuses to build on it; the fix is `a.md`.
    IndexPageWithoutSection {
        /// The file.
        file: Arc<str>,
    },
    /// A repository path, of any file (a page, an asset, an ignored file,
    /// a symbolic link), longer than [`crate::MAX_PATH_BYTES`] or of more
    /// than [`crate::MAX_PATH_SEGMENTS`] segments: refused before any other
    /// work on it, it takes part in nothing else (no page, asset, fold or
    /// other finding). `rubrica build` refuses to build on it.
    PathTooLong {
        /// The path's first 200 bytes (to a character boundary) and `…`:
        /// the finding holds no more of it.
        file: Arc<str>,
        /// The path's length in bytes.
        bytes: usize,
        /// Its segments.
        segments: usize,
    },
    /// A `.md` file whose path is no page path.
    BadSourcePath {
        /// The file.
        file: Arc<str>,
        /// Why.
        why: BadPath,
    },
    /// A file the site would use that is not a regular file (a symbolic
    /// link, a submodule): never read, neither its bytes nor what it points
    /// at. Also a symbolic link or submodule where a directory of pages or
    /// assets could be (`linked`, `assets`, a nested checkout): were it a
    /// directory, nothing under it would be read, so it is reported rather
    /// than ignored.
    NotARegularFile {
        /// The file.
        file: Arc<str>,
        /// What it is.
        kind: FileKind,
        /// Reported for what it could hide as a directory, not as a file
        /// the site uses.
        directory: bool,
    },
    /// A file the site would use that is larger than its reader's cap
    /// ([`FileKind::TooLarge`]): never read. `rubrica build` refuses to build on it.
    FileTooLarge {
        /// The file.
        file: Arc<str>,
    },
    /// A `.md` file that is not UTF-8.
    NotUtf8 {
        /// The file.
        file: Arc<str>,
    },
    /// A `.md` file under `assets/`, which holds uploaded files only.
    MdUnderAssets {
        /// The file.
        file: Arc<str>,
    },
    /// A page path longer than Wiki.js's `varchar(255)`, in code points,
    /// with [`crate::SiteConfig::wikijs_limits`].
    PathTooLongForWikijs {
        /// The file.
        file: Arc<str>,
        /// The path's length in code points.
        chars: usize,
    },
    /// A page Wiki.js would import as another page, never show, or refuse
    /// to edit, with [`crate::SiteConfig::wikijs_limits`].
    PathDiffersInWikijs {
        /// The file.
        file: Arc<str>,
        /// How.
        why: WikijsPathProblem,
    },
    /// A redirect stub whose `redirect_to` names no page.
    RedirectTargetMissing {
        /// The stub's file.
        file: Arc<str>,
        /// The key's line.
        line: usize,
        /// `redirect_to` as written.
        target: String,
    },
    /// A redirect stub whose target is a stub (or itself).
    RedirectChain {
        /// The stub's file.
        file: Arc<str>,
        /// The key's line.
        line: usize,
        /// The target stub.
        target: PagePath,
    },
    /// A link to a redirect stub: the link should name the stub's target.
    LinkToRedirectStub {
        /// The linking file.
        file: Arc<str>,
        /// The link's line.
        line: usize,
        /// The href as written.
        href: String,
        /// The stub.
        stub: PagePath,
        /// Where the stub sends readers: its target page's URL and
        /// `redirect_to`'s fragment, or `redirect_to` as written when that
        /// names no page. Shared by every link to the stub.
        target: Arc<str>,
    },
    /// The repository is over a budget ([`Budget`]): more entries, path
    /// bytes or file bytes than a site is made from. Nothing else of it is
    /// looked at (no page, no other finding), so the work it costs is
    /// counting. Reported at `.`, line 1. `rubrica build` refuses to build on it.
    RepositoryOverBudget {
        /// Which budget.
        what: Budget,
        /// At least this many: a reader stops at the first entry or file
        /// over the limit.
        at_least: u64,
        /// The limit.
        limit: u64,
    },
    /// An asset larger than [`crate::MAX_ASSET_BYTES`].
    AssetTooLarge {
        /// The asset.
        file: Arc<str>,
        /// Its size.
        bytes: u64,
    },
    /// The assets total more than [`crate::ASSET_BUDGET_WARN`] (a
    /// warning) or [`crate::ASSET_BUDGET_ERROR`] (an error) bytes
    /// Reported at `assets`, line 1.
    AssetBudget {
        /// Their total.
        bytes: u64,
    },
    /// The suggestion search's budget ran out, so some [`Finding::BrokenAnchor`]s
    /// have no word-overlap suggestion. Reported once, at the first link
    /// that lost one.
    SuggestionsBounded {
        /// The linking file.
        file: Arc<str>,
        /// The link's line.
        line: usize,
    },
}

impl Finding {
    /// The file it is about.
    pub fn file(&self) -> &str {
        match self {
            Self::BrokenPageLink { file, .. }
            | Self::BrokenAnchor { file, .. }
            | Self::RelativeLink { file, .. }
            | Self::BrokenAssetLink { file, .. }
            | Self::DuplicateSecId { file, .. }
            | Self::BadFrontMatter { file, .. }
            | Self::FrontMatter { file, .. }
            | Self::Render { file, .. }
            | Self::DuplicateUrl { file, .. }
            | Self::DirectoryFileConflict { file, .. }
            | Self::ReservedPagePath { file, .. }
            | Self::IndexPageWithoutSection { file }
            | Self::PathTooLong { file, .. }
            | Self::BadSourcePath { file, .. }
            | Self::NotARegularFile { file, .. }
            | Self::FileTooLarge { file }
            | Self::NotUtf8 { file }
            | Self::MdUnderAssets { file }
            | Self::PathTooLongForWikijs { file, .. }
            | Self::PathDiffersInWikijs { file, .. }
            | Self::RedirectTargetMissing { file, .. }
            | Self::RedirectChain { file, .. }
            | Self::LinkToRedirectStub { file, .. }
            | Self::SuggestionsBounded { file, .. }
            | Self::AssetTooLarge { file, .. } => file,
            Self::RepositoryOverBudget { .. } => ".",
            Self::AssetBudget { .. } => crate::source::ASSETS_DIR,
        }
    }

    /// The 1-based line in the file (1 for a finding about the whole file).
    pub fn line(&self) -> usize {
        match self {
            Self::BrokenPageLink { line, .. }
            | Self::BrokenAnchor { line, .. }
            | Self::RelativeLink { line, .. }
            | Self::BrokenAssetLink { line, .. }
            | Self::DuplicateSecId { line, .. }
            | Self::Render { line, .. }
            | Self::RedirectTargetMissing { line, .. }
            | Self::RedirectChain { line, .. }
            | Self::LinkToRedirectStub { line, .. }
            | Self::SuggestionsBounded { line, .. } => *line,
            Self::BadFrontMatter { error, .. } => error.line.max(1),
            Self::FrontMatter { finding, .. } => finding.line(),
            Self::DuplicateUrl { .. }
            | Self::DirectoryFileConflict { .. }
            | Self::ReservedPagePath { .. }
            | Self::IndexPageWithoutSection { .. }
            | Self::PathTooLong { .. }
            | Self::BadSourcePath { .. }
            | Self::NotARegularFile { .. }
            | Self::FileTooLarge { .. }
            | Self::NotUtf8 { .. }
            | Self::MdUnderAssets { .. }
            | Self::PathTooLongForWikijs { .. }
            | Self::PathDiffersInWikijs { .. }
            | Self::RepositoryOverBudget { .. }
            | Self::AssetTooLarge { .. }
            | Self::AssetBudget { .. } => 1,
        }
    }

    /// How bad it is, run as `opts` says. A warning fails `check` only
    /// under `--strict`. A dead anchor is a warning, and an error under
    /// `--strict`, as check-anchors.cjs is warn-only unless strict.
    pub fn severity(&self, opts: CheckOptions) -> Severity {
        match self {
            Self::BrokenAnchor { .. } if opts.strict => Severity::Error,
            Self::BrokenAnchor { .. }
            | Self::BrokenPageLink {
                why: NoPage::Unpublished,
                ..
            }
            | Self::BrokenAssetLink { .. }
            | Self::SuggestionsBounded { .. }
            | Self::PathDiffersInWikijs {
                why: WikijsPathProblem::EditorReserved,
                ..
            } => Severity::Warning,
            Self::AssetBudget { bytes } if *bytes <= crate::source::ASSET_BUDGET_ERROR => {
                Severity::Warning
            }
            Self::FrontMatter { finding, .. } => finding.severity(),
            Self::Render { finding, .. } => finding.severity(),
            _ => Severity::Error,
        }
    }

    /// Whether `rubrica build` refuses to write a site with this finding
    /// the repository's files or paths would collide in `out/` or
    /// on a case- or normalisation-insensitive disk, would be served as
    /// something else, are not what git says a page is, or are over a
    /// budget; or a page would vanish from the build without a word (its
    /// file unreadable, not UTF-8, or its front matter unparsable). The
    /// links and the content are `rubrica check`'s to fail on, not the
    /// build's. Every variant is named, so a new one must be decided.
    pub fn refuses_build(&self) -> bool {
        match self {
            Self::DuplicateUrl { .. }
            | Self::DirectoryFileConflict { .. }
            | Self::ReservedPagePath { .. }
            | Self::IndexPageWithoutSection { .. }
            | Self::PathTooLong { .. }
            | Self::BadSourcePath { .. }
            | Self::NotARegularFile { .. }
            | Self::FileTooLarge { .. }
            | Self::NotUtf8 { .. }
            | Self::BadFrontMatter { .. }
            | Self::MdUnderAssets { .. }
            | Self::RepositoryOverBudget { .. }
            | Self::AssetTooLarge { .. } => true,
            Self::AssetBudget { .. } => self.severity(CheckOptions::default()) == Severity::Error,
            Self::BrokenPageLink { .. }
            | Self::BrokenAnchor { .. }
            | Self::RelativeLink { .. }
            | Self::BrokenAssetLink { .. }
            | Self::DuplicateSecId { .. }
            | Self::FrontMatter { .. }
            | Self::Render { .. }
            | Self::PathTooLongForWikijs { .. }
            | Self::PathDiffersInWikijs { .. }
            | Self::RedirectTargetMissing { .. }
            | Self::RedirectChain { .. }
            | Self::LinkToRedirectStub { .. }
            | Self::SuggestionsBounded { .. } => false,
        }
    }

    /// The href to write instead, built from the finding (as
    /// check-anchors.cjs and check-frontmatter.cjs write their `fix:`), if
    /// there is one:
    /// - [`Finding::BrokenAnchor`]: the link's path as written, `#`, and the
    ///   suggested id as [`render_fragment`] writes it; the path alone when
    ///   the fragment should go;
    /// - [`Finding::RelativeLink`] to a published page: the page's URL, then
    ///   the href's query or fragment (to any other file, none: the absolute
    ///   link would name no page);
    /// - [`Finding::LinkToRedirectStub`]: the stub's target, with the href's
    ///   own fragment in place of the target's when it has one.
    pub fn fix(&self) -> Option<String> {
        match self {
            Self::BrokenAnchor {
                href,
                why,
                suggestion,
                ..
            } => {
                let path = &href[..href.find('#').unwrap_or(href.len())];
                match (suggestion, why) {
                    (Some(s), _) => Some(format!("{path}#{}", render_fragment(&s.id))),
                    (
                        None,
                        AnchorProblem::LineCitation {
                            drop_fragment: true,
                            ..
                        },
                    ) => Some(path.to_owned()),
                    _ => None,
                }
            }
            Self::RelativeLink { href, to, .. } => {
                let cut = href.find(['?', '#']).unwrap_or(href.len());
                to.as_ref()
                    .map(|to| format!("{}{}", to.url(), &href[cut..]))
            }
            Self::LinkToRedirectStub { href, target, .. } => Some(match href.find('#') {
                Some(i) => {
                    let base = &target[..target.find('#').unwrap_or(target.len())];
                    format!("{base}{}", &href[i..])
                }
                None => target.to_string(),
            }),
            _ => None,
        }
    }
}
