//! The HTML writer over [`rubrica_markdown`]'s events: CommonMark 0.31.2's HTML, byte
//! for byte, GFM's tables, task lists and strikethrough, footnotes, the wikijs-compat
//! markup behind [`WriterOptions`]'s flags, and the bounds (container depth, back
//! references, output size), with heading ids and `@sec` anchors from a list the caller
//! computed.
//!
//! The writer takes any `IntoIterator` of `(Event, Range)` pairs and collects it first:
//! a pre-pass over the vector finds the headings (so their list is read in
//! source order even where a footnote definition is written out of it), the lists that
//! hold a task item (`<ul class="contains-task-list">`) and the footnote definitions
//! (written after the body, in first-reference order).
//!
//! # The rules (CommonMark's HTML, [`WriterOptions::NATIVE`])
//!
//! - Text, code spans and code-block content escape `&`, `<`, `>` and `"` (`&amp;`,
//!   `&lt;`, `&gt;`, `&quot;`); `'` is not escaped in text. Attribute values escape the
//!   same set. An `href`/`src` percent-encodes every byte outside the URL-safe set, with
//!   `&` as `&amp;` and `'` as `&#x27;`.
//! - Every block tag ends with `\n`, and a block that opens where the last byte written is
//!   not a newline is preceded by one (cmark's `cr`): that is what gives a tight item
//!   `<li>text</li>` and a loose one `<li>\n<p>…</p>\n</li>` (the parser omits the
//!   `Paragraph` events of a tight item), and an HTML block that is the first child of a
//!   tight item its own line (`<li>\n<div>\n</li>`, spec example 175). An HTML block is
//!   followed by one too (`<div>` at EOF is `<div>\n`).
//! - A heading is `<hN>` or `<hN id="…">` from the caller's list, then
//!   `<span class="sec" id="…"></span>` when the entry has an anchor; the list is read in
//!   document order; a heading past the end of the list is plain.
//! - A fenced code block's class is `language-L`, `L` the info string's first word. The
//!   word ends at a space **or a tab** (upstream pulldown-cmark splits at a space only:
//!   Rubrica's deviation, recorded for `docs/spec/deviations.md`); a form feed or vertical
//!   tab inside the word stays in it under `NATIVE`. The parser has already dropped
//!   leading and trailing ASCII whitespace from the info string.
//! - Raw HTML (`Html`, `InlineHtml`) is written as it is. `SoftBreak` is `\n`, `HardBreak`
//!   is `<br />\n`, `Rule` is `<hr />\n`.
//! - A `LinkType::Email` destination gets `mailto:` when it has none (the parser's own
//!   `<a@b.c>` autolinks carry the bare address; every Email link out of
//!   [`rubrica_markdown::events`] already has the scheme, so the two paths agree).
//! - An image's `alt` is the text of its inner events, folded as the spec does (inline
//!   HTML and code as text, breaks as spaces, nested images by their text).
//! - Tables as the GFM spec writes them: every tag on its own line, `align="left|center|right"`
//!   on `<th>` and `<td>`, `<tbody>` opened at the first body row and closed only if opened.
//! - A task-list marker is `<input checked="" disabled="" type="checkbox">` (no `checked=""`
//!   when unchecked) and one space; the parser's `Text` after the marker does not carry
//!   that space. A marker followed by the item's `Paragraph` is written inside it.
//! - Strikethrough is `<del>`.
//! - Footnotes: a reference is
//!   `<sup class="footnote-ref"><a href="#fn-N" id="fnref-N[-k]" data-footnote-ref>N</a></sup>`,
//!   N the 1-based order of the footnote **name**'s first reference, k the reference's
//!   number for that name (omitted when 1); names match case-insensitively, as the parser
//!   matches labels (`UniCase`), and are reported in their first spelling. The definitions are written after the body as
//!   `<section class="footnotes" data-footnotes>\n<ol>\n<li id="fn-N">\n…</li>\n…</ol>\n</section>\n`
//!   in that order (the newline after `<li …>` is the first block's `cr`; an empty
//!   definition has its back references on that line), unreferenced ones dropped, the
//!   first definition of a name the one written (as for link reference definitions). Each
//!   definition's last paragraph gets the back references before its `</p>` (after the
//!   content when the last child is not a paragraph), one per reference, as the blessed pre-switch
//!   render has them (a space before each):
//!   `<a href="#fnref-N[-k]" class="footnote-backref" data-footnote-backref data-footnote-backref-idx="N[-k]" aria-label="Back to reference N[-k]">↩</a>`,
//!   with `<sup class="footnote-ref">k</sup>` after the arrow for k ≥ 2; at most
//!   [`MAX_FOOTNOTE_BACKREFS`] of them, the excess recorded in [`Written::backrefs_bounded`].
//!   The ids use the ordinal, not the name (the pre-switch render's were `fn-NAME`; the
//!   deviation is in `docs/spec/deviations.md`). Every id written is in
//!   [`Written::footnote_ids`].
//! - Containers (`BlockQuote`, `List`, `Item`, `FootnoteDefinition`) are counted; past
//!   [`MAX_CONTAINER_DEPTH`] their tags are not written and their content is; a tight
//!   task item there loses its checkbox with its tags (a loose item's opens its
//!   paragraph and stays), as today's formatter has it. A footnote
//!   definition past the bound is never written: its name goes to
//!   [`Written::unlinked_footnotes`] and its references are `<sup class="footnote-ref">N</sup>`,
//!   no link, no id.
//! - [`Written::positions`] gets `(html offset, source offset)` at every `Start`, `Html`,
//!   `InlineHtml`, `Code`, `Rule`, `TaskListMarker` and `FootnoteReference` whose source offset differs
//!   from the last pushed, sorted by html offset at the end (footnote definitions are
//!   written out of source order).
//! - Output past [`MAX_RENDER_OUTPUT_BYTES`] is replaced by a one-paragraph refusal
//!   notice naming the limit, with [`Written::truncated`] set and the positions, ids and
//!   footnote records cleared.
//!
//! # The compat flags ([`WriterOptions::WIKIJS_COMPAT`], today's `wikijs_formatter` markup)
//!
//! - `soft_break_is_br`: a soft break is `<br />\n`.
//! - `task_markup: MarkdownIt`: `<li class="task-list-item">`, the checkbox
//!   `<input class="task-list-item-checkbox"[ checked=""] disabled="" type="checkbox" />`
//!   and one space (inside the first `<p>` of a loose item), and `class="contains-task-list"`
//!   on a list holding a task item.
//! - `mermaid: PreClass`: a `mermaid` fence is `<pre class="mermaid">…</pre>\n`, the source
//!   escaped as text; `LanguageCode` writes `<pre><code class="language-mermaid">` like any
//!   language (both forms kept).
//! - `raw_html: Escape`: `Html` and `InlineHtml` are written as escaped text.
//! - `form_feed_ends_language`: the info string is first rewritten by today's rule (every
//!   `\x0C` a space, trimmed, then the first word), so `rust\x0Cx` is `rust`. A leading
//!   form feed never reaches the writer: the parser trims it with the other whitespace.
//!
//! Events outside Rubrica's option set (math, metadata blocks, definition lists,
//! superscript, subscript, wikilinks) are unreachable from [`rubrica_markdown::events`] with
//! [`rubrica_markdown::GFM`] and are a panic, never dropped.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::mem;
use std::ops::Range;

