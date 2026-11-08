//! Markdown to HTML: the event parser ([`rubrica_markdown`]), the adapters and the
//! writer ([`crate::writer`]), configured by profile ([`Profile`]), plus everything the
//! site needs from the same parse: headings with their ids and `@sec` anchors, links,
//! task items, Mermaid sources, text.

use std::borrow::Cow;

use crate::heading::Heading;
use crate::sanitize::clean;
use crate::sec::{check_packages_markers, page_markers, RenderFinding, SecMarker};

/// How to render: [`RenderOptions::native`] (the default), adjusted
/// with the builder methods.
///
/// ```
/// use rubrica_render::{render, RawHtml, RenderOptions};
/// let opts = RenderOptions::wikijs_compat().raw_html(RawHtml::Escape);
/// assert_eq!(render("a <b>b</b>\n", &opts).html, "<p>a &lt;b&gt;b&lt;/b&gt;</p>\n");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderOptions {
    raw_html: RawHtml,
    hard_breaks: bool,
    profile: Profile,
    highlighter: Option<Highlighter>,
}

/// What a highlighter says about one fenced block. `Html` is written as the block's body
/// (inside `<pre><code class="language-L">`); `Plain` means "escape the text as usual, no
/// finding" (an unknown language); `OverCap` means "escape the text as usual and report
/// it": the writer records it and [`render`] turns it into
/// [`RenderFinding::CodeBlockNotHighlighted`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Highlight {
    /// The block's body, already escaped; it still goes through the final sanitizer pass.
    Html(String),
    /// Written as escaped text, nothing reported.
    Plain,
    /// Written as escaped text, and reported: the block is over one of the highlighter's caps.
    OverCap {
        /// The block's length in bytes, its trailing newline trimmed.
        bytes: usize,
        /// The longest line's length in bytes, its line end not counted.
        longest_line: usize,
        /// The most bytes a block may have.
        block_cap: usize,
        /// The most bytes a line may have.
        line_cap: usize,
    },
}

/// A build-time highlighter: a plain `fn` in a newtype. `RenderOptions` derives `PartialEq`
/// and `Eq`, and a bare `Option<fn>` field fails clippy's
/// `unpredictable_function_pointer_comparisons` inside that derive, so the newtype
/// compares the function addresses explicitly. The wasm preview passes `None`.
/// The HTML returned goes through the final sanitizer pass like everything else.
#[derive(Debug, Clone, Copy)]
pub struct Highlighter(pub fn(lang: &str, code: &str) -> Highlight);

impl PartialEq for Highlighter {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::fn_addr_eq(self.0, other.0)
    }
}

impl Eq for Highlighter {}

/// Which rules render the page: Wiki.js 2's ([`Profile::WikijsCompat`]) or
/// Rubrica's own ([`Profile::Native`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Rubrica's own rules: CommonMark's HTML, GFM task lists.
    Native,
    /// Wiki.js 2's rules.
    WikijsCompat,
}

/// What happens to raw HTML in the Markdown (a block or inline `<…>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RawHtml {
    /// Filtered to [`crate::sanitize::RAW_TAGS`] with no attributes (but
    /// `open` on `details`); other tags dropped and their text kept, as
    /// Wiki.js's `html: true` and DOMPurify do (Rubrica keeps fewer tags).
    Sanitize,
    /// Written as text, escaped (`&lt;b&gt;`).
    Escape,
}

impl RenderOptions {
    /// Wiki.js 2.5.314's markdown-it, where the corpus can tell: raw HTML on
    /// and sanitized ([`RawHtml::Sanitize`]), soft line breaks as `<br>`
    /// (`breaks: true`).
    pub fn wikijs_compat() -> Self {
        Self {
            raw_html: RawHtml::Sanitize,
            hard_breaks: true,
            profile: Profile::WikijsCompat,
            highlighter: None,
        }
    }

    /// Rubrica's own rules ([`Profile::Native`]): raw HTML sanitized, soft line breaks
    /// as newlines. The [`Default`].
    pub fn native() -> Self {
        Self {
            raw_html: RawHtml::Sanitize,
            hard_breaks: false,
            profile: Profile::Native,
            highlighter: None,
        }
    }

