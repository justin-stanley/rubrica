//! `@sec` markers: `<!-- @sec id=a/b/c tags=x,y -->` on its own line,
//! immediately before an ATX heading, names that heading's section for
//! a section reader and gives it a stable anchor,
//! `<span class="sec" id="sec-a/b/c">`, that survives a heading rename.
//!
//! The rule is `check-packages.cjs`'s (the Wiki.js-era checker it mirrors): the
//! comment alone on its line, the next line an ATX heading (`^ {0,3}#{1,6}\s`,
//! so a blank line between, a setext heading or anything else loses the
//! heading), an `id`; tags are a comma list. Rubrica reads markers from the
//! parse (an HTML block that is the marker line), and puts the anchor on the
//! heading the parse has after it.
//!
//! **Parity with `check-packages.cjs` of the body, plus front-matter
//! markers reported, by construction.** Rubrica also runs `check-packages`'
//! own harvest ([`check_packages_markers`]: its regex as JavaScript runs it,
//! over the body as given, or over the whole file as `check-packages` reads
//! it, front matter included, in [`crate::markdown::render_page`]) and
//! pairs each harvested marker of the body with the parse's by line and id:
//!
//! - a match that starts in the front matter (a marker in a YAML block
//!   scalar) is a [`RenderFinding::SecInFrontMatter`] at its line in the
//!   file, and is not in `secs`: `check-packages` counts its id and Rubrica
//!   anchors nothing in front matter. One left open there runs on to the
//!   next `-->`, in the body, and a body marker it takes gets no anchor: it
//!   is [`RenderFinding::SecNotSeenByCheckPackages`] if it has an id, and
//!   [`RenderFinding::SecWithoutId`] if it has none (see below);
//! - [`crate::Rendered::secs`] is the body's harvested markers with an id,
//!   always, so the repo-wide set is `check-packages`' set less the
//!   front-matter ids, each of which is reported;
//! - each harvested marker is an anchor on a heading or named by a finding
//!   at its line: one of the parse's below, or, where the parse has no
//!   marker on that line, [`RenderFinding::SecNotRendered`] with what it has
//!   there ([`SecHidden`]: code, fenced or indented; another HTML block, which
//!   the marker follows with no blank line or which a comment over two lines
//!   is; text, where JavaScript's U+2028 or U+2029 starts a line the parser does
//!   not) or [`RenderFinding::SecWithoutId`] for one with no id;
//! - a marker the parse sees and the harvest does not gets no anchor: it is a
//!   [`RenderFinding::SecInContainer`] behind `> ` or `- `, and otherwise a
//!   [`RenderFinding::SecNotSeenByCheckPackages`] (`render` skips a leading
//!   byte-order mark, JavaScript does not; or a match from the front matter
//!   took it);
//! - a marker with no id is [`RenderFinding::SecWithoutId`] whether or not
//!   check-packages sees it; [`RenderFinding::SecNotSeenByCheckPackages`]
//!   covers only markers with an id. The parse's no-id markers check-packages
//!   misses have the same two causes: a marker left open in the front matter
//!   that runs on into a body marker, and a leading byte-order mark in the
//!   body.
//!
//! So the anchors are a subset of `check-packages`' ids of the body, and a
//! page with no `@sec` finding has an anchor for each of its ids
//! (`tests/sec_parity.rs` checks both on 5,000 documents answered by the
//! JavaScript itself and 50,000 more against the port, and on 20,000 page
//! files with markers in their front matter). The parse's own findings,
//! where it is stricter:
//!
//! - a marker inside a blockquote, list item or footnote definition is a
//!   [`RenderFinding::SecInContainer`] and gets no anchor, even where
//!   `check-packages` sees it (only blanks before it on its line: a list
//!   item's continuation, `  <!-- @sec … -->`);
//! - a marker on an ATX heading with no visible text (`#`, `# #`, `##  `, an
//!   image only, or only invisible characters such as U+200B) is a
//!   [`RenderFinding::SecOnEmptyHeading`]: `check-packages` reports `#` alone
//!   (`#{1,6}\s` needs a blank after the hashes), and an anchor on a heading
//!   with no text is one no reader can see;
//! - the next line is found as `check-packages` finds it, after a `\n`: a
//!   marker whose line ends in a bare `\r` (a CR-only file) names no heading.
//!   `\r\n` is a line end to both;
//! - attributes are split on JavaScript's `\s`, which has U+FEFF and not
//!   U+0085 (Rust's `char::is_whitespace` is the reverse): `id=⟨U+FEFF⟩a` is
//!   a marker with no id, `id=a⟨U+0085⟩b` the id `a⟨U+0085⟩b`;
//! - `check-packages`' heading test is `^ {0,3}#{1,6}\s`, and JavaScript's
//!   `\s` takes NBSP, so `#⟨NBSP⟩H` is a heading to it and a paragraph to
//!   CommonMark (and markdown-it): Rubrica reports
//!   [`RenderFinding::SecWithoutHeading`], as the heading Wiki.js showed is no
//!   heading either.