use rubrica_markdown::{Alignment, CodeBlockKind, Event, LinkType, Tag, TagEnd};
use unicase::UniCase;

use crate::markdown::{Highlight, Highlighter};

/// What the writer is told about each heading, in document order (the adapters compute them;
/// the spec tests pass an empty slice).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HeadingAttrs {
    /// The `id` attribute of the `<hN>`, if any.
    pub id: Option<String>,
    /// The `id` of the `<span class="sec">` written first inside the heading, if any.
    pub sec_anchor: Option<String>,
}

/// How a task-list item is written: GFM's `<input>` (native) or markdown-it's
/// `<li class="task-list-item">` (wikijs-compat).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskMarkup {
    /// `<input disabled="" type="checkbox">` (native).
    Gfm,
    /// markdown-it's task-list markup (wikijs-compat).
    MarkdownIt,
}

/// How a `mermaid` fence is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MermaidMarkup {
    /// `<pre class="mermaid">` (wikijs-compat).
    PreClass,
    /// `<pre><code class="language-mermaid">` (native).
    LanguageCode,
}

/// What happens to raw HTML in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawHtmlMode {
    /// Written as it is (the sanitizer's final pass filters it).
    Raw,
    /// Written as escaped text.
    Escape,
}

/// The writer's profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriterOptions {
    /// A soft line break is `<br>` (compat: true).
    pub soft_break_is_br: bool,
    /// The task-list markup.
    pub task_markup: TaskMarkup,
    /// The `mermaid` fence markup.
    pub mermaid: MermaidMarkup,
    /// Raw HTML, written or escaped.
    pub raw_html: RawHtmlMode,
    /// A form feed ends the language word of an info string (compat: true; today's
    /// `end_languages_at_form_feeds`).
    pub form_feed_ends_language: bool,
    /// The build-time highlighter for fenced code with a language (none in both presets).
    pub highlighter: Option<Highlighter>,
}

impl WriterOptions {
    /// Rubrica's own rules: CommonMark's HTML.
    pub const NATIVE: Self = Self {
        soft_break_is_br: false,
        task_markup: TaskMarkup::Gfm,
        mermaid: MermaidMarkup::LanguageCode,
        raw_html: RawHtmlMode::Raw,
        form_feed_ends_language: false,
        highlighter: None,
    };
    /// Wiki.js 2's rules.
    pub const WIKIJS_COMPAT: Self = Self {
        soft_break_is_br: true,
        task_markup: TaskMarkup::MarkdownIt,
        mermaid: MermaidMarkup::LanguageCode,
        raw_html: RawHtmlMode::Raw,
        form_feed_ends_language: true,
        highlighter: None,
    };
}

/// The most bytes `write` produces for one page: 64 MiB, 16 × MAX_PAGE_BYTES (4 MiB). The
/// footnote shape (`[^1] ` repeated) amplifies about 18× (measured: 0.5 MB of it is 9.55 MB
/// of HTML, 2 MB is 38.5 MB), so a page of that shape near the page cap passes this one
/// and is refused: its output is replaced by a refusal notice and `Written::truncated` is
/// set. Any other shape measured stays well under it.
pub const MAX_RENDER_OUTPUT_BYTES: usize = 64 << 20;
/// The deepest container nesting written; deeper containers are written as text.
pub const MAX_CONTAINER_DEPTH: usize = 100;
/// The most back-references one footnote gets.
pub const MAX_FOOTNOTE_BACKREFS: u32 = 100;

/// What replaces the HTML of a page whose output passed [`MAX_RENDER_OUTPUT_BYTES`].
const REFUSAL_NOTICE: &str = "<p>This page is not rendered: its HTML would be larger than Rubrica's limit of 67108864 bytes.</p>\n";
// The notice names the constant's value.
const _: () = assert!(MAX_RENDER_OUTPUT_BYTES == 67_108_864);

/// What `write` produced.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Written {
    /// The HTML.
    pub html: String,
    /// `(html_offset, source_offset)`: where each source node's HTML starts, sorted by
    /// html offset.
    pub positions: Vec<(usize, usize)>,
    /// Every `fn-…`/`fnref-…` id written, for HeadingIdIsAFootnoteId.
    pub footnote_ids: Vec<String>,
    /// `(name, references, source_offset)` of each footnote past the back-reference cap.
    pub backrefs_bounded: Vec<(String, usize, usize)>,
    /// The footnotes whose every definition is past [`MAX_CONTAINER_DEPTH`]: none is
    /// written, and their references carry no link.
    pub unlinked_footnotes: Vec<String>,
    /// The fenced blocks the highlighter refused as over a cap, written plain.
    pub not_highlighted: Vec<NotHighlighted>,
    /// The output passed [`MAX_RENDER_OUTPUT_BYTES`] and was replaced.
    pub truncated: bool,
}