    /// What to do with raw HTML.
    pub fn raw_html(mut self, raw_html: RawHtml) -> Self {
        self.raw_html = raw_html;
        self
    }

    /// Whether a soft line break is a `<br>` (Wiki.js: yes) or a newline.
    pub fn hard_breaks(mut self, hard_breaks: bool) -> Self {
        self.hard_breaks = hard_breaks;
        self
    }

    /// The build-time highlighter for fenced code with a language (none by default).
    pub fn highlighter(mut self, highlighter: Option<Highlighter>) -> Self {
        self.highlighter = highlighter;
        self
    }
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self::native()
    }
}

/// A rendered page body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// The body HTML, sanitized ([`crate::sanitize`]).
    pub html: String,
    /// Every heading, in document order.
    pub headings: Vec<Heading>,
    /// Every link (`<a href>`), in document order. Images are not links.
    pub links: Vec<Link>,
    /// Every `@sec` marker with an id, in document order, attached or not
    /// (the ones on a heading are also its [`Heading::sec`]).
    pub secs: Vec<SecMarker>,
    /// What is wrong with the page: its `@sec` markers, footnotes, raw HTML
    /// and the bounds that kept its render linear; by line, in document
    /// order within one.
    pub findings: Vec<RenderFinding>,
    /// Every task-list item, in document order.
    pub task_items: Vec<TaskItem>,
    /// The source of every ```` ```mermaid ```` fence, in document order.
    pub mermaid_blocks: Vec<String>,
    /// Words in [`Rendered::plain_text`].
    pub word_count: usize,
    /// The text of the page without markup (search input).
    pub plain_text: String,
}

/// A link as written (after entity and escape resolution).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// The destination.
    pub href: String,
    /// 1-based source line (of the body passed to [`render`]).
    pub line: usize,
    /// What kind of destination it is.
    pub kind: LinkKind,
}

/// The kinds of link destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkKind {
    /// `/path` or `/path#fragment`: another page of this wiki.
    Internal {
        /// The path, with its leading `/`, without query or fragment.
        path: String,
        /// The fragment, without `#`.
        fragment: Option<String>,
    },
    /// `#fragment`: this page.
    Anchor(String),
    /// Anything with a scheme (`https:`, `mailto:`) or protocol-relative.
    External,
    /// Anything else: a relative path, which Wiki.js resolves wrongly.
    Relative,
}

/// A GFM task-list item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskItem {
    /// 1-based source line.
    pub line: usize,
    /// `[x]`.
    pub checked: bool,
    /// Index into [`Rendered::headings`] of the heading the item sits under.
    pub section: Option<usize>,
}

/// Classify a link destination. Fragments are percent-decoded, as a
/// browser decodes them before matching an id (`#caf%C3%A9` is `café`).
pub fn classify(href: &str) -> LinkKind {
    if let Some(fragment) = href.strip_prefix('#') {
        return LinkKind::Anchor(percent_decode(fragment));
    }
    if href.starts_with("//") || has_scheme(href) {
        return LinkKind::External;
    }
    if href.starts_with('/') {
        let (rest, fragment) = match href.split_once('#') {
            Some((r, f)) => (r, Some(percent_decode(f))),
            None => (href, None),
        };
        let path = rest.split_once('?').map_or(rest, |(p, _)| p);
        return LinkKind::Internal {
            path: path.to_owned(),
            fragment,
        };
    }
    LinkKind::Relative
}

/// `%XX` escapes decoded, when the result is UTF-8; otherwise (and for a
/// `%` that starts no escape) the text as written.
fn percent_decode(s: &str) -> String {
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
    String::from_utf8(out).unwrap_or_else(|_| s.to_owned())
}

/// RFC 3986 `scheme ":"`.
fn has_scheme(href: &str) -> bool {
    let Some((scheme, _)) = href.split_once(':') else {
        return false;
    };
    let mut bytes = scheme.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
}

