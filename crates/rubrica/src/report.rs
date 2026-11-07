//! `rubrica check`'s report: one [`Item`] per finding, written as text (in
//! `check-anchors.cjs`'s shape, so the wiki's authors see the report they
//! know) or as JSON.
//!
//! Every string in a finding comes from the repository (paths, hrefs,
//! heading text, ids), so the text report escapes, on every line it
//! prints, the characters that can drive or reorder a terminal, those that
//! are invisible, and the backslash its escapes start with ([`escape`]).
//! The JSON report keeps them raw: JSON's own escaping reads back as the
//! string itself.

use std::fmt::Write as _;

use crate::escape_table::ESCAPED;

use rubrica_render::frontmatter::{FrontMatterErrorKind, FrontMatterFinding, Severity};
use rubrica_render::markdown::MAX_PAGE_BYTES;
use rubrica_render::{RenderFinding, SecHidden};
use rubrica_site::{
    AnchorProblem, Budget, CheckOptions, FileKind, Finding, NoPage, Site, WikijsPathProblem,
    ASSET_BUDGET_ERROR, ASSET_BUDGET_WARN, MAX_ASSET_BYTES, MAX_PATH_BYTES, MAX_PATH_SEGMENTS,
};
use serde::Serialize;

use crate::read::{ReadProblem, MAX_FILE_BYTES};

/// One finding, ready to print.
#[derive(Debug, Clone, Serialize)]
pub struct Item {
    /// The repository file (or path) it is about.
    pub file: String,
    /// The 1-based line in it.
    pub line: usize,
    /// `error` or `warning`, as the check is run.
    #[serde(serialize_with = "severity_name")]
    pub severity: Severity,
    /// What it is, as a stable kebab-case name (`broken-anchor`).
    pub kind: String,
    /// The link as written, for a finding about a link.
    pub href: Option<String>,
    /// Why it is wrong.
    pub message: String,
    /// The href to write instead.
    pub fix: Option<String>,
    /// More help.
    pub hint: Option<String>,
    /// The other file of a pair (a duplicate id, URL or spelling).
    pub other_file: Option<String>,
    /// Its line, when it has one.
    pub other_line: Option<usize>,
    /// `file` is already escaped (the reader's `lossless_display`).
    #[serde(skip)]
    pub file_is_lossless: bool,
}

fn severity_name<S: serde::Serializer>(s: &Severity, ser: S) -> Result<S::Ok, S::Error> {
    ser.serialize_str(match s {
        Severity::Error => "error",
        Severity::Warning => "warning",
    })
}

/// How the check was run.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Options {
    /// `--strict`.
    pub strict: bool,
    /// `--same-page`.
    pub same_page: bool,
    /// `--wikijs-limits`.
    pub wikijs_limits: bool,
}

/// The whole report.
#[derive(Debug, Serialize)]
pub struct Report {
    /// The JSON format's version.
    pub format: u32,
    /// How the check was run.
    pub options: Options,
    /// Published pages.
    pub pages: usize,
    /// Links on them.
    pub links: usize,
    /// Errors among `findings`.
    pub errors: usize,
    /// Warnings among `findings`.
    pub warnings: usize,
    /// Dead same-page anchors left out (no `--same-page`).
    pub same_page_unchecked: usize,
    /// Whether the check fails (exit 1).
    pub failed: bool,
    /// Sorted by file and line.
    pub findings: Vec<Item>,
}

/// Whether the text report prints `c` escaped: the controls (C0, DEL,
/// C1), the line and paragraph separators, and every format character (Cf:
/// the bidi controls, zero-width characters, the BOM, the tag characters)
/// and default-ignorable one (variation selectors, Hangul fillers, the
/// rest of plane 14), Unicode 17 ([`ESCAPED`]), which would reorder a line
/// or make two different names print alike.
fn hostile(c: char) -> bool {
    let u = c as u32;
    let i = ESCAPED.partition_point(|&(_, hi)| hi < u);
    c.is_control()
        || matches!(c, '\u{2028}' | '\u{2029}')
        || ESCAPED.get(i).is_some_and(|r| r.0 <= u)
}