use crate::frontmatter::Severity;

/// One `@sec` marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecMarker {
    /// The section id (`ops/deployments/golden-path`); may contain `/`, a
    /// valid HTML5 id character. The anchor's id is `sec-<id>`.
    pub id: String,
    /// The `tags=` list, in order, empty entries dropped.
    pub tags: Vec<String>,
    /// 1-based source line of the marker.
    pub line: usize,
}

impl SecMarker {
    /// The id of the anchor the marker puts on its heading.
    pub fn anchor(&self) -> String {
        format!("sec-{}", self.id)
    }
}

/// What a render found wrong with the page (not with its front matter: see
/// [`crate::frontmatter::FrontMatterFinding`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RenderFinding {
    /// A marker not immediately followed by an ATX heading: its section has
    /// no anchor and no heading (a section reader resolves it with heading "").
    SecWithoutHeading {
        /// The marker's id.
        id: String,
        /// The marker's line.
        line: usize,
    },
    /// A marker with no `id=`, whether or not `check-packages.cjs` sees it
    /// (a front-matter match can take it, a byte-order mark can hide it).
    SecWithoutId {
        /// The marker's line.
        line: usize,
    },
    /// A second marker with an id already used on this page: it gets no
    /// anchor (ids are unique in a page). Repo-wide uniqueness is the
    /// corpus's check.
    DuplicateSecId {
        /// The id.
        id: String,
        /// The second marker's line.
        line: usize,
        /// The first marker's line.
        first: usize,
    },
    /// The marker's anchor id (`sec-<id>`) is also a heading id on the page
    /// (a heading `Sec <id>`): the heading keeps it, the marker gets no
    /// anchor.
    SecAnchorIsAHeadingId {
        /// The marker's id.
        id: String,
        /// The marker's line.
        line: usize,
    },
    /// A marker inside a blockquote, list item or footnote definition: it gets
    /// no anchor (`check-packages.cjs` reads lines, and does not see one behind
    /// `> ` or `- `; see the module docs).
    SecInContainer {
        /// The marker's id.
        id: String,
        /// The marker's line.
        line: usize,
    },
    /// A marker before an ATX heading with no visible text (`#`, or only
    /// invisible characters): no anchor.
    SecOnEmptyHeading {
        /// The marker's id.
        id: String,
        /// The marker's line.
        line: usize,
    },
    /// A heading's id is also a footnote's (`# fn-1` and `[^1]`'s
    /// `<li id="fn-1">`, or a reference's `fnref-1`): both keep it, and a
    /// link to it reaches the first in the page.
    HeadingIdIsAFootnoteId {
        /// The id.
        id: String,
        /// The heading's line.
        line: usize,
    },
    /// A footnote referenced more than [`crate::markdown::MAX_FOOTNOTE_BACKREFS`]
    /// times: its definition links back to the first that many only.
    FootnoteBackrefsBounded {
        /// The footnote's name.
        name: String,
        /// How many references it has.
        references: usize,
        /// The definition's line.
        line: usize,
    },
    /// Raw elements nested deeper than 100 (unclosed `<sub>`s count): this
    /// start tag, the first, and every later raw tag that would open deeper,
    /// are dropped with their end tags (see [`crate::sanitize`]).
    RawHtmlTooDeep {
        /// The line of the piece with the first dropped tag.
        line: usize,
    },
    /// A raw HTML piece with a tag of more than
    /// [`crate::sanitize::MAX_RAW_ATTRIBUTES`] attributes, written as text.
    RawHtmlTooManyAttributes {
        /// The piece's line.
        line: usize,
    },
    /// An attribute left out because ammonia's serializer would spend too
    /// long on its value ([`crate::sanitize::MAX_ATTRIBUTE_COST`]). An `@sec`
    /// anchor's id is checked before it is written: that marker gets no
    /// anchor (its id stays in [`crate::Rendered::secs`]).
    AttributesTooCostly {
        /// The element (`a`, `img`, `code`, `span`, …).
        element: String,
        /// The attribute (`href`, `title`, `class`, `id`, …).
        attribute: String,
        /// The line of the Markdown that wrote it (its block's, or its
        /// link's or image's).
        line: usize,
    },
    /// A marker `check-packages.cjs` harvests that Rubrica's parse does not
    /// see as one (in code, inside another HTML block, or not a block of its
    /// own): no anchor.
    SecNotRendered {
        /// The marker's id.
        id: String,
        /// The marker's line.
        line: usize,
        /// What the parse sees there instead.
        why: SecHidden,
    },
    /// A marker with an id that Rubrica's parse sees and `check-packages.cjs`
    /// does not (a byte-order mark before it, which `render` skips and
    /// JavaScript keeps, or, in [`crate::markdown::render_page`], a match
    /// that started in the front matter and ran on to its `-->`): no anchor,
    /// and not in [`crate::Rendered::secs`]. Only markers with an id: a
    /// marker with no id is [`RenderFinding::SecWithoutId`] whether or not
    /// `check-packages.cjs` sees it, for either cause.
    SecNotSeenByCheckPackages {
        /// The marker's id.
        id: String,
        /// The marker's line.
        line: usize,
    },
    /// A heading whose text is only invisible characters (zero-width
    /// spaces and joiners, U+2060, U+FEFF and the other default-ignorable
    /// code points): no reader can see it, or click it in a table of
    /// contents.
    InvisibleHeading {
        /// The heading's line.
        line: usize,
    },
    /// A marker `check-packages.cjs` harvests from the page's front matter
    /// (it reads the whole file; Rubrica's markers are the body's): a match
    /// that starts in the YAML, such as a `<!-- @sec … -->` line in a block
    /// scalar. Not in [`crate::Rendered::secs`], and its line is the file's,
    /// not the body's. Reported by [`crate::markdown::render_page`], which
    /// has the file, first among the page's findings.
    SecInFrontMatter {
        /// The id check-packages reads from the match, if any.
        id: Option<String>,
        /// The match's line in the file (not the body).
        line: usize,
    },
    /// The page body is longer than [`crate::markdown::MAX_PAGE_BYTES`]: it
    /// is not rendered, and a notice stands in for it.
    PageTooLarge {
        /// The body's length.
        bytes: usize,
    },
    /// A fenced code block over the highlighter's per-line or per-block cap: it is
    /// written plain. A warning; it never stops a build.
    CodeBlockNotHighlighted {
        /// The line of the block's opening fence.
        line: usize,
        /// The block's length in bytes, its trailing newline trimmed.
        bytes: usize,
        /// The longest line's length in bytes, its line end not counted.
        longest_line: usize,
        /// The most bytes a block may have and be highlighted.
        block_cap: usize,
        /// The most bytes a line may have and be highlighted.
        line_cap: usize,
        /// `Some(budget)` when the block was written plain because the page's
        /// highlighted output had used its budget (then the two caps above are 0);
        /// `None` when the block was over a cap.
        page_budget: Option<usize>,
    },
}