/// The longest page body [`render`] renders: 4 MiB. A longer one is not
/// rendered; a notice stands in for it and a [`RenderFinding::PageTooLarge`]
/// says why. A bound for the shapes nobody has found yet: every shape known
/// to cost more than linear time has its own bound, and 61
/// others were timed (cmark's pathological inputs, raw HTML, references, autolinks and
/// e-mail addresses, at 0.5, 1 and 2 MB): all linear. Just under it, in
/// release (`/usr/bin/time -l`), the slowest is an empty heading on
/// every line (`#⏎` 2,097,152 times): 2.6 s and 2.8 GB peak RSS; `# h⏎`,
/// 1.6 s and 1.8 GB; a paragraph, list item or table row a line, 1.0–1.2 s
/// and 1.1–1.5 GB; `&`, 1.0 s and 0.9 GB; text, 18 ms and 43 MB. At 1 MiB the
/// slowest is 0.61 s and 0.71 GB. The corpus's longest body is
/// 46 KB: the cap is 90 times that, and 6
/// times the whole corpus (664 KB). Whether `serve` keeps it is the owner's
/// call.
pub const MAX_PAGE_BYTES: usize = 4 << 20;

/// The most back references a footnote's definition gets ([`crate::writer`]'s bound,
/// re-exported where the tests name it).
pub use crate::writer::MAX_FOOTNOTE_BACKREFS;

/// The notice that stands in for a page over [`MAX_PAGE_BYTES`].
fn refused(bytes: usize) -> Rendered {
    Rendered {
        html: format!(
            "<p>This page is not rendered: its body is {bytes} bytes, more than Rubrica's limit of {MAX_PAGE_BYTES}.</p>\n"
        ),
        headings: Vec::new(),
        links: Vec::new(),
        secs: Vec::new(),
        findings: vec![RenderFinding::PageTooLarge { bytes }],
        task_items: Vec::new(),
        mermaid_blocks: Vec::new(),
        word_count: 0,
        plain_text: String::new(),
    }
}

/// Render a page body (the part after the front matter), assembled from the event
/// parser, the event adapters and the writer ([`crate::writer`]). The
/// profile ([`RenderOptions::native`] or [`RenderOptions::wikijs_compat`]) picks the
/// parser options, the adapters' rules and the writer's markup. Lines in the result are
/// lines of `body` as markdown-it counts them: each `\r\n`, `\r` or `\n` ends one (the
/// body is read with `\r\n?` as `\n`, as markdown-it reads it); they come from the
/// parser's own offsets.
pub fn render(body: &str, opts: &RenderOptions) -> Rendered {
    render_with(body, opts, false, None)
}

/// A page file rendered: its front matter, its body's [`Rendered`], and
/// the number of lines before the body (body line `n` is file line
/// `n + offset`), as [`crate::split_front_matter`] gives them.
///
/// The body renders as [`render`] renders it, but its `@sec` markers are
/// check-packages.cjs's harvest of the whole file, as check-packages reads
/// it ([`crate::sec`]): a match that starts in the front matter is a
/// [`RenderFinding::SecInFrontMatter`] at its line in the file (these come
/// first in [`Rendered::findings`]; every other finding's line is the
/// body's), and one left open there that runs on into the body takes the
/// body's marker with it.
pub fn render_page(
    src: &str,
    opts: &RenderOptions,
) -> Result<(crate::FrontMatter, Rendered, usize), crate::frontmatter::FrontMatterError> {
    let (front, body, offset) = crate::split_front_matter(src)?;
    let (harvest, in_front) = page_markers(src, offset);
    let mut out = render_with(body, opts, false, Some(harvest));
    out.findings.splice(0..0, in_front);
    Ok((front, out, offset))
}

/// [`render`], but a text whose source position the adapters got wrong panics
/// ([`crate::adapt::check_positions`]). For the integration tests only: no build of the
/// `rubrica` binary calls it, so strictness cannot reach one (a cargo feature would, by
/// unification under `--all-features`).
#[doc(hidden)]
pub fn render_strict(body: &str, opts: &RenderOptions) -> Rendered {
    render_with(body, opts, true, None)
}