/// `s` with every [`hostile`] character as `\u{hex}` and a backslash as
/// `\\`, so an escape always reads back as one: `\u{1b}` written in a
/// file name prints as `\\u{1b}`, never as an escaped ESC.
pub fn escape(s: &str) -> String {
    escape_with(s, true)
}

fn escape_with(s: &str, backslash: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if hostile(c) {
            let _ = write!(out, "\\u{{{:x}}}", c as u32);
        } else if c == '\\' && backslash {
            out.push_str("\\\\");
        } else {
            out.push(c);
        }
    }
    out
}

/// `CamelCase` → `camel-case`, from the first identifier of `debug`.
fn kind_name(debug: &str) -> String {
    let mut out = String::new();
    for (i, c) in debug
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .enumerate()
    {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

impl Item {
    fn new(file: &str, line: usize, severity: Severity, kind: String, message: String) -> Self {
        Self {
            file: file.to_owned(),
            line,
            severity,
            kind,
            href: None,
            message,
            fix: None,
            hint: None,
            other_file: None,
            other_line: None,
            file_is_lossless: false,
        }
    }
}

/// Sorts `items` into the report's order: by file and line, then by every
/// other field, so the order never depends on the order findings were
/// made in.
pub fn sort(items: &mut [Item]) {
    let key = |i: &Item| {
        (
            i.file.clone(),
            i.line,
            i.kind.clone(),
            i.message.clone(),
            i.href.clone(),
            i.other_file.clone(),
            i.other_line,
            matches!(i.severity, Severity::Warning),
            i.fix.clone(),
            i.hint.clone(),
        )
    };
    items.sort_by_cached_key(key);
}

fn front_matter_error(kind: &FrontMatterErrorKind) -> String {
    match kind {
        FrontMatterErrorKind::Missing => "no front matter: the first line is not ---".into(),
        FrontMatterErrorKind::BareCarriageReturns => {
            "a bare carriage return in the front matter: Rubrica reads LF and CRLF line ends only"
                .into()
        }
        FrontMatterErrorKind::Unterminated => "the front matter has no closing ---".into(),
        FrontMatterErrorKind::Yaml(e) => format!("the front matter is not a YAML mapping: {e}"),
        FrontMatterErrorKind::NoTitle => "the front matter has no title".into(),
        FrontMatterErrorKind::BadValue { key, expected } => {
            format!("{key} is not {expected}")
        }
        FrontMatterErrorKind::TooLarge { bytes } => format!(
            "the front matter is {bytes} bytes, over Rubrica's limit of {}",
            rubrica_render::frontmatter::MAX_FRONT_MATTER_BYTES
        ),
        FrontMatterErrorKind::Anchor => "a YAML anchor (&…) in the front matter".into(),
        FrontMatterErrorKind::Alias => "a YAML alias (*…) in the front matter".into(),
        FrontMatterErrorKind::Tag => "an explicit YAML tag (!…) in the front matter".into(),
        FrontMatterErrorKind::TooDeep => format!(
            "the front matter nests deeper than {}",
            rubrica_render::frontmatter::MAX_FRONT_MATTER_DEPTH
        ),
    }
}

fn front_matter_finding(f: &FrontMatterFinding) -> String {
    match f {
        FrontMatterFinding::TooLongForWikijs { field, chars, .. } => {
            format!("{field} is {chars} chars (max 255): Wiki.js would drop the page")
        }
        FrontMatterFinding::PublishedIgnoredByWikijs { .. } => {
            "published: false is ignored by Wiki.js, which reads isPublished: Wiki.js publishes this page".into()
        }
        FrontMatterFinding::IsPublishedBreaksWikijsImport { .. } => {
            "isPublished is not a boolean to Wiki.js: its import of a new page fails".into()
        }
        FrontMatterFinding::TagListBreaksWikijsSync { .. } => {
            "tags is a YAML list: Wiki.js's import splits it as a string, and the page never syncs".into()
        }
        FrontMatterFinding::TagsSplitDifferentlyByWikijs { wikijs, .. } => {
            format!("Wiki.js reads these tags as: {}", wikijs.join(", "))
        }
        FrontMatterFinding::NoFrontMatterForWikijs { reason, .. } => {
            format!("Wiki.js sees no front matter here: {reason}")
        }
        FrontMatterFinding::WikijsClosesFrontMatterElsewhere { wikijs_line, .. } => {
            format!("Wiki.js ends the front matter at line {wikijs_line}, not here")
        }
    }
}

fn render_finding(f: &RenderFinding) -> String {
    match f {
        RenderFinding::SecWithoutHeading { id, .. } => format!(
            "the @sec marker \"{id}\" is not immediately followed by an ATX heading: its section has no anchor"
        ),
        RenderFinding::SecWithoutId { .. } => "an @sec marker with no id=".into(),
        RenderFinding::DuplicateSecId { id, .. } => format!(
            "@sec id \"{id}\" is used twice on this page: the second marker gets no anchor"
        ),
        RenderFinding::SecAnchorIsAHeadingId { id, .. } => format!(
            "the @sec anchor sec-{id} is also a heading id on this page: the marker gets no anchor"
        ),
        RenderFinding::SecInContainer { id, .. } => format!(
            "the @sec marker \"{id}\" is inside a blockquote, list item or footnote: it gets no anchor"
        ),
        RenderFinding::SecOnEmptyHeading { id, .. } => format!(
            "the @sec marker \"{id}\" is before a heading with no visible text: it gets no anchor"
        ),
        RenderFinding::HeadingIdIsAFootnoteId { id, .. } => {
            format!("heading id \"{id}\" is also a footnote's: a link to it reaches the first")
        }
        RenderFinding::FootnoteBackrefsBounded {
            name, references, ..
        } => format!(
            "footnote \"{name}\" is referenced {references} times: its definition links back to the first ones only"
        ),
        RenderFinding::RawHtmlTooDeep { .. } => {
            "raw HTML nested deeper than 100: the deeper tags are dropped".into()
        }
        RenderFinding::RawHtmlTooManyAttributes { .. } => {
            "a raw HTML tag with too many attributes: written as text".into()
        }
        RenderFinding::AttributesTooCostly {
            element, attribute, ..
        } => format!("the {attribute} of a <{element}> is left out: too costly to write"),
        RenderFinding::SecNotRendered { id, why, .. } => format!(
            "the @sec marker \"{id}\" is no marker to Rubrica ({}): it gets no anchor",
            match why {
                SecHidden::Code => "it is in code",
                SecHidden::HtmlBlock => "it is part of another HTML block",
                SecHidden::Text => "it is not an HTML block of its own",
                _ => "it is not read as one",
            }
        ),
        RenderFinding::SecNotSeenByCheckPackages { id, .. } => format!(
            "the @sec marker \"{id}\" is not seen by check-packages.cjs: it gets no anchor"
        ),
        RenderFinding::InvisibleHeading { .. } => {
            "a heading whose text is only invisible characters".into()
        }
        RenderFinding::SecInFrontMatter { .. } => {
            "check-packages.cjs harvests an @sec marker from the front matter".into()
        }
        RenderFinding::PageTooLarge { bytes } => format!(
            "the page body is {bytes} bytes, over Rubrica's limit of {MAX_PAGE_BYTES}: it is not rendered"
        ),
        RenderFinding::CodeBlockNotHighlighted {
            bytes,
            longest_line,
            block_cap,
            line_cap,
            page_budget,
            ..
        } => match page_budget {
            Some(budget) => format!(
                "code block ({bytes} bytes, longest line {longest_line}) is over the page's highlighting budget ({} MiB) and is written plain",
                budget >> 20
            ),
            None => format!(
            "code block ({bytes} bytes, longest line {longest_line}) is over the highlighting cap ({block_cap} bytes per block, {line_cap} bytes per line) and is written plain"
            ),
        },
        other => format!("{other:?}"),
    }
}

fn kind_of_file(kind: FileKind) -> &'static str {
    match kind {
        FileKind::Regular => "a regular file",
        FileKind::Symlink => "a symbolic link",
        FileKind::Submodule => "a submodule or nested checkout",
        FileKind::Other => "a special file (a FIFO, a socket or a device)",
        FileKind::TooLarge => "a file over the size cap",
        FileKind::UnknownMode => {
            "an entry of an unknown mode (no file, link, submodule or directory)"
        }
    }
}

/// The item for `f`, run as `opts` says.
pub fn describe(f: &Finding, site: &Site, opts: CheckOptions) -> Item {
    let sev = f.severity(opts);
    let kind = match f {
        Finding::Render { finding, .. } => kind_name(&format!("{finding:?}")),
        Finding::FrontMatter { finding, .. } => kind_name(&format!("{finding:?}")),
        other => kind_name(&format!("{other:?}")),
    };
    let mut item = Item::new(f.file(), f.line(), sev, kind, String::new());
    item.fix = f.fix();
    item.message = match f {
        Finding::BrokenPageLink { href, path, why, .. } => {
            item.href = Some(href.clone());
            match why {
                NoPage::Missing => format!("there is no page /{path} in this repo"),
                NoPage::Unpublished => format!("/{path} is not published (published: false)"),
                NoPage::BadFrontMatter => {
                    format!("/{path} is not rendered: its front matter does not parse")
                }
                NoPage::NotUtf8 => format!("/{path} is not rendered: its file is not UTF-8"),
                NoPage::Reserved(r) => format!("/{path} is no page: {r}"),
                NoPage::Invalid(e) => format!("{path} can be no page's path: {e}"),
            }
        }
        Finding::BrokenAnchor {
            href,
            to,
            fragment,
            why,
            suggestion,
            ..
        } => {
            item.href = Some(href.clone());
            let headings = site.page(to).map(|p| p.rendered.headings.len());
            match why {
                AnchorProblem::NoSuchId => {
                    item.hint = Some(match (suggestion, headings) {
                        (Some(s), _) => format!("heading is \"{}\" (line {})", s.text, s.line),
                        (None, Some(n)) => {
                            format!("{n} heading(s) on that page; none is a close match")
                        }
                        (None, None) => "no heading on that page is a close match".into(),
                    });
                    format!("no heading on {} has id \"{fragment}\"", to.url())
                }
                AnchorProblem::DroppedId { text } => {
                    item.hint =
                        Some("rename the heading (two words is enough) and update this link".into());
                    format!(
                        "\"{text}\" slugs to \"{fragment}\", which DOMPurify deletes (it clobbers a document or form property): the heading has no id"
                    )
                }
                AnchorProblem::LineCitation { n, drop_fragment } => {
                    item.hint = Some(match (drop_fragment, suggestion) {
                        (true, _) => format!(
                            "line {n} of the target is above its first section: drop the fragment"
                        ),
                        (false, Some(s)) => {
                            format!("line {n} of the target falls under \"{}\"", s.text)
                        }
                        (false, None) => {
                            format!("line {n} of the target falls under a heading with no id")
                        }
                    });
                    format!(
                        "\"#{fragment}\" is a source-line citation, not a heading id: nothing renders with id=\"{fragment}\""
                    )
                }
            }
        }
        Finding::RelativeLink {
            href, reaches, to, ..
        } => {
            item.href = Some(href.clone());
            item.hint = Some(match to {
                Some(_) => "make internal links absolute, with a leading \"/\"".into(),
                None => format!("{reaches} is no published page, so no absolute link is suggested"),
            });
            format!(
                "a relative link to {reaches}: Wiki.js resolves it against the page as a directory, a browser against the page's directory"
            )
        }
        Finding::BrokenAssetLink { href, .. } => {
            item.href = Some(href.clone());
            "the file it names is not in this repo".into()
        }
        Finding::DuplicateSecId {
            id,
            first_file,
            first_line,
            ..
        } => {
            item.other_file = Some(first_file.to_string());
            item.other_line = Some(*first_line);
            format!("@sec id \"{id}\" is already used at {first_file}:{first_line}")
        }
        Finding::BadFrontMatter { error, .. } => front_matter_error(&error.kind),
        Finding::FrontMatter { finding, .. } => front_matter_finding(finding),
        Finding::Render { finding, .. } => render_finding(finding),
        Finding::DuplicateUrl { other, .. } => {
            item.other_file = Some(other.to_string());
            format!(
                "{other} is the same name on a case- or normalisation-insensitive disk (full case folding over NFD), or the same path"
            )
        }
        Finding::DirectoryFileConflict { other, .. } => {
            item.other_file = Some(other.to_string());
            format!(
                "it is one name with {other} on a case- or normalisation-insensitive disk: a file and a directory, or one directory spelled two ways"
            )
        }
        Finding::ReservedPagePath { why, .. } => format!("a reserved page path: {why}"),
        Finding::IndexPageWithoutSection { file } => {
            let dir = file.strip_suffix("/index.md").unwrap_or(file);
            format!(
                "an index page with no section page {dir}.md: the static server would answer /{dir} with it"
            )
        }
        Finding::PathTooLong {
            bytes, segments, ..
        } => format!(
            "the path is {bytes} bytes and {segments} segments, over the limits ({MAX_PATH_BYTES} bytes, {MAX_PATH_SEGMENTS} segments): nothing under it is read"
        ),
        Finding::BadSourcePath { why, .. } => format!("no page path: {why}"),
        Finding::NotARegularFile {
            kind,
            directory: false,
            ..
        } => {
            format!("{}: never read, nor what it points at", kind_of_file(*kind))
        }
        Finding::NotARegularFile {
            kind,
            directory: true,
            ..
        } => format!(
            "{} where a directory of pages or assets could be: never followed, so nothing under it is read",
            kind_of_file(*kind)
        ),
        Finding::FileTooLarge { .. } => {
            format!("larger than Rubrica's limit of {MAX_FILE_BYTES} bytes: not read")
        }
        Finding::NotUtf8 { .. } => "the file is not UTF-8".into(),
        Finding::MdUnderAssets { .. } => {
            "a .md file under assets/, which holds uploaded files only".into()
        }
        Finding::PathTooLongForWikijs { chars, .. } => {
            format!("path is {chars} chars (max 255): Wiki.js would drop the page")
        }
        Finding::PathDiffersInWikijs { why, .. } => match why {
            WikijsPathProblem::ImportedAs(p) => format!(
                "Wiki.js imports this file as page {} in locale {}",
                p.path, p.locale
            ),
            WikijsPathProblem::Route => {
                "one of Wiki.js's own routes answers this page's URL first: Wiki.js never shows it"
                    .into()
            }
            WikijsPathProblem::EditorReserved => {
                "its first segment is reserved in Wiki.js: its editor refuses to open the page"
                    .into()
            }
        },
        Finding::RedirectTargetMissing { target, .. } => {
            format!("redirect_to: {target} names no page")
        }
        Finding::RedirectChain { target, .. } => {
            format!("redirect_to: names {}, itself a redirect stub (or this page)", target.url())
        }
        Finding::LinkToRedirectStub { href, stub, .. } => {
            item.href = Some(href.clone());
            format!("{} is a redirect stub", stub.url())
        }
        Finding::RepositoryOverBudget {
            what,
            at_least,
            limit,
        } => {
            let more = |what: &str| {
                format!(
                    "the repository has more than {limit} {what} (at least {at_least}): nothing else of it is checked"
                )
            };
            match what {
                Budget::Files => more("entries (files, links and submodules; from git, directories and names that are not UTF-8 too)"),
                Budget::PathBytes => more("bytes of paths"),
                Budget::Bytes => more("bytes of file content"),
                Budget::TreeBytes => more("bytes in one tree object"),
                Budget::CommitBytes => more("bytes in one commit or tag object"),
                Budget::ObjectBytes => more("bytes of git objects read"),
                // one object, named.
                Budget::DeltaBase { object } => format!(
                    "{object} cannot be rebuilt within the memory limit of {limit} bytes (too large, or the pack is damaged): nothing else of the repository is checked. Run `git fsck` to rule out damage. If it is clean, run `git config core.bigFileThreshold 16m && git repack -adf` in the repository the build clones from (for CI, the repository CI clones: a fresh clone inherits the delta); it repacks with no object over 16 MiB as a delta base, and later repacks keep to that. `-a -d` drops unreachable packed objects"
                ),
                Budget::DeltaDepth { object } => format!(
                    "{object}'s delta chain is {at_least} deep, deeper than git writes ({limit}): nothing else of the repository is checked"
                ),
            }
        }
        Finding::AssetTooLarge { bytes, .. } => {
            format!("the asset is {bytes} bytes, over the limit of {MAX_ASSET_BYTES}")
        }
        Finding::AssetBudget { bytes } => format!(
            "the assets total {bytes} bytes: over {ASSET_BUDGET_WARN} is a warning, over {ASSET_BUDGET_ERROR} an error"
        ),
        Finding::SuggestionsBounded { .. } => {
            "the anchor suggestion search ran out of budget here: some dead anchors have no suggestion"
                .into()
        }
        other => format!("{other:?}"),
    };
    item
}

/// The item for a path the reader could not hand to the site.
pub fn describe_problem(p: &ReadProblem) -> Item {
    match p {
        ReadProblem::PathNotUtf8 { path } => {
            let mut item = Item::new(
                path,
                1,
                Severity::Error,
                "path-not-utf8".into(),
                "the name is not UTF-8: neither it nor anything under it is read".into(),
            );
            item.file_is_lossless = true;
            item
        }
    }
}

/// Whether the site is over a repository budget: then it has no pages
/// because nothing of it was looked at, which [`no_pages`] would misname.
pub fn over_budget(site: &Site) -> bool {
    site.findings()
        .iter()
        .any(|f| matches!(f, Finding::RepositoryOverBudget { .. }))
}

/// The item for a repository with no published page: a wrong directory, or a checkout that failed, is not a pass.
pub fn no_pages() -> Item {
    Item::new(
        ".",
        1,
        Severity::Error,
        "no-pages".into(),
        "no published page in this directory: is it the wiki's checkout?".into(),
    )
}

/// The item for a site with pages but no home page: `/`
/// would be a 404.
pub fn no_home_page() -> Item {
    Item::new(
        ".",
        1,
        Severity::Error,
        "no-home-page".into(),
        "no published home page (home.md): the site would have no index.html, and / would be a 404"
            .into(),
    )
}

/// The item for a site whose output would be over `limit` bytes.
pub fn output_over_budget(limit: u64) -> Item {
    // About the output, not a file of the repository: no location.
    Item::new(
        "",
        0,
        Severity::Error,
        "output-over-budget".into(),
        format!("the built site would be more than {limit} bytes: nothing is written"),
    )
}

/// The text report.
pub fn text(r: &Report) -> String {
    let out = std::cell::RefCell::new(String::new());
    let line = |s: &str| {
        let mut o = out.borrow_mut();
        o.push_str(&escape(s));
        o.push('\n');
    };
    if r.findings.is_empty() {
        line(&format!(
            "rubrica check: OK — {} page(s), {} link(s); no findings.",
            r.pages, r.links
        ));
    } else {
        line(&format!(
            "rubrica check: {} finding(s) in {} page(s), {} link(s): {} error(s), {} warning(s)",
            r.findings.len(),
            r.pages,
            r.links,
            r.errors,
            r.warnings
        ));
        line("");
        out.borrow_mut().push_str(&items_text(&r.findings));
    }
    if r.same_page_unchecked > 0 || !r.findings.is_empty() {
        line("");
    }
    if r.same_page_unchecked > 0 {
        line(&format!(
            "NOTE — {} dead same-page ](#anchor) link(s) not reported; run with --same-page to include them.",
            r.same_page_unchecked
        ));
    }
    if r.failed {
        let strict = if r.errors == 0 { " (--strict)" } else { "" };
        line(&format!(
            "rubrica check FAILED{strict}: {} error(s), {} warning(s).",
            r.errors, r.warnings
        ));
    } else if r.warnings > 0 {
        line(&format!(
            "rubrica check: OK with {} warning(s); --strict fails on them.",
            r.warnings
        ));
    }
    out.into_inner()
}

/// Each item's lines, escaped: `  path:line: severity kind`, then the href,
/// the message, the fix and the hint.
fn items_text(items: &[Item]) -> String {
    let mut out = String::new();
    let mut line = |s: &str| {
        out.push_str(s);
        out.push('\n');
    };
    for f in items {
        let sev = match f.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        // The reader's path is already escaped (`lossless_display`):
        // its backslashes are not doubled again.
        let file = escape_with(&f.file, !f.file_is_lossless);
        if f.file.is_empty() {
            // About no file (the output's budget): no `file:line:`.
            line(&format!("  {sev} {}", escape(&f.kind)));
        } else {
            line(&format!("  {file}:{}: {sev} {}", f.line, escape(&f.kind)));
        }
        if let Some(href) = &f.href {
            line(&escape(&format!("    ]({href})")));
        }
        line(&escape(&format!("    {}", f.message)));
        if let Some(fix) = &f.fix {
            line(&escape(&format!("    fix: ]({fix})")));
        }
        if let Some(hint) = &f.hint {
            line(&escape(&format!("    ({hint})")));
        }
    }
    out
}

/// `rubrica build`'s report when it refuses: the findings it cannot build
/// a site with, as `check` prints them, and a verdict.
pub fn refusal_text(items: &[Item]) -> String {
    let mut out = format!(
        "rubrica build: refused: {} finding(s) a site cannot be built with (files and paths that would collide, or are not what a page needs):\n\n",
        items.len()
    );
    out.push_str(&items_text(items));
    out.push_str("\nrubrica build FAILED: nothing was written.\n");
    out
}

/// The JSON report, pretty-printed, with a final newline.
pub fn json(r: &Report) -> String {
    let mut s = serde_json::to_string_pretty(r).expect("a report serializes");
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-written set (the bidi controls and the invisible format
    /// characters), which the generated table must still hold.
    fn pr5b(c: char) -> bool {
        matches!(c,
            '\u{61c}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{feff}'
            | '\u{e0000}'..='\u{e007f}')
    }

    #[test]
    fn the_escape_table_is_unicode_17s_cf_and_default_ignorables() {
        assert_eq!(crate::escape_table::UNICODE_VERSION, "17.0.0");
        // Sorted, disjoint and not adjacent (else a range was split).
        assert!(ESCAPED.iter().all(|&(lo, hi)| lo <= hi));
        assert!(ESCAPED.windows(2).all(|w| w[0].1 + 1 < w[1].0));
        // 170 Cf and 4,174 Default_Ignorable_Code_Point, 138 in both.
        let n: u32 = ESCAPED.iter().map(|&(lo, hi)| hi - lo + 1).sum();
        assert_eq!((ESCAPED.len(), n), (25, 4206));
    }

    #[test]
    fn every_hostile_character_is_escaped_and_nothing_else() {
        for c in [
            '\0',
            '\u{7}',
            '\t',
            '\n',
            '\r',
            '\u{1b}',
            '\u{7f}',
            '\u{80}',
            '\u{85}',
            '\u{9f}',
            '\u{61c}',
            '\u{200e}',
            '\u{200f}',
            '\u{202a}',
            '\u{202e}',
            '\u{2066}',
            '\u{2069}',
            '\u{2028}',
            '\u{2029}',
            '\u{200b}',
            '\u{200c}',
            '\u{200d}',
            '\u{2060}',
            '\u{2064}',
            '\u{206a}',
            '\u{206f}',
            '\u{feff}',
            '\u{e0000}',
            '\u{e0041}',
            '\u{e007f}',
            // Every other Cf and Default_Ignorable_Code_Point: the soft hyphen, the Arabic number signs, the
            // Mongolian and the other variation selectors, the combining
            // grapheme joiner, the Hangul fillers, the Khmer inherent vowels,
            // U+2065, the interlinear annotation marks, the Egyptian and the
            // musical format controls, the shorthand format controls, and
            // the rest of plane 14.
            '\u{ad}',
            '\u{34f}',
            '\u{600}',
            '\u{6dd}',
            '\u{115f}',
            '\u{1160}',
            '\u{17b4}',
            '\u{180b}',
            '\u{180e}',
            '\u{180f}',
            '\u{2065}',
            '\u{3164}',
            '\u{fe00}',
            '\u{fe0f}',
            '\u{ffa0}',
            '\u{fff9}',
            '\u{110bd}',
            '\u{13430}',
            '\u{1bca0}',
            '\u{1d173}',
            '\u{e0080}',
            '\u{e0100}',
            '\u{e01ef}',
            '\u{e0fff}',
        ] {
            assert_eq!(
                escape(&c.to_string()),
                format!("\\u{{{:x}}}", c as u32),
                "{c:?}"
            );
        }
        for c in [
            ' ',
            '~',
            '\u{a0}',
            'é',
            '\u{2800}',
            '\u{fe10}',
            '\u{fffc}',
            '\u{1f600}',
            '\u{e1000}',
        ] {
            assert_eq!(escape(&c.to_string()), c.to_string(), "{c:?}");
        }
        // A backslash is doubled: `\u{1b}` written in a name reads as
        // `\\u{1b}`, never as an escaped ESC.
        assert_eq!(escape("\\"), "\\\\");
        assert_eq!(escape("a\\u{1b}"), "a\\\\u{1b}");
        assert_ne!(escape("\\u{1b}"), escape("\u{1b}"));
        // Every code point: escaped exactly when a control (Cc), a line or
        // paragraph separator, in the table or a backslash; the hand-written set is
        // still escaped; an escape is printable ASCII, and nothing hostile
        // is ever printed.
        for c in (0..=0x10ffff).filter_map(char::from_u32) {
            let e = escape(&c.to_string());
            let in_table = ESCAPED
                .iter()
                .any(|&(lo, hi)| (lo..=hi).contains(&(c as u32)));
            let line_sep = matches!(c, '\u{2028}' | '\u{2029}');
            assert_eq!(
                e != c.to_string(),
                c.is_control() || line_sep || in_table || c == '\\',
                "{c:?}"
            );
            assert!(!pr5b(c) || e != c.to_string(), "{c:?}");
            assert!(
                e == c.to_string() || e.chars().all(|x| x.is_ascii_graphic()),
                "{c:?}"
            );
            assert!(e.chars().all(|x| !hostile(x)), "{c:?}");
        }
    }

    #[test]
    fn a_lossless_path_is_not_escaped_twice() {
        // The reader's `path-not-utf8` file is already escaped
        // (`lossless_display`: `\xNN`, and `\\`): printed, its backslashes
        // stay single, so the byte 0xfe and a name holding `\xfe` differ.
        let mut a = Item::new(
            "d\\xfe",
            1,
            Severity::Error,
            "path-not-utf8".into(),
            "m".into(),
        );
        a.file_is_lossless = true;
        let b = Item::new(
            "d\\xfe",
            1,
            Severity::Error,
            "bad-source-path".into(),
            "m".into(),
        );
        let r = |f: Item| Report {
            format: 1,
            options: Options {
                strict: false,
                same_page: false,
                wikijs_limits: false,
            },
            pages: 1,
            links: 0,
            errors: 1,
            warnings: 0,
            same_page_unchecked: 0,
            failed: true,
            findings: vec![f],
        };
        assert!(text(&r(a)).contains("\n  d\\xfe:1: error path-not-utf8\n"));
        assert!(text(&r(b)).contains("\n  d\\\\xfe:1: error bad-source-path\n"));
    }

    #[test]
    fn items_sort_by_every_field() {
        // Two findings at one file and line sort the same whatever order
        // they come in.
        let mk = |kind: &str, msg: &str, href: Option<&str>| {
            let mut i = Item::new("a.md", 3, Severity::Warning, kind.into(), msg.into());
            i.href = href.map(str::to_owned);
            i
        };
        let items = vec![
            mk("broken-anchor", "no heading has id \"y\"", Some("#y")),
            mk("broken-anchor", "no heading has id \"x\"", Some("#x")),
            mk("broken-anchor", "no heading has id \"x\"", Some("/b#x")),
            mk("relative-link", "a", Some("b")),
        ];
        let mut fwd = items.clone();
        let mut rev: Vec<Item> = items.into_iter().rev().collect();
        sort(&mut fwd);
        sort(&mut rev);
        let show = |v: &[Item]| format!("{v:?}");
        assert_eq!(show(&fwd), show(&rev));
        assert_eq!(fwd[0].href.as_deref(), Some("#x"));
    }

    #[test]
    fn kinds_are_kebab_case_variant_names() {
        assert_eq!(kind_name("BrokenAnchor { file: .. }"), "broken-anchor");
        assert_eq!(kind_name("NotUtf8 { file: \"x\" }"), "not-utf8");
        assert_eq!(
            kind_name("PathTooLongForWikijs"),
            "path-too-long-for-wikijs"
        );
        assert_eq!(kind_name("SecWithoutId { line: 3 }"), "sec-without-id");
    }
}