/// What Rubrica's parse sees where `check-packages.cjs` harvests a marker
/// ([`RenderFinding::SecNotRendered`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SecHidden {
    /// A fenced or indented code block.
    Code,
    /// Part of another HTML block: one the marker does not start (a
    /// `<div>` the line follows), or a marker comment that goes on past
    /// its line (`<!-- @sec id=a⏎-->`).
    HtmlBlock,
    /// Text: the marker's line is not a line to the parser (JavaScript ends a
    /// line at U+2028 and U+2029 too), or its comment is not alone on its
    /// line to the parser.
    Text,
}

impl RenderFinding {
    /// How bad it is: errors to `rubrica check`, except the highlighting cap, a warning.
    pub fn severity(&self) -> Severity {
        match self {
            Self::CodeBlockNotHighlighted { .. } => Severity::Warning,
            _ => Severity::Error,
        }
    }

    /// The 1-based line it is reported at.
    pub fn line(&self) -> usize {
        match self {
            Self::SecWithoutHeading { line, .. }
            | Self::SecWithoutId { line }
            | Self::DuplicateSecId { line, .. }
            | Self::SecAnchorIsAHeadingId { line, .. }
            | Self::SecInContainer { line, .. }
            | Self::SecOnEmptyHeading { line, .. }
            | Self::HeadingIdIsAFootnoteId { line, .. }
            | Self::FootnoteBackrefsBounded { line, .. }
            | Self::RawHtmlTooDeep { line }
            | Self::RawHtmlTooManyAttributes { line }
            | Self::AttributesTooCostly { line, .. }
            | Self::SecNotRendered { line, .. }
            | Self::SecNotSeenByCheckPackages { line, .. }
            | Self::InvisibleHeading { line }
            | Self::SecInFrontMatter { line, .. }
            | Self::CodeBlockNotHighlighted { line, .. } => *line,
            Self::PageTooLarge { .. } => 1,
        }
    }
}