/// The render; `harvest` is check-packages' harvest of `body` when the
/// caller has it ([`render_page`]), and otherwise it is taken from `body`.
fn render_with(
    body: &str,
    opts: &RenderOptions,
    strict: bool,
    harvest: Option<Vec<crate::sec::HarvestedMarker>>,
) -> Rendered {
    use crate::writer::{write, RawHtmlMode, WriterOptions};

    if body.len() > MAX_PAGE_BYTES {
        return refused(body.len());
    }
    let raw = body;
    let bare_cr = bare_cr_lines(body);
    let body = normalize_line_ends(body);
    // One leading BOM is skipped, as the earlier renderer skipped it; offsets and lines are the stripped
    // body's (the BOM is on line 1 either way), the harvest is of the body as given.
    let body = body.strip_prefix('\u{feff}').unwrap_or(&body);
    let table = crate::lines::LineTable::new(body);
    let harvest = harvest.unwrap_or_else(|| check_packages_markers(raw));
    let collected =
        crate::adapt::collect(body, &table, opts.profile, opts.raw_html, harvest, &bare_cr);
    if strict {
        if let Err((range, text)) = crate::adapt::check_positions(&collected.events, body) {
            panic!("position {range:?} does not hold {text:?}");
        }
    }
    let mut writer_opts = match opts.profile {
        Profile::Native => WriterOptions::NATIVE,
        Profile::WikijsCompat => WriterOptions::WIKIJS_COMPAT,
    };
    // The builder methods act on both profiles, as they do on `render`.
    writer_opts.soft_break_is_br = opts.hard_breaks;
    writer_opts.raw_html = match opts.raw_html {
        RawHtml::Sanitize => RawHtmlMode::Raw,
        RawHtml::Escape => RawHtmlMode::Escape,
    };
    writer_opts.highlighter = opts.highlighter;
    let crate::adapt::Collected {
        events,
        headings,
        heading_attrs,
        links,
        task_items,
        mermaid_blocks,
        secs,
        mut findings,
        plain_text,
    } = collected;
    let written = write(events, &heading_attrs, writer_opts);
    findings.extend(crate::adapt::footnote_findings(&written, &headings, &table));
    findings.extend(crate::adapt::not_highlighted_findings(&written, &table));
    let lines = written.positions_as_lines(&table);
    let (html, dropped) = clean(&written.html);
    findings.extend(dropped.into_iter().map(|d| {
        let at = lines.partition_point(|&(offset, _)| offset <= d.at);
        RenderFinding::AttributesTooCostly {
            element: d.element,
            attribute: d.attribute,
            line: at.checked_sub(1).map_or(1, |i| lines[i].1),
        }
    }));
    // In document order; a stable sort keeps a line's findings as found.
    findings.sort_by_key(RenderFinding::line);
    Rendered {
        html,
        word_count: plain_text.split_whitespace().count(),
        plain_text,
        headings,
        links,
        secs,
        findings,
        task_items,
        mermaid_blocks,
    }
}

/// The 1-based lines of `body` that end in a bare `\r` (no `\n` after it),
/// ascending: lines to markdown-it and the parser, not to `check-packages.cjs`,
/// which finds the next line after a `\n`.
fn bare_cr_lines(body: &str) -> Vec<usize> {
    if !body.contains('\r') {
        return Vec::new();
    }
    let b = body.as_bytes();
    let (mut out, mut line) = (Vec::new(), 1);
    for (i, &c) in b.iter().enumerate() {
        match c {
            b'\n' => line += 1,
            b'\r' if b.get(i + 1) != Some(&b'\n') => {
                out.push(line);
                line += 1;
            }
            _ => {}
        }
    }
    out
}

/// markdown-it's `normalize` rule: each `\r\n` and each bare `\r` becomes a
/// `\n`, before the parser sees the body: lines and columns in the normalised text are
/// lines and columns in `body`.
fn normalize_line_ends(body: &str) -> Cow<'_, str> {
    if !body.contains('\r') {
        return Cow::Borrowed(body);
    }
    Cow::Owned(body.replace("\r\n", "\n").replace('\r', "\n"))
}