/// One fenced block the highlighter refused as over a cap ([`crate::markdown::Highlight::OverCap`]), written
/// plain. `offset` is the block's source offset (the start event's `range.start`, like the
/// source offset of [`Written::backrefs_bounded`]): the writer has no line table, so the
/// adapter turns it into the finding's line. The other four fields are `OverCap`'s, copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotHighlighted {
    /// The block's source offset.
    pub offset: usize,
    /// The block's length in bytes, its trailing newline trimmed.
    pub bytes: usize,
    /// The longest line's length in bytes.
    pub longest_line: usize,
    /// The most bytes a block may have.
    pub block_cap: usize,
    /// The most bytes a line may have.
    pub line_cap: usize,
    /// `Some(budget)` when the block is plain because the page's budget was used (the caps
    /// are then 0).
    pub page_budget: Option<usize>,
}

/// The most highlighted HTML bytes one page may carry: once a block's highlighted output
/// would pass it, that block and every later one are written plain and reported.
pub const MAX_PAGE_HIGHLIGHT_BYTES: usize = 16 << 20;

impl Written {
    /// [`Written::positions`] as `(html_offset, line)`: each source offset through
    /// `table`, keeping the first of consecutive pairs on the same line (the shape the
    /// sanitizer's `partition_point` lookup reads).
    pub fn positions_as_lines(&self, table: &crate::lines::LineTable) -> Vec<(usize, usize)> {
        let mut lines: Vec<(usize, usize)> = Vec::with_capacity(self.positions.len());
        for &(html, src) in &self.positions {
            let line = table.line(src);
            if lines.last().is_none_or(|&(_, last)| last != line) {
                lines.push((html, line));
            }
        }
        lines
    }
}

/// Writes `events` as HTML. `headings` holds one entry per heading in document order;
/// `events` is any `IntoIterator` (the writer needs the whole vector for the
/// task-list and footnote pre-pass; it is collected first).
pub fn write<'a>(
    events: impl IntoIterator<Item = (Event<'a>, Range<usize>)>,
    headings: &[HeadingAttrs],
    opts: WriterOptions,
) -> Written {
    write_bounded(events, headings, opts, MAX_RENDER_OUTPUT_BYTES)
}

/// [`write`] with the output cap as a parameter, for the cap's tests. Public but hidden;
/// nothing outside `rubrica-render`'s tests may call it.
#[doc(hidden)]
pub fn write_bounded<'a>(
    events: impl IntoIterator<Item = (Event<'a>, Range<usize>)>,
    headings: &[HeadingAttrs],
    opts: WriterOptions,
    cap: usize,
) -> Written {
    let events: Vec<_> = events.into_iter().collect();
    let plan = Plan::new(&events);
    // Fewer entries than headings is allowed (the rest are plain); more is a bug.
    debug_assert!(
        headings.len() <= plan.headings.len(),
        "{} heading entries for {} headings",
        headings.len(),
        plan.headings.len()
    );
    let mut w = Writer {
        out: String::new(),
        total: 0,
        cap,
        truncated: false,
        positions: Vec::new(),
        last_src: None,
        depth: 0,
        headings,
        opts,
        plan: &plan,
        table: None,
        code_pre_class: false,
        ordinals: HashMap::new(),
        order: Vec::new(),
        ref_counts: Vec::new(),
        footnote_ids: Vec::new(),
        backrefs_bounded: Vec::new(),
        not_highlighted: Vec::new(),
        highlighted_bytes: 0,
        highlight_exhausted: false,
    };
    w.render(&events, 0, events.len());
    w.footnotes_section(&events);
    w.finish()
}

/// One footnote definition in the event vector.
struct Def {
    name: String,
    /// The index of its `Start`, and of its `End`.
    start: usize,
    end: usize,
    /// The `Start`'s source offset.
    src: usize,
    /// Past [`MAX_CONTAINER_DEPTH`] where it stands in the source.
    deep: bool,
}

/// What the pre-pass over the whole event vector knows.
struct Plan {
    /// The index of every `Start(Heading)`, ascending: a heading's place in the caller's
    /// list is its place here, whatever order it is written in.
    headings: Vec<usize>,
    /// Per event index: a `Start(List)` that directly holds a task item.
    task_lists: Vec<bool>,
    /// Every footnote definition, in source order.
    defs: Vec<Def>,
    /// The `End` index of each definition `Start`, to skip it in the body.
    def_end: HashMap<usize, usize>,
    /// Name (case-folded, as the parser matches labels) → index into `defs` of the first
    /// definition within the depth bound.
    chosen: HashMap<UniCase<String>, usize>,
    /// The names whose every definition is past the depth bound, in first-definition order
    /// and first spelling.
    unlinked: Vec<String>,
}