/// The marker an HTML block is, if it is one: `Some(Ok)` a marker, `Some(Err)`
/// a marker with no id, `None` not a marker.
///
/// `check-packages.cjs`'s regex, `^[ \t]*<!--[ \t]*@sec[ \t]+([^>]*?)-->[ \t]*$`,
/// over the whole block: a marker block is that one line. The attributes are
/// split on JavaScript's whitespace ([`is_js_space`]) into `key=value` (the
/// last of a key wins, as its object assignment does).
pub(crate) fn parse_marker(literal: &str, line: usize) -> Option<Result<SecMarker, usize>> {
    let text = literal.strip_suffix('\n').unwrap_or(literal);
    if text.contains('\n') {
        return None;
    }
    let blank = |c: char| c == ' ' || c == '\t';
    let rest = text.trim_start_matches(blank).strip_prefix("<!--")?;
    let rest = rest.trim_start_matches(blank).strip_prefix("@sec")?;
    if !rest.starts_with(blank) {
        return None;
    }
    let attrs = rest.trim_end_matches(blank).strip_suffix("-->")?;
    if attrs.contains('>') {
        return None;
    }
    Some(parse_attrs(attrs, line).ok_or(line))
}

/// JavaScript's `\s` (ECMA-262 WhiteSpace and LineTerminator): what
/// `check-packages.cjs`'s `trim()` and `split(/\s+/)` take. Unlike
/// [`char::is_whitespace`] it has U+FEFF and not U+0085.
pub(crate) fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// A marker as `check-packages.cjs` harvests it: its line (as the parser counts
/// lines) and the marker, or `Err(line)` for one with no id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarvestedMarker {
    /// 1-based line of the match's start.
    pub line: usize,
    /// The marker; `None` when it has no id.
    pub marker: Option<SecMarker>,
}

/// Every marker `check-packages.cjs` harvests from `body`, in order: its
/// regex, `/^[ \t]*<!--[ \t]*@sec[ \t]+([^>]*?)-->[ \t]*$/gm`, run as
/// JavaScript runs it (`^` and `$` at a `\n`, `\r`, U+2028 or U+2029; the
/// group crossing line ends; each search from the last match's end), then
/// its `trim().split(/\s+/)` into `key=value` (the last of a key wins).
/// Linear: the group ends at the first `>` after `@sec`, found with one
/// forward cursor for the whole body.
pub fn check_packages_markers(body: &str) -> Vec<HarvestedMarker> {
    let b = body.as_bytes();
    let n = b.len();
    let blank = |c: u8| c == b' ' || c == b'\t';
    // A JavaScript line terminator at `i`, and its length.
    let terminator = |i: usize| match b.get(i) {
        Some(b'\n' | b'\r') => 1,
        Some(0xE2) if matches!(b.get(i + 1..i + 3), Some([0x80, 0xA8 | 0xA9])) => 3,
        _ => 0,
    };
    let mut out = Vec::new();
    // The parser's line at `counted`: each `\r\n`, `\r` or `\n` ends one.
    let (mut line, mut counted) = (1, 0);
    // The regex's `lastIndex`, and the cursor on the first `>` at or after
    // some position no later than the group's start.
    let (mut last, mut gt) = (0, 0);
    // `p` runs over the positions `^` matches at: 0 and after terminators.
    let mut p = 0;
    while p <= n {
        if p >= last {
            for (k, &c) in b.iter().enumerate().take(p).skip(counted) {
                if c == b'\r' || (c == b'\n' && (k == 0 || b[k - 1] != b'\r')) {
                    line += 1;
                }
            }
            counted = counted.max(p);
            if let Some((group, end)) = match_at(b, p, &mut gt, blank, terminator) {
                out.push(HarvestedMarker {
                    line,
                    marker: parse_attrs(&body[group], line),
                });
                last = end;
            }
        }
        // The next position after a terminator.
        match (p..n).find(|&i| terminator(i) > 0) {
            Some(i) => p = i + terminator(i),
            None => break,
        }
    }
    out
}

/// [`check_packages_markers`] of a page file `src` whose body starts after
/// line `offset` (as [`crate::split_front_matter`] gives it), split as the
/// body's render needs it: the matches that start in the body, at body
/// lines, and a [`RenderFinding::SecInFrontMatter`] at its file line for
/// each match that starts in the front matter. A match that starts in the
/// front matter and runs on into the body (a marker left open in a block
/// scalar) takes the body's text up to its `-->`, as check-packages reads
/// it: a marker there is no match of its own.
pub(crate) fn page_markers(src: &str, offset: usize) -> (Vec<HarvestedMarker>, Vec<RenderFinding>) {
    let (mut body, mut front) = (Vec::new(), Vec::new());
    for h in check_packages_markers(src) {
        if h.line <= offset {
            front.push(RenderFinding::SecInFrontMatter {
                id: h.marker.map(|m| m.id),
                line: h.line,
            });
        } else {
            body.push(HarvestedMarker {
                line: h.line - offset,
                marker: h.marker.map(|m| SecMarker {
                    line: m.line - offset,
                    ..m
                }),
            });
        }
    }
    (body, front)
}

/// The regex at `p`: the group's range and the match's end.
fn match_at(
    b: &[u8],
    p: usize,
    gt: &mut usize,
    blank: impl Fn(u8) -> bool,
    terminator: impl Fn(usize) -> usize,
) -> Option<(std::ops::Range<usize>, usize)> {
    let skip = |mut j: usize| {
        while j < b.len() && blank(b[j]) {
            j += 1;
        }
        j
    };
    let j = skip(p);
    let j = skip(j + b[j..].starts_with(b"<!--").then_some(4)?);
    let j = j + b[j..].starts_with(b"@sec").then_some(4)?;
    let s = skip(j);
    if s == j {
        return None;
    }
    if *gt < s {
        *gt = s;
    }
    while *gt < b.len() && b[*gt] != b'>' {
        *gt += 1;
    }
    let g = *gt;
    if g == b.len() || g < s + 2 || &b[g - 2..g] != b"--" {
        return None;
    }
    let e = skip(g + 1);
    (e == b.len() || terminator(e) > 0).then_some((s..g - 2, e))
}

/// `m[1].trim().split(/\s+/)` into attributes; `None` for no id.
fn parse_attrs(group: &str, line: usize) -> Option<SecMarker> {
    let (mut id, mut tags) = (None, None);
    for token in group.split(is_js_space).filter(|t| !t.is_empty()) {
        // JavaScript's `indexOf('=') > 0`: a key, then `=`.
        match token.split_once('=') {
            Some(("id", v)) => id = Some(v),
            Some(("tags", v)) => tags = Some(v),
            _ => {}
        }
    }
    let id = id.filter(|id| !id.is_empty())?;
    Some(SecMarker {
        id: id.to_owned(),
        tags: tags
            .unwrap_or("")
            .split(',')
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
            .collect(),
        line,
    })
}