impl Plan {
    fn new(events: &[(Event<'_>, Range<usize>)]) -> Self {
        let mut plan = Plan {
            headings: Vec::new(),
            task_lists: vec![false; events.len()],
            defs: Vec::new(),
            def_end: HashMap::new(),
            chosen: HashMap::new(),
            unlinked: Vec::new(),
        };
        let mut depth = 0usize;
        let mut lists: Vec<usize> = Vec::new();
        let mut open_defs: Vec<usize> = Vec::new();
        for (i, (ev, range)) in events.iter().enumerate() {
            match ev {
                Event::Start(tag) => {
                    if is_container(tag) {
                        depth += 1;
                    }
                    match tag {
                        Tag::Heading { .. } => plan.headings.push(i),
                        Tag::List(_) => lists.push(i),
                        Tag::Item => {
                            if is_task_item(events, i) {
                                if let Some(&list) = lists.last() {
                                    plan.task_lists[list] = true;
                                }
                            }
                        }
                        Tag::FootnoteDefinition(name) => {
                            plan.defs.push(Def {
                                name: name.to_string(),
                                start: i,
                                end: i,
                                src: range.start,
                                deep: depth > MAX_CONTAINER_DEPTH,
                            });
                            open_defs.push(plan.defs.len() - 1);
                        }
                        _ => {}
                    }
                }
                Event::End(tag) => {
                    match tag {
                        TagEnd::List(_) => {
                            lists.pop();
                        }
                        TagEnd::FootnoteDefinition => {
                            if let Some(d) = open_defs.pop() {
                                plan.defs[d].end = i;
                                plan.def_end.insert(plan.defs[d].start, i);
                            }
                        }
                        _ => {}
                    }
                    if is_container_end(*tag) {
                        depth = depth.saturating_sub(1);
                    }
                }
                _ => {}
            }
        }
        for (d, def) in plan.defs.iter().enumerate() {
            if !def.deep {
                plan.chosen
                    .entry(UniCase::new(def.name.clone()))
                    .or_insert(d);
            }
        }
        let mut seen: HashSet<UniCase<String>> = HashSet::new();
        for def in &plan.defs {
            let key = UniCase::new(def.name.clone());
            if !plan.chosen.contains_key(&key) && seen.insert(key) {
                plan.unlinked.push(def.name.clone());
            }
        }
        plan
    }
}

fn is_container(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::BlockQuote(_) | Tag::List(_) | Tag::Item | Tag::FootnoteDefinition(_)
    )
}

fn is_container_end(tag: TagEnd) -> bool {
    matches!(
        tag,
        TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item | TagEnd::FootnoteDefinition
    )
}

/// Whether the item starting at `events[i]` carries a task-list marker: right after its
/// `Start`, or inside its first paragraph (a loose item).
fn is_task_item(events: &[(Event<'_>, Range<usize>)], i: usize) -> bool {
    let marker = |j: usize| matches!(events.get(j), Some((Event::TaskListMarker(_), _)));
    marker(i + 1)
        || (matches!(events.get(i + 1), Some((Event::Start(Tag::Paragraph), _))) && marker(i + 2))
}

/// An open table: its column alignments, the next cell's column, whether the head is
/// open, and whether `<tbody>` has been written.
struct Table {
    aligns: Vec<Alignment>,
    cell: usize,
    head: bool,
    body_open: bool,
}

/// The writer's state.
struct Writer<'h, 'p> {
    /// The output being written: the body, or (inside [`Writer::buffered`]) one footnote
    /// definition.
    out: String,
    /// Every byte written so far, over all buffers, against `cap`.
    total: usize,
    cap: usize,
    truncated: bool,
    positions: Vec<(usize, usize)>,
    /// The source offset last pushed to `positions`.
    last_src: Option<usize>,
    /// Open containers.
    depth: usize,
    headings: &'h [HeadingAttrs],
    opts: WriterOptions,
    plan: &'p Plan,
    table: Option<Table>,
    /// The open code block is a `<pre class="mermaid">`.
    code_pre_class: bool,
    /// Footnote name (case-folded) → ordinal N.
    ordinals: HashMap<UniCase<String>, usize>,
    /// Footnote names by ordinal (N − 1), as first referenced.
    order: Vec<String>,
    /// References written so far, by ordinal (N − 1).
    ref_counts: Vec<usize>,
    footnote_ids: Vec<String>,
    backrefs_bounded: Vec<(String, usize, usize)>,
    not_highlighted: Vec<NotHighlighted>,
    /// Highlighted HTML bytes written so far, against [`MAX_PAGE_HIGHLIGHT_BYTES`].
    highlighted_bytes: usize,
    /// The budget is used: every later block is written plain.
    highlight_exhausted: bool,
}

/// One footnote definition's rendered content.
struct Rendered {
    /// Index into [`Plan::defs`].
    def: usize,
    html: String,
    positions: Vec<(usize, usize)>,
    /// Where the back references go: before the last paragraph's `</p>\n`, or after the
    /// whole content (`None`).
    backref_at: Option<usize>,
}

impl Writer<'_, '_> {
    /// Appends `s`.
    fn push(&mut self, s: &str) {
        self.out.push_str(s);
        self.count(s.len());
    }

    /// Lets `f` append to the output and counts what it wrote.
    fn raw(&mut self, f: impl FnOnce(&mut String)) {
        let before = self.out.len();
        f(&mut self.out);
        let n = self.out.len() - before;
        self.count(n);
    }

    /// `n` more bytes written; past the cap the writer stops.
    fn count(&mut self, n: usize) {
        self.total += n;
        if self.total > self.cap {
            self.truncated = true;
        }
    }

    /// Whether the output ends with a newline (cmark's `cr` needs to know); an empty
    /// output does.
    fn end_newline(&self) -> bool {
        self.out.as_bytes().last().is_none_or(|&b| b == b'\n')
    }

    /// Starts a new line unless the output already ends with one (cmark's `cr`).
    fn cr(&mut self) {
        if !self.end_newline() {
            self.push("\n");
        }
    }

    fn text(&mut self, text: &str) {
        self.raw(|o| esc_text(o, text));
    }

    /// Records where the node at source offset `src` starts, once per source offset.
    fn mark(&mut self, src: usize) {
        if self.last_src != Some(src) {
            self.positions.push((self.out.len(), src));
            self.last_src = Some(src);
        }
    }

    /// Writes `events[from..to]`.
    fn render(&mut self, events: &[(Event<'_>, Range<usize>)], from: usize, to: usize) {
        let mut i = from;
        while i < to && !self.truncated {
            i = self.event(events, i);
        }
    }

    /// Writes `events[i]` and returns the index of the next event to write.
    fn event(&mut self, events: &[(Event<'_>, Range<usize>)], i: usize) -> usize {
        let (ev, range) = &events[i];
        let mut next = i + 1;
        match ev {
            // Definitions are written after the body, in first-reference order.
            Event::Start(Tag::FootnoteDefinition(_)) => {
                return self
                    .plan
                    .def_end
                    .get(&i)
                    .map_or(events.len(), |&end| end + 1);
            }
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                self.mark(range.start);
                self.push("<img src=\"");
                self.raw(|o| esc_href(o, dest_url));
                self.push("\" alt=\"");
                next = self.alt_text(events, next);
                if !title.is_empty() {
                    self.push("\" title=\"");
                    self.raw(|o| esc_attr(o, title));
                }
                self.push("\" />");
            }
            Event::Start(tag) => {
                self.mark(range.start);
                if let Tag::CodeBlock(CodeBlockKind::Fenced(info)) = tag {
                    if let Some(after) = self.highlighted_fence(events, i, info, range.start) {
                        return after;
                    }
                }
                self.start(tag, events, i);
            }
            Event::End(tag) => self.end(*tag, ev),
            Event::Text(text) => self.text(text),
            Event::Code(code) => {
                self.mark(range.start);
                self.push("<code>");
                self.text(code);
                self.push("</code>");
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                self.mark(range.start);
                self.html(html);
            }
            Event::SoftBreak => {
                if self.opts.soft_break_is_br {
                    self.push("<br />\n");
                } else {
                    self.push("\n");
                }
            }
            Event::HardBreak => self.push("<br />\n"),
            Event::Rule => {
                self.mark(range.start);
                self.cr();
                self.push("<hr />\n");
            }
            Event::FootnoteReference(name) => {
                self.mark(range.start);
                self.footnote_reference(name);
            }
            Event::TaskListMarker(checked) => {
                self.mark(range.start);
                // A marker before the item's paragraph is written inside it.
                let opens_paragraph =
                    matches!(events.get(next), Some((Event::Start(Tag::Paragraph), _)));
                if opens_paragraph {
                    let prange = &events[next].1;
                    self.mark(prange.start);
                    self.start(&Tag::Paragraph, events, next);
                    next += 1;
                }
                // A tight item's marker (right after `Start(Item)`, no paragraph) past the
                // depth bound loses its checkbox with the item's tags, as today's formatter
                // drops it; the space stays, as the text's. A loose item's checkbox opens a
                // paragraph, which is no container, and stays.
                let tight =
                    !opens_paragraph && i > 0 && matches!(events[i - 1].0, Event::Start(Tag::Item));
                if tight && self.depth > MAX_CONTAINER_DEPTH {
                    self.push(" ");
                } else {
                    // Alone before a line break the checkbox gets no space (the blessed pre-switch render).
                    let lone = matches!(
                        events.get(next),
                        Some((Event::SoftBreak | Event::HardBreak, _))
                    );
                    self.checkbox(*checked, !lone);
                }
            }
            Event::InlineMath(_) | Event::DisplayMath(_) => {
                unreachable!("not in Rubrica's option set (GFM): {ev:?}")
            }
        }
        next
    }

    fn html(&mut self, html: &str) {
        match self.opts.raw_html {
            RawHtmlMode::Raw => self.push(html),
            RawHtmlMode::Escape => self.text(html),
        }
    }

    fn checkbox(&mut self, checked: bool, space: bool) {
        match self.opts.task_markup {
            TaskMarkup::Gfm => self.push(if checked {
                "<input checked=\"\" disabled=\"\" type=\"checkbox\">"
            } else {
                "<input disabled=\"\" type=\"checkbox\">"
            }),
            TaskMarkup::MarkdownIt => {
                self.push("<input class=\"task-list-item-checkbox\"");
                if checked {
                    self.push(" checked=\"\"");
                }
                self.push(" disabled=\"\" type=\"checkbox\" />");
            }
        }
        if space {
            self.push(" ");
        }
    }

    /// Opens a container; whether its tags are written.
    fn enter_container(&mut self) -> bool {
        self.depth += 1;
        self.depth <= MAX_CONTAINER_DEPTH
    }

    /// Closes a container; whether its tags are written.
    fn leave_container(&mut self) -> bool {
        let written = self.depth <= MAX_CONTAINER_DEPTH;
        self.depth = self.depth.saturating_sub(1);
        written
    }

    /// The language word of a fenced block's info string, by the profile's rule.
    fn language<'i>(&self, info: &'i str) -> Cow<'i, str> {
        if self.opts.form_feed_ends_language && info.contains('\u{c}') {
            let spaced = info.replace('\u{c}', " ");
            Cow::Owned(first_word(spaced.trim()).to_owned())
        } else {
            Cow::Borrowed(first_word(info))
        }
    }

    /// Writes the fenced block opening at `events[i]` through the highlighter, when there is
    /// one and the block has a language: returns the index after the block's end. `None`
    /// means "write it as usual" (no highlighter, no language, an unknown language or a
    /// capped block; a capped block is also recorded in [`Written::not_highlighted`]).
    fn highlighted_fence(
        &mut self,
        events: &[(Event<'_>, Range<usize>)],
        i: usize,
        info: &str,
        offset: usize,
    ) -> Option<usize> {
        let highlighter = self.opts.highlighter?;
        let lang = self.language(info);
        if lang.is_empty() || (self.opts.mermaid == MermaidMarkup::PreClass && lang == "mermaid") {
            return None;
        }
        let mut literal = String::new();
        let mut end = i + 1;
        while let Some((Event::Text(text), _)) = events.get(end) {
            literal.push_str(text);
            end += 1;
        }
        if !matches!(events.get(end), Some((Event::End(TagEnd::CodeBlock), _))) {
            return None;
        }
        // Past the page's budget, a block is not even highlighted.
        let highlight = if self.highlight_exhausted {
            None
        } else {
            Some((highlighter.0)(&lang, &literal))
        };
        let highlight = match highlight {
            Some(Highlight::Html(html))
                if self.highlighted_bytes + html.len() > MAX_PAGE_HIGHLIGHT_BYTES =>
            {
                self.highlight_exhausted = true;
                None
            }
            other => other,
        };
        let Some(highlight) = highlight else {
            self.not_highlighted.push(NotHighlighted {
                offset,
                bytes: literal.trim_end_matches('\n').len(),
                longest_line: literal.lines().map(str::len).max().unwrap_or(0),
                block_cap: 0,
                line_cap: 0,
                page_budget: Some(MAX_PAGE_HIGHLIGHT_BYTES),
            });
            return None;
        };
        match highlight {
            Highlight::Html(html) => {
                self.highlighted_bytes += html.len();
                self.cr();
                self.push("<pre><code class=\"language-");
                self.raw(|o| esc_attr(o, &lang));
                self.push("\">");
                self.push(&html);
                self.push("</code></pre>\n");
                Some(end + 1)
            }
            Highlight::Plain => None,
            Highlight::OverCap {
                bytes,
                longest_line,
                block_cap,
                line_cap,
            } => {
                self.not_highlighted.push(NotHighlighted {
                    offset,
                    bytes,
                    longest_line,
                    block_cap,
                    line_cap,
                    page_budget: None,
                });
                None
            }
        }
    }

    fn start(&mut self, tag: &Tag<'_>, events: &[(Event<'_>, Range<usize>)], i: usize) {
        match tag {
            Tag::Paragraph => {
                self.cr();
                self.push("<p>");
            }
            Tag::Heading { level, .. } => {
                // Upstream's `id`, `classes` and `attrs` are ignored (heading attributes
                // are not parsed); the id and the anchor come from the caller's list, read
                // by the heading's place in the source.
                self.cr();
                let attrs = self
                    .plan
                    .headings
                    .binary_search(&i)
                    .ok()
                    .and_then(|n| self.headings.get(n));
                self.raw(|o| {
                    let _ = write!(o, "<{level}");
                });
                if let Some(id) = attrs.and_then(|h| h.id.as_deref()) {
                    self.push(" id=\"");
                    self.raw(|o| esc_attr(o, id));
                    self.push("\"");
                }
                self.push(">");
                if let Some(anchor) = attrs.and_then(|h| h.sec_anchor.as_deref()) {
                    self.push("<span class=\"sec\" id=\"");
                    self.raw(|o| esc_attr(o, anchor));
                    self.push("\"></span>");
                }
            }
            Tag::BlockQuote(_) => {
                if self.enter_container() {
                    self.cr();
                    self.push("<blockquote>\n");
                }
            }
            Tag::CodeBlock(kind) => {
                self.cr();
                match kind {
                    CodeBlockKind::Indented => self.push("<pre><code>"),
                    CodeBlockKind::Fenced(info) => {
                        let lang = self.language(info);
                        if self.opts.mermaid == MermaidMarkup::PreClass && lang == "mermaid" {
                            self.push("<pre class=\"mermaid\">");
                            self.code_pre_class = true;
                        } else if lang.is_empty() {
                            self.push("<pre><code>");
                        } else {
                            self.push("<pre><code class=\"language-");
                            self.raw(|o| esc_attr(o, &lang));
                            self.push("\">");
                        }
                    }
                }
            }
            // cmark's `cr` before an HTML block, once per block (its lines come as
            // separate `Html` events, an indented block's leading spaces as their own):
            // an HTML block that opens inline, the first child of a tight item, starts on
            // its own line (spec example 175).
            Tag::HtmlBlock => self.cr(),
            Tag::List(start) => {
                if self.enter_container() {
                    self.cr();
                    self.push(if start.is_some() { "<ol" } else { "<ul" });
                    if self.opts.task_markup == TaskMarkup::MarkdownIt && self.plan.task_lists[i] {
                        self.push(" class=\"contains-task-list\"");
                    }
                    match start {
                        Some(1) | None => {}
                        Some(start) => self.raw(|o| {
                            let _ = write!(o, " start=\"{start}\"");
                        }),
                    }
                    self.push(">\n");
                }
            }
            Tag::Item => {
                if self.enter_container() {
                    self.cr();
                    if self.opts.task_markup == TaskMarkup::MarkdownIt && is_task_item(events, i) {
                        self.push("<li class=\"task-list-item\">");
                    } else {
                        self.push("<li>");
                    }
                }
            }
            Tag::FootnoteDefinition(_) => {
                unreachable!("Start(FootnoteDefinition) is skipped by the event loop")
            }
            Tag::Emphasis => self.push("<em>"),
            Tag::Strong => self.push("<strong>"),
            Tag::Strikethrough => self.push("<del>"),
            Tag::Link {
                link_type,
                dest_url,
                title,
                ..
            } => {
                self.push("<a href=\"");
                if *link_type == LinkType::Email && !dest_url.starts_with("mailto:") {
                    self.push("mailto:");
                }
                self.raw(|o| esc_href(o, dest_url));
                if !title.is_empty() {
                    self.push("\" title=\"");
                    self.raw(|o| esc_attr(o, title));
                }
                self.push("\">");
            }
            Tag::Image { .. } => unreachable!("Start(Image) is written by the alt-text fold"),
            Tag::Table(aligns) => {
                self.cr();
                self.push("<table>\n");
                self.table = Some(Table {
                    aligns: aligns.clone(),
                    cell: 0,
                    head: false,
                    body_open: false,
                });
            }
            Tag::TableHead => {
                self.push("<thead>\n<tr>\n");
                let t = self.table();
                t.head = true;
                t.cell = 0;
            }
            Tag::TableRow => {
                let t = self.table();
                let open_body = !t.body_open;
                t.body_open = true;
                t.cell = 0;
                if open_body {
                    self.push("<tbody>\n");
                }
                self.push("<tr>\n");
            }
            Tag::TableCell => {
                let t = self.table();
                let head = t.head;
                let align = match t.aligns.get(t.cell) {
                    Some(Alignment::Left) => " align=\"left\"",
                    Some(Alignment::Center) => " align=\"center\"",
                    Some(Alignment::Right) => " align=\"right\"",
                    Some(Alignment::None) | None => "",
                };
                t.cell += 1;
                self.push(if head { "<th" } else { "<td" });
                self.push(align);
                self.push(">");
            }
            Tag::MetadataBlock(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Superscript
            | Tag::Subscript => unreachable!("not in Rubrica's option set (GFM): {tag:?}"),
        }
    }

    fn table(&mut self) -> &mut Table {
        self.table
            .as_mut()
            .expect("a table head, row or cell outside a table")
    }

    fn end(&mut self, tag: TagEnd, ev: &Event<'_>) {
        match tag {
            TagEnd::Paragraph => self.push("</p>\n"),
            TagEnd::Heading(level) => {
                self.raw(|o| {
                    let _ = write!(o, "</{level}>");
                });
                self.push("\n");
            }
            TagEnd::BlockQuote(_) => {
                if self.leave_container() {
                    self.push("</blockquote>\n");
                }
            }
            TagEnd::CodeBlock => {
                if self.code_pre_class {
                    self.code_pre_class = false;
                    self.push("</pre>\n");
                } else {
                    self.push("</code></pre>\n");
                }
            }
            TagEnd::HtmlBlock => self.cr(),
            TagEnd::List(ordered) => {
                if self.leave_container() {
                    self.push(if ordered { "</ol>\n" } else { "</ul>\n" });
                }
            }
            TagEnd::Item => {
                if self.leave_container() {
                    self.push("</li>\n");
                }
            }
            TagEnd::FootnoteDefinition => {
                unreachable!("End(FootnoteDefinition) is skipped by the event loop")
            }
            TagEnd::Emphasis => self.push("</em>"),
            TagEnd::Strong => self.push("</strong>"),
            TagEnd::Strikethrough => self.push("</del>"),
            TagEnd::Link => self.push("</a>"),
            TagEnd::Image => unreachable!("End(Image) is consumed by the alt-text fold"),
            TagEnd::Table => {
                let body_open = self.table.take().is_some_and(|t| t.body_open);
                if body_open {
                    self.push("</tbody>\n");
                }
                self.push("</table>\n");
            }
            TagEnd::TableHead => {
                self.table().head = false;
                self.push("</tr>\n</thead>\n");
            }
            TagEnd::TableRow => self.push("</tr>\n"),
            TagEnd::TableCell => {
                let head = self.table().head;
                self.push(if head { "</th>\n" } else { "</td>\n" });
            }
            TagEnd::MetadataBlock(_)
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Superscript
            | TagEnd::Subscript => unreachable!("not in Rubrica's option set (GFM): {ev:?}"),
        }
    }

    /// Writes the `alt` text of an image from `events[from..]` up to and including the
    /// image's own `End`, the way the spec folds it: text, code and inline HTML as escaped
    /// text, breaks and rules as a space, nested spans and images by their text; footnote
    /// references and task-list markers write nothing. Returns the index after the
    /// consumed `End`.
    fn alt_text(&mut self, events: &[(Event<'_>, Range<usize>)], from: usize) -> usize {
        let mut nest = 0usize;
        let mut i = from;
        while i < events.len() {
            let ev = &events[i].0;
            i += 1;
            match ev {
                Event::Start(_) => nest += 1,
                Event::End(_) => {
                    if nest == 0 {
                        return i;
                    }
                    nest -= 1;
                }
                Event::Html(_) => {}
                Event::InlineHtml(text) | Event::Code(text) | Event::Text(text) => {
                    self.raw(|o| esc_attr(o, text));
                }
                Event::SoftBreak | Event::HardBreak | Event::Rule => self.push(" "),
                Event::FootnoteReference(_) | Event::TaskListMarker(_) => {}
                Event::InlineMath(_) | Event::DisplayMath(_) => {
                    unreachable!("not in Rubrica's option set (GFM): {ev:?}")
                }
            }
        }
        i
    }

    /// The ordinal of the footnote `key` names, assigned at its first reference.
    fn ordinal(&mut self, key: UniCase<String>) -> usize {
        if let Some(&n) = self.ordinals.get(&key) {
            return n;
        }
        self.order.push(key.clone().into_inner());
        self.ref_counts.push(0);
        let n = self.order.len();
        self.ordinals.insert(key, n);
        n
    }

    fn footnote_reference(&mut self, name: &str) {
        let key = UniCase::new(name.to_owned());
        let linked = self.plan.chosen.contains_key(&key);
        let n = self.ordinal(key);
        self.ref_counts[n - 1] += 1;
        let k = self.ref_counts[n - 1];
        if linked {
            let id = if k == 1 {
                format!("fnref-{n}")
            } else {
                format!("fnref-{n}-{k}")
            };
            self.raw(|o| {
                let _ = write!(
                    o,
                    "<sup class=\"footnote-ref\"><a href=\"#fn-{n}\" id=\"{id}\" data-footnote-ref>{n}</a></sup>"
                );
            });
            self.footnote_ids.push(id);
        } else {
            self.raw(|o| {
                let _ = write!(o, "<sup class=\"footnote-ref\">{n}</sup>");
            });
        }
    }

    /// Runs `f` with a fresh output and position vector, one container deep (the
    /// definition itself), and returns what it wrote.
    fn buffered(&mut self, f: impl FnOnce(&mut Self)) -> (String, Vec<(usize, usize)>) {
        let out = mem::take(&mut self.out);
        let positions = mem::take(&mut self.positions);
        let last_src = self.last_src.take();
        let depth = mem::replace(&mut self.depth, 1);
        let table = self.table.take();
        let code_pre_class = mem::replace(&mut self.code_pre_class, false);
        f(self);
        let html = mem::replace(&mut self.out, out);
        let written = mem::replace(&mut self.positions, positions);
        self.last_src = last_src;
        self.depth = depth;
        self.table = table;
        self.code_pre_class = code_pre_class;
        (html, written)
    }

    /// Writes the referenced footnote definitions after the body, in first-reference
    /// order (a definition's own references extend the order), each with its back
    /// references.
    fn footnotes_section(&mut self, events: &[(Event<'_>, Range<usize>)]) {
        let mut rendered: Vec<Option<Rendered>> = Vec::new();
        while rendered.len() < self.order.len() && !self.truncated {
            let key = UniCase::new(self.order[rendered.len()].clone());
            let Some(&d) = self.plan.chosen.get(&key) else {
                rendered.push(None);
                continue;
            };
            let (start, end) = (self.plan.defs[d].start, self.plan.defs[d].end);
            let (html, positions) = self.buffered(|w| w.render(events, start + 1, end));
            // The back references go inside the last paragraph when it is the last child
            // (a buffer the cap cut short may lack the `</p>`; the writer stops anyway).
            let last_is_paragraph =
                end > start + 1 && matches!(events[end - 1].0, Event::End(TagEnd::Paragraph));
            let backref_at = if last_is_paragraph {
                html.strip_suffix("</p>\n").map(str::len)
            } else {
                None
            };
            rendered.push(Some(Rendered {
                def: d,
                html,
                positions,
                backref_at,
            }));
        }
        if self.truncated || rendered.iter().all(Option::is_none) {
            return;
        }
        self.cr();
        // The first definition's HTML starts at the section (today's rule: the position is
        // pushed on entering the node, before `<section>`, as in the pre-switch render); the others' at
        // their `<li>`.
        if let Some(first) = rendered.iter().flatten().next() {
            let src = self.plan.defs[first.def].src;
            self.mark(src);
        }
        self.push("<section class=\"footnotes\" data-footnotes>\n<ol>\n");
        for (idx, r) in rendered.into_iter().enumerate() {
            let Some(r) = r else {
                continue;
            };
            let n = idx + 1;
            let src = self.plan.defs[r.def].src;
            let refs = self.ref_counts[idx];
            if refs > MAX_FOOTNOTE_BACKREFS as usize {
                self.backrefs_bounded
                    .push((self.order[idx].clone(), refs, src));
            }
            self.mark(src);
            // The newline after `<li …>` is the first block's `cr` (as in the blessed
            // pre-switch render): an empty definition has its back references on the `<li>` line.
            self.raw(|o| {
                let _ = write!(o, "<li id=\"fn-{n}\">");
            });
            if !r.html.is_empty() {
                self.push("\n");
            }
            self.footnote_ids.push(format!("fn-{n}"));
            // `data-footnote-backref-idx` and `aria-label` name the reference as cmark-gfm
            // and the pre-switch render do: `N` for the first, `N-k` after it.
            let mut backrefs = String::new();
            for k in 1..=refs.min(MAX_FOOTNOTE_BACKREFS as usize) {
                if k > 1 {
                    let _ = write!(
                        backrefs,
                        " <a href=\"#fnref-{n}-{k}\" class=\"footnote-backref\" data-footnote-backref data-footnote-backref-idx=\"{n}-{k}\" aria-label=\"Back to reference {n}-{k}\">↩<sup class=\"footnote-ref\">{k}</sup></a>"
                    );
                } else {
                    let _ = write!(
                        backrefs,
                        "<a href=\"#fnref-{n}\" class=\"footnote-backref\" data-footnote-backref data-footnote-backref-idx=\"{n}\" aria-label=\"Back to reference {n}\">↩</a>"
                    );
                }
            }
            let base = self.out.len();
            let (split, inserted) = match r.backref_at {
                Some(p) => {
                    self.append_buffered(&r.html[..p]);
                    let before = self.out.len();
                    self.push(" ");
                    self.push(&backrefs);
                    let inserted = self.out.len() - before;
                    self.append_buffered(&r.html[p..]);
                    (p, inserted)
                }
                None => {
                    self.append_buffered(&r.html);
                    self.push(&backrefs);
                    self.push("\n");
                    (r.html.len(), 0)
                }
            };
            for (h, s) in r.positions {
                let shift = if h >= split { inserted } else { 0 };
                self.positions.push((base + h + shift, s));
            }
            self.push("</li>\n");
            if self.truncated {
                return;
            }
        }
        self.push("</ol>\n</section>\n");
    }

    /// Appends HTML already counted against the cap when it was first written.
    fn append_buffered(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn finish(mut self) -> Written {
        if self.truncated {
            // Nothing of the page stands: no id, no record.
            return Written {
                html: REFUSAL_NOTICE.to_owned(),
                truncated: true,
                ..Written::default()
            };
        }
        self.positions.sort_by_key(|&(html, _)| html);
        Written {
            html: self.out,
            positions: self.positions,
            footnote_ids: self.footnote_ids,
            backrefs_bounded: self.backrefs_bounded,
            not_highlighted: self.not_highlighted,
            unlinked_footnotes: self.plan.unlinked.clone(),
            truncated: false,
        }
    }
}

/// The first word of an info string: up to the first space or tab.
fn first_word(info: &str) -> &str {
    info.split([' ', '\t']).next().unwrap_or("")
}

/// Appends `s` with `&`, `<`, `>` and `"` escaped: text content, code content.
fn esc_text(out: &mut String, s: &str) {
    let bytes = s.as_bytes();
    let mut mark = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let rep = match b {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' => "&quot;",
            // CommonMark 2.3: U+0000 is U+FFFD.
            0 => "\u{fffd}",
            _ => continue,
        };
        out.push_str(&s[mark..i]);
        out.push_str(rep);
        mark = i + 1;
    }
    out.push_str(&s[mark..]);
}

/// Appends `s` with `&`, `<`, `>` and `"` escaped: a quoted attribute value.
fn esc_attr(out: &mut String, s: &str) {
    esc_text(out, s);
}

/// Appends `s` as an `href`/`src` value: [`escape_href`], then `&` as `&amp;` and `'` as
/// `&#x27;` for the attribute.
fn esc_href(out: &mut String, s: &str) {
    for c in escape_href(s).chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '\'' => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
}

/// A link or image destination as the writer writes it in `href` or `src`, before the
/// attribute's own escaping (so as a browser reads the attribute back): ASCII letters and
/// digits, `!#$&'()*+,-./:;=?@^_~` and a `%` that starts a `%XX` escape are kept; any other
/// `%` is `%25`, as markdown-it writes it; a NUL is U+FFFD (`%EF%BF%BD`); every
/// other byte is percent-encoded, as cmark does. The one source of the rule for the site's
/// link checks (`rubrica_site::links::written_href`).
pub fn escape_href(href: &str) -> String {
    // Byte by byte: a percent-encoded byte may be one of a multi-byte character, so no
    // `&str` slice can start after it. Every byte pushed as a `char` is ASCII.
    let b = href.as_bytes();
    let mut out = String::with_capacity(b.len());
    for (i, &c) in b.iter().enumerate() {
        let hex = |at: usize| b.get(at).is_some_and(u8::is_ascii_hexdigit);
        match c {
            b'%' if hex(i + 1) && hex(i + 2) => out.push('%'),
            b'%' => out.push_str("%25"),
            _ if href_safe(c) || c == b'&' || c == b'\'' => out.push(c as char),
            0 => out.push_str("%EF%BF%BD"),
            _ => {
                let _ = write!(out, "%{c:02X}");
            }
        }
    }
    out
}

fn href_safe(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b'-'
                | b'.'
                | b'/'
                | b':'
                | b';'
                | b'='
                | b'?'
                | b'@'
                | b'^'
                | b'_'
                | b'~'
        )
}
