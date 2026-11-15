//! The sanitizer, in two passes.
//!
//! 1. **Raw HTML** (the page author's `<…>`, the parser's `Html` and
//!    `InlineHtml` events) is filtered before the writer writes it: each piece is
//!    tokenized by html5ever's tokenizer and rewritten from its tokens. Only
//!    the tags of [`RAW_TAGS`] pass, written canonically and with no
//!    attribute but `open` on `details`; every other tag, comment, doctype
//!    and processing instruction is dropped, and text is escaped. So `<script>
//!    x</script>` leaves `x`, and no raw tag carries an `id`, `class`, `style`,
//!    `href` or event handler. Pieces are filtered one at a time, which is
//!    why it is a token filter and not a DOM sanitizer: `<details>` and
//!    `</details>` are often separate blocks with Markdown between them, and
//!    the pairing is the final pass's job.
//! 2. **Everything** the writer and Rubrica wrote, ids included, goes through
//!    ammonia: only the elements and attributes Rubrica emits,
//!    URLs restricted to [`URL_SCHEMES`] (so `javascript:`, `vbscript:` and
//!    `data:` hrefs and srcs are removed, whatever their case, entities or
//!    leading blanks; ammonia reads a URL as the WHATWG parser does), and
//!    the result well formed.
//!
//! **Diagram SVG** is checked twice against one vocabulary ([`SVG_ELEMENTS`],
//! [`SVG_ATTRIBUTES`], [`SVG_CLASSES`] and the value grammars): [`svg_allowed`] checks each
//! SVG the renderer returns, token by token, before the writer writes it, and the final pass
//! admits the same elements, attributes, classes and values (an `attribute_filter`), so a
//! diagram passes it byte for byte. The first check exists because ammonia's lists are by
//! local name, not namespace: `a`, allowed for links, survives inside an `svg` with its
//! `href`. Page authors cannot reach either: pass 1 drops `<svg>` from raw HTML. No MathML
//! element and no `foreignObject` is in either list: with SVG `title` admitted, either would
//! open ammonia's mutation XSS (RUSTSEC-2026-0533).
//!
//! Ids are injected before the final pass and allowed on the elements that
//! carry Rubrica's ids (headings, the `@sec` span, footnotes). That is safe
//! only because of pass 1: no raw tag reaching ammonia has an attribute, so
//! every id it sees is one Rubrica wrote. The alternative, sanitizing first
//! and injecting ids after, would have to find the headings again in the
//! sanitized HTML; pass 1 is what makes the simpler order sound, and
//! `raw_html_cannot_set_an_id_or_a_class` pins it.
//!
//! **Cost bounds.** html5ever is linear in most shapes but not all, so each
//! superlinear one is bounded before it is reached, each with a finding:
//!
//! - its tokenizer checks every new attribute of a tag against the tag's
//!   earlier ones, so a tag with `k` attributes costs O(k²) (`<div a0 …
//!   a159999>`, 1.17 MB). A raw piece with a tag of more than
//!   [`MAX_RAW_ATTRIBUTES`] attributes, counted by [`max_attributes`] before
//!   tokenizing, is written as text instead;
//! - its tree builder (ammonia's) is O(depth) per block start tag: raw
//!   elements deeper than 100 are dropped here, Markdown
//!   containers in the formatter;
//! - its serializer (ammonia's output) scans from each `&`, NBSP or other
//!   `0xC2`-led character to the end of the text or attribute value for the
//!   next `<` or `>`, so a text of `n` such characters costs O(n²) (1 MB of
//!   `&`). [`bound_serializer_cost`] splits long text runs with empty
//!   comments, which ammonia removes without merging the text back, and drops
//!   an attribute whose value would cost more than [`MAX_ATTRIBUTE_COST`].
//!
//! Long attribute values, long tag names, long comments, doctypes and bogus
//! comments, and many tags in one piece are linear in html5ever (1 MB of
//! each; `tests/quadratic.rs`). A page body over
//! [`crate::markdown::MAX_PAGE_BYTES`] is not rendered at all.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use html5ever::tokenizer::{
    BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};

use crate::sec::RenderFinding;

/// The raw HTML tags that pass: `<br>` and the
/// inline and disclosure tags Markdown has no syntax for.
pub const RAW_TAGS: &[&str] = &["br", "sub", "sup", "kbd", "details", "summary"];

/// The URL schemes a link or image may have; relative URLs pass. Wiki.js's
/// DOMPurify also allows `ftp`, `ftps`, `tel`, `callto`, `sms`, `cid`,
/// `xmpp`, `matrix` and, on images, `data:`; the corpus uses `http`,
/// `https` and `mailto` only.
pub const URL_SCHEMES: &[&str] = &["http", "https", "mailto"];

/// The most raw elements open at once (across the page's raw pieces, in
/// document order). A deeper start tag is dropped, and so is the end tag
/// that would close it: the final pass's tree builder costs O(depth) per
/// block start tag (`<details>` 100,000 deep: seconds), so depth is bounded
/// here as the formatter bounds Markdown's containers. An element left open
/// (`<sub>` with no `</sub>`; the final pass closes it at its block's end)
/// stays open here, so a page with 100 of them keeps no later raw tag; the
/// first tag dropped is a [`RenderFinding::RawHtmlTooDeep`].
pub(crate) const MAX_RAW_DEPTH: usize = 100;

/// The most attributes a tag in a raw piece may have. html5ever's tokenizer
/// costs O(k²) on a tag of `k` attributes; with `k` at most 256 a piece of
/// `n` bytes costs at most 128 comparisons per byte (a 1 MB piece of tags of
/// 256 attributes). No raw tag Rubrica keeps has an
/// attribute but `open`, and no page of the corpus has a raw tag at all, so
/// the bound only ever meets hostile or broken input: the piece is written
/// as escaped text and a [`RenderFinding::RawHtmlTooManyAttributes`] says so.
pub const MAX_RAW_ATTRIBUTES: usize = 256;

/// The raw elements open so far on the page, as the filter wrote them.
#[derive(Debug, Default)]
pub(crate) struct RawState {
    /// Written elements still open, innermost last (at most
    /// [`MAX_RAW_DEPTH`]).
    open: Vec<&'static str>,
    /// Start tags dropped for depth and not yet closed, per name.
    dropped: HashMap<&'static str, usize>,
    /// The line of the piece being filtered.
    line: usize,
    /// What the filter found: a piece escaped for its attributes, and the
    /// first start tag dropped for depth (once per page).
    pub(crate) findings: Vec<RenderFinding>,
    deep_reported: bool,
}

/// Writes the tokens of one raw HTML piece back out, keeping only
/// [`RAW_TAGS`].
struct RawFilter<'s> {
    out: RefCell<String>,
    state: RefCell<&'s mut RawState>,
}

impl TokenSink for RawFilter<'_> {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut out = self.out.borrow_mut();
        match token {
            Token::TagToken(tag) => {
                let Some(&name) = RAW_TAGS.iter().find(|t| **t == &*tag.name) else {
                    return TokenSinkResult::Continue;
                };
                let mut state = self.state.borrow_mut();
                match tag.kind {
                    TagKind::StartTag if name != "br" => {
                        if state.open.len() >= MAX_RAW_DEPTH {
                            *state.dropped.entry(name).or_default() += 1;
                            if !state.deep_reported {
                                state.deep_reported = true;
                                let line = state.line;
                                state.findings.push(RenderFinding::RawHtmlTooDeep { line });
                            }
                            return TokenSinkResult::Continue;
                        }
                        state.open.push(name);
                        out.push('<');
                        out.push_str(name);
                        if name == "details" && tag.attrs.iter().any(|a| &*a.name.local == "open") {
                            out.push_str(" open");
                        }
                        out.push('>');
                    }
                    TagKind::StartTag => out.push_str("<br>"),
                    // `</br>` is a `<br>` to a browser; one written as an end
                    // tag is dropped instead.
                    TagKind::EndTag if name != "br" => {
                        // Closes a dropped start tag first (they are the
                        // innermost), then the innermost written one; an end
                        // tag that closes nothing is dropped.
                        if let Some(n) = state.dropped.get_mut(name).filter(|n| **n > 0) {
                            *n -= 1;
                            return TokenSinkResult::Continue;
                        }
                        let Some(at) = state.open.iter().rposition(|n| *n == name) else {
                            return TokenSinkResult::Continue;
                        };
                        state.open.remove(at);
                        out.push_str("</");
                        out.push_str(name);
                        out.push('>');
                    }
                    TagKind::EndTag => {}
                }
            }
            Token::CharacterTokens(text) => escape_text(&mut out, &text),
            // Comments (an `@sec` marker that is not one, too), doctypes,
            // NULs, parse errors.
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

fn escape_text(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

/// Pass 1 on one raw HTML piece (a block's or an inline's literal), which
/// starts on `line`.
pub(crate) fn filter_raw_html(raw: &str, line: usize, state: &mut RawState) -> String {
    if max_attributes(raw) > MAX_RAW_ATTRIBUTES {
        state
            .findings
            .push(RenderFinding::RawHtmlTooManyAttributes { line });
        let mut out = String::with_capacity(raw.len() + raw.len() / 8);
        escape_text(&mut out, raw);
        return out;
    }
    state.line = line;
    let sink = RawFilter {
        out: RefCell::new(String::with_capacity(raw.len())),
        state: RefCell::new(state),
    };
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(raw.into());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.out.into_inner()
}

/// The most attributes html5ever's tokenizer starts on any one tag of `raw`
/// (a start or an end tag, closed or cut off by the end of the piece): what
/// its duplicate-attribute check costs, O(k²) in that number `k`. Linear and
/// allocation-free, so it runs before the tokenizer does.
///
/// It follows the WHATWG tokenizer's states for what can hide a `<` or a
/// `>` (html5ever is that tokenizer; Rubrica's sink never switches it into
/// RAWTEXT or script states): tags with their quoted and unquoted values,
/// comments (`<!--` to `-->` or `--!>`, `<!-->` and `<!--->` empty, `--!-`
/// back to comment end dash, so `--!->` ends nothing), doctypes,
/// `<![CDATA[` outside foreign content, `<?` and `</` + a non-letter (bogus
/// comments, to `>`). `tag_scan_matches_html5ever` checks it against the
/// tokenizer on random pieces, and
/// `tag_scan_matches_html5ever_on_walks_through_every_state` on pieces
/// steered through every comment, tag and attribute state's transitions.
pub fn max_attributes(raw: &str) -> usize {
    let mut max = 0;
    scan_tags(raw.as_bytes(), |_, _| {}, |count, _| max = max.max(count));
    max
}

/// The tag walk of [`max_attributes`]: `on_attr(start, end)` for each
/// attribute name's bytes, `on_tag(attributes, closed)` at each tag's end.
fn scan_tags(b: &[u8], mut on_attr: impl FnMut(usize, usize), mut on_tag: impl FnMut(usize, bool)) {
    #[derive(Clone, Copy)]
    enum St {
        TagName,
        BeforeName,
        Name(usize),
        AfterName,
        BeforeValue,
        Quoted(u8),
        Unquoted,
        AfterQuoted,
        SelfClosing,
    }
    // The tokenizer reads `\r` as `\n`.
    let ws = |c: u8| matches!(c, b'\t' | b'\n' | b'\x0c' | b'\r' | b' ');
    let to_gt = |i: usize| memchr_gt(b, i);
    let n = b.len();
    let mut i = 0;
    while i < n {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        i += 1;
        let Some(&c) = b.get(i) else { break };
        match c {
            b'!' => {
                i += 1;
                i = if b[i..].starts_with(b"--") {
                    skip_comment(b, i + 2)
                } else {
                    // A doctype ends at the first `>` in every one of its
                    // states; CDATA outside foreign content and the rest
                    // are bogus comments.
                    to_gt(i)
                };
                continue;
            }
            b'?' => {
                i = to_gt(i);
                continue;
            }
            b'/' => match b.get(i + 1) {
                Some(c) if c.is_ascii_alphabetic() => i += 1,
                Some(b'>') => {
                    i += 2;
                    continue;
                }
                Some(_) => {
                    i = to_gt(i + 1);
                    continue;
                }
                None => break,
            },
            c if c.is_ascii_alphabetic() => {}
            // `<` as text; `c` is read again in the data state.
            _ => continue,
        }
        // `b[i]` is the tag name's first letter.
        let mut st = St::TagName;
        let mut count = 0;
        let mut closed = false;
        while i < n {
            let c = b[i];
            match st {
                St::TagName => match c {
                    c if ws(c) => st = St::BeforeName,
                    b'/' => st = St::SelfClosing,
                    b'>' => closed = true,
                    _ => {}
                },
                St::BeforeName => match c {
                    c if ws(c) => {}
                    b'/' | b'>' => {
                        st = St::AfterName;
                        continue;
                    }
                    // `=` here starts a name with it.
                    _ => {
                        count += 1;
                        st = St::Name(i);
                    }
                },
                St::Name(start) => match c {
                    c if ws(c) || c == b'/' || c == b'>' => {
                        on_attr(start, i);
                        st = St::AfterName;
                        continue;
                    }
                    b'=' => {
                        on_attr(start, i);
                        st = St::BeforeValue;
                    }
                    _ => {}
                },
                St::AfterName => match c {
                    c if ws(c) => {}
                    b'/' => st = St::SelfClosing,
                    b'=' => st = St::BeforeValue,
                    b'>' => closed = true,
                    _ => {
                        count += 1;
                        st = St::Name(i);
                    }
                },
                St::BeforeValue => match c {
                    c if ws(c) => {}
                    b'"' | b'\'' => st = St::Quoted(c),
                    b'>' => closed = true,
                    _ => {
                        st = St::Unquoted;
                        continue;
                    }
                },
                St::Quoted(q) => {
                    if c == q {
                        st = St::AfterQuoted;
                    }
                }
                St::Unquoted => match c {
                    c if ws(c) => st = St::BeforeName,
                    b'>' => closed = true,
                    _ => {}
                },
                St::AfterQuoted => match c {
                    c if ws(c) => st = St::BeforeName,
                    b'/' => st = St::SelfClosing,
                    b'>' => closed = true,
                    _ => {
                        st = St::BeforeName;
                        continue;
                    }
                },
                St::SelfClosing => match c {
                    b'>' => closed = true,
                    _ => {
                        st = St::BeforeName;
                        continue;
                    }
                },
            }
            i += 1;
            if closed {
                break;
            }
        }
        if let (St::Name(start), false) = (st, closed) {
            on_attr(start, n);
        }
        on_tag(count, closed);
    }
}

/// The index after the first `>` at or after `i`, or the end.
fn memchr_gt(b: &[u8], i: usize) -> usize {
    b[i.min(b.len())..]
        .iter()
        .position(|&c| c == b'>')
        .map_or(b.len(), |at| i + at + 1)
}

/// The index after the comment that starts at `i` (just after `<!--`), by the
/// WHATWG comment states; the less-than-sign states only report nesting, so
/// they end a comment exactly where the plain states do.
fn skip_comment(b: &[u8], mut i: usize) -> usize {
    #[derive(Clone, Copy)]
    enum C {
        Start,
        StartDash,
        Body,
        EndDash,
        End,
        EndBang,
    }
    let mut st = C::Start;
    while i < b.len() {
        let c = b[i];
        st = match (st, c) {
            (C::Start | C::StartDash | C::End | C::EndBang, b'>') => return i + 1,
            (C::Start, b'-') => C::StartDash,
            (C::StartDash | C::EndDash, b'-') => C::End,
            (C::Body, b'-') => C::EndDash,
            (C::End, b'!') => C::EndBang,
            (C::End, b'-') => C::End,
            // `--!-` is comment end dash, not end: `--!->` closes nothing.
            (C::EndBang, b'-') => C::EndDash,
            _ => C::Body,
        };
        i += 1;
    }
    b.len()
}

/// The longest text run, in bytes of the written HTML, [`bound_serializer_cost`]
/// leaves unsplit. The serializer then scans at most this far from each
/// special character in text (a page of `n` bytes: at most `n` × 1024 bytes
/// scanned). 1 MB of `&` renders within the bound; most of
/// the cost is parsing and writing its 5 MB of `&amp;`, not the scan (1 MB of
/// `°`, as special but written as itself, is the comparison).
pub const TEXT_RUN: usize = 1024;

/// The most an attribute value may cost the serializer: its special
/// characters (`&`, and `0xC2`-led ones such as NBSP and `°`) times its
/// length. Over it the attribute is dropped
/// ([`RenderFinding::AttributesTooCostly`]). 2²² admits a 16 KB URL with 256
/// `&`s; the worst page of `n` bytes then costs at most `n` × 2,048 bytes
/// scanned.
pub const MAX_ATTRIBUTE_COST: usize = 1 << 22;

/// What the serializer spends on an attribute value, as written (escaped):
/// its special characters times its length, both counted as the serializer
/// sees the value, each character reference one character.
pub(crate) fn attribute_cost(value: &[u8]) -> usize {
    let (mut specials, mut len, mut i) = (0usize, 0usize, 0);
    while i < value.len() {
        match value[i] {
            b'&' => {
                specials += 1;
                // The reference: `&`, then letters, digits or `#`, then `;`.
                let body = value[i + 1..]
                    .iter()
                    .take_while(|c| c.is_ascii_alphanumeric() || **c == b'#')
                    .count();
                i += 1 + body + usize::from(value.get(i + 1 + body) == Some(&b';'));
            }
            c => {
                specials += usize::from(c == 0xC2);
                i += 1;
            }
        }
        len += 1;
    }
    specials.saturating_mul(len)
}

/// An attribute [`bound_serializer_cost`] left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedAttribute {
    /// Its element's name, as written.
    pub element: String,
    /// Its name, as written.
    pub attribute: String,
    /// The byte offset of its tag in the HTML.
    pub at: usize,
}

/// Rewrites the HTML Rubrica wrote (the writer's and its own, pass 1's included:
/// text escaped, attribute values quoted) so ammonia's serializer stays
/// linear: an empty comment (`<!---->`, removed by ammonia, which keeps the
/// text on either side as separate nodes, written back to back) after every
/// [`TEXT_RUN`] bytes of a text run, never inside a character reference, a
/// UTF-8 sequence or a `\r\n` (which html5ever reads as one line end, and
/// `\r<!---->\n` as two); an attribute whose value costs more than
/// [`MAX_ATTRIBUTE_COST`] left out. Returns the HTML and the attributes left
/// out. HTML with neither is returned unchanged, byte for byte.
pub fn bound_serializer_cost(html: &str) -> (Cow<'_, str>, Vec<DroppedAttribute>) {
    let b = html.as_bytes();
    let ws = |c: u8| matches!(c, b'\t' | b'\n' | b'\x0c' | b'\r' | b' ');
    let mut out: Option<String> = None;
    // `html[..copied]` is in `out`, when there is one.
    let mut copied = 0;
    let mut dropped = Vec::new();
    let (mut run, mut in_ref) = (0, false);
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'<' && b[i..].starts_with(b"<!--") {
            // A comment (Rubrica writes none) is copied whole.
            i = b[i + 4..]
                .windows(3)
                .position(|w| w == b"-->")
                .map_or(b.len(), |at| i + 4 + at + 3);
            (run, in_ref) = (0, false);
            continue;
        }
        if c == b'<'
            && b.get(i + 1)
                .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'/')
        {
            (run, in_ref) = (0, false);
            // The tag's name, then each attribute, its value's cost checked.
            let tag = i;
            let mut j = i + 1 + usize::from(b[i + 1] == b'/');
            let name = j;
            while j < b.len() && !ws(b[j]) && !matches!(b[j], b'>' | b'/') {
                j += 1;
            }
            let name = name..j;
            loop {
                let attr_start = j;
                while j < b.len() && (ws(b[j]) || b[j] == b'/') {
                    j += 1;
                }
                if j >= b.len() || b[j] == b'>' {
                    break;
                }
                let key = j;
                while j < b.len() && !ws(b[j]) && !matches!(b[j], b'=' | b'>' | b'/') {
                    j += 1;
                }
                let key = key..j;
                let (mut v0, mut v1) = (j, j);
                if b.get(j) == Some(&b'=') {
                    j += 1;
                    if let Some(&q @ (b'"' | b'\'')) = b.get(j) {
                        v0 = j + 1;
                        v1 = b[v0..]
                            .iter()
                            .position(|&c| c == q)
                            .map_or(b.len(), |at| v0 + at);
                        j = (v1 + 1).min(b.len());
                    } else {
                        v0 = j;
                        while j < b.len() && !ws(b[j]) && b[j] != b'>' {
                            j += 1;
                        }
                        v1 = j;
                    }
                }
                if attribute_cost(&b[v0..v1]) > MAX_ATTRIBUTE_COST {
                    let o = out.get_or_insert_with(|| String::with_capacity(html.len()));
                    o.push_str(&html[copied..attr_start]);
                    copied = j;
                    dropped.push(DroppedAttribute {
                        element: html[name.clone()].to_owned(),
                        attribute: html[key].to_owned(),
                        at: tag,
                    });
                }
            }
            i = (j + 1).min(b.len());
            continue;
        }
        // Text. Split before this byte when the run is long, the byte starts
        // a character, no character reference is open (Rubrica's text is
        // escaped, so each `&` opens one; any byte that cannot continue one
        // ends it) and the byte before is no `\r`.
        if run >= TEXT_RUN && !in_ref && (c & 0xC0) != 0x80 && b[i - 1] != b'\r' {
            let o = out.get_or_insert_with(|| String::with_capacity(html.len() + html.len() / 64));
            o.push_str(&html[copied..i]);
            o.push_str("<!---->");
            copied = i;
            run = 0;
        }
        if c == b'&' {
            in_ref = true;
        } else if in_ref && !(c.is_ascii_alphanumeric() || c == b'#') {
            in_ref = false;
        }
        run += 1;
        i += 1;
    }
    match out {
        None => (Cow::Borrowed(html), dropped),
        Some(mut o) => {
            o.push_str(&html[copied..]);
            (Cow::Owned(o), dropped)
        }
    }
}

/// The classes a highlighter may leave on a `span`: the top-level TextMate scope names the
/// theme styles (`.hl-*` in the theme's stylesheet, ten rules). The final pass allows exactly
/// these plus `sec` on `span`; `rubrica-site`'s highlighter filters syntect's output down to
/// them first.
pub const HL_CLASSES: [&str; 10] = [
    "hl-comment",
    "hl-keyword",
    "hl-string",
    "hl-constant",
    "hl-entity",
    "hl-storage",
    "hl-support",
    "hl-variable",
    "hl-punctuation",
    "hl-meta",
];

/// The elements a diagram's SVG may hold.
pub const SVG_ELEMENTS: [&str; 7] = ["svg", "g", "path", "rect", "text", "title", "desc"];

/// Per element: the attributes it may carry (lowercase, as the tokenizer gives them).
pub const SVG_ATTRIBUTES: [(&str, &[&str]); 7] = [
    (
        "svg",
        &[
            "role",
            "viewbox",
            "width",
            "height",
            "class",
            "font-family",
            "font-size",
        ],
    ),
    ("g", &["class"]),
    ("path", &["class", "d"]),
    ("rect", &["class", "x", "y", "width", "height", "rx", "ry"]),
    ("text", &["class", "x", "y", "text-anchor"]),
    ("title", &[]),
    ("desc", &[]),
];

/// Per element: the classes its `class` may name (one or two of them; the `svg`'s is `rd`
/// and exactly one of the other three, in that order).
pub const SVG_CLASSES: [(&str, &[&str]); 5] = [
    ("svg", &["rd", "rd-flowchart", "rd-sequence", "rd-state"]),
    (
        "g",
        &[
            "rd-clusters",
            "rd-edges",
            "rd-nodes",
            "rd-labels",
            "rd-node",
            "rd-cluster",
            "rd-edge",
            "rd-edge-solid",
            "rd-edge-dotted",
            "rd-edge-thick",
            "rd-edge-label",
            "rd-actors",
            "rd-actor",
            "rd-lifelines",
            "rd-activations",
            "rd-messages",
            "rd-notes",
            "rd-note",
            "rd-frames",
            "rd-frame",
        ],
    ),
    (
        "rect",
        &[
            "rd-shape",
            "rd-label-bg",
            "rd-cluster-box",
            "rd-actor-box",
            "rd-activation",
            "rd-note-box",
            "rd-frame-box",
            "rd-frame-tag",
            "rd-start",
            "rd-end-ring",
            "rd-end-dot",
        ],
    ),
    (
        "path",
        &[
            "rd-shape",
            "rd-shape-line",
            "rd-line",
            "rd-arrow",
            "rd-lifeline",
            "rd-frame-divider",
        ],
    ),
    (
        "text",
        &[
            "rd-text",
            "rd-cluster-title",
            "rd-frame-label",
            "rd-frame-text",
        ],
    ),
];

/// Whether `svg` is exactly one `<svg>` element in the diagram vocabulary. The `Err` is the
/// first reason, for the finding (names lowercase, as tokenized).
///
/// One pass of html5ever's tokenizer (which never switches to RCDATA here: that is the tree
/// builder's job, so `<title>`'s content arrives as tags and text), O(1) work per token and
/// attribute, and O(1) state: only `g` nests, so the open elements are the `svg`, a count of
/// `g`s and at most one leaf.
pub fn svg_allowed(svg: &str) -> Result<(), String> {
    let tokenizer = Tokenizer::new(SvgCheck::default(), TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(svg.into());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    match tokenizer.sink.error.into_inner() {
        Some(why) => Err(why),
        None => Ok(()),
    }
}

/// [`svg_allowed`]'s sink: the open elements and the first violation.
#[derive(Default)]
struct SvgCheck {
    state: RefCell<SvgState>,
    error: RefCell<Option<String>>,
}

#[derive(Default)]
struct SvgState {
    /// The `svg` start tag has been seen.
    started: bool,
    /// Its end tag has been seen.
    done: bool,
    /// How many `g`s are open.
    g_depth: usize,
    /// The open element that holds no element: `title`, `desc`, `rect`, `path` or `text`.
    leaf: Option<&'static str>,
    /// How many children of the `svg` have started.
    children: usize,
}

impl SvgState {
    /// The innermost open element, if any.
    fn innermost(&self) -> Option<&'static str> {
        if self.leaf.is_some() {
            self.leaf
        } else if self.g_depth > 0 {
            Some("g")
        } else if self.started && !self.done {
            Some("svg")
        } else {
            None
        }
    }
}

const NOT_ONE_SVG: &str = "not one `<svg>` element";
const TITLE_AND_DESC_FIRST: &str = "`<title>` and `<desc>` must open the `<svg>`";

impl SvgCheck {
    fn token(&self, token: Token) -> Result<(), String> {
        let mut st = self.state.borrow_mut();
        match token {
            Token::TagToken(tag) => match tag.kind {
                TagKind::StartTag => {
                    let Some(parent) = st.innermost() else {
                        if st.started || &*tag.name != "svg" {
                            return Err(NOT_ONE_SVG.into());
                        }
                        svg_attributes_allowed("svg", &tag.attrs)?;
                        if tag.self_closing {
                            return Err("a self-closing `<svg>`".into());
                        }
                        st.started = true;
                        return Ok(());
                    };
                    let Some(&name) = SVG_ELEMENTS.iter().find(|e| **e == &*tag.name) else {
                        return Err(format!("`<{}>` is not allowed", &*tag.name));
                    };
                    let parent_ok = match name {
                        "title" | "desc" => parent == "svg",
                        "g" => parent == "svg" || parent == "g",
                        "rect" | "path" | "text" => parent == "g",
                        _ => false,
                    };
                    if !parent_ok {
                        return Err(format!("`<{name}>` inside `<{parent}>`"));
                    }
                    if parent == "svg" {
                        let want = match st.children {
                            0 => "title",
                            1 => "desc",
                            _ => "g",
                        };
                        st.children += 1;
                        if name != want {
                            return Err(TITLE_AND_DESC_FIRST.into());
                        }
                    }
                    if tag.self_closing {
                        return Err(format!("a self-closing `<{name}>`"));
                    }
                    svg_attributes_allowed(name, &tag.attrs)?;
                    if name == "g" {
                        st.g_depth += 1;
                    } else {
                        st.leaf = Some(name);
                    }
                }
                TagKind::EndTag => {
                    let Some(open) = st.innermost() else {
                        return Err(NOT_ONE_SVG.into());
                    };
                    if &*tag.name != open {
                        return Err(format!("`</{}>` does not close `<{open}>`", &*tag.name));
                    }
                    if st.leaf.is_some() {
                        st.leaf = None;
                    } else if st.g_depth > 0 {
                        st.g_depth -= 1;
                    } else if st.children < 2 {
                        return Err(TITLE_AND_DESC_FIRST.into());
                    } else {
                        st.done = true;
                    }
                }
            },
            Token::CharacterTokens(_) | Token::NullCharacterToken => match st.innermost() {
                Some("text" | "title" | "desc") => {}
                Some(_) => return Err("text outside `<text>`, `<title>` and `<desc>`".into()),
                None => return Err(NOT_ONE_SVG.into()),
            },
            Token::CommentToken(_) | Token::DoctypeToken(_) => {
                return Err("a comment, doctype or processing instruction".into());
            }
            Token::EOFToken if !st.done => return Err(NOT_ONE_SVG.into()),
            // A duplicate attribute (dropped, the first kept, as a browser does) and the
            // like: the tokens say what a browser builds.
            Token::EOFToken | Token::ParseError(_) => {}
        }
        Ok(())
    }
}

impl TokenSink for SvgCheck {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        if self.error.borrow().is_none() {
            if let Err(why) = self.token(token) {
                *self.error.borrow_mut() = Some(why);
            }
        }
        TokenSinkResult::Continue
    }
}

/// The attributes of one start tag of `element` (in [`SVG_ELEMENTS`]): each in its list, each
/// value in its grammar, the required ones present, `rx` and `ry` together.
fn svg_attributes_allowed(element: &str, attrs: &[html5ever::Attribute]) -> Result<(), String> {
    let allowed = SVG_ATTRIBUTES
        .iter()
        .find(|(e, _)| *e == element)
        .map_or(&[][..], |(_, a)| a);
    // Bit `i`: the attribute `allowed[i]` is present.
    let mut present = 0u32;
    for attr in attrs {
        let name = &*attr.name.local;
        let Some(at) = allowed.iter().position(|a| *a == name) else {
            return Err(format!("`{name}` on `<{element}>` is not allowed"));
        };
        present |= 1 << at;
        if name == "class" {
            svg_classes_allowed(element, &attr.value)?;
        } else if !svg_value_allowed(element, name, &attr.value) {
            return Err(format!("a bad `{name}` value on `<{element}>`"));
        }
    }
    let has = |a: &str| {
        allowed
            .iter()
            .position(|x| *x == a)
            .is_some_and(|at| present & (1 << at) != 0)
    };
    let required: &[&str] = match element {
        "rect" => &["class", "x", "y", "width", "height"],
        "title" | "desc" => &[],
        _ => allowed,
    };
    if let Some(missing) = required.iter().find(|a| !has(a)) {
        return Err(format!("`<{element}>` lacks `{missing}`"));
    }
    match (has("rx"), has("ry")) {
        (true, false) => Err(format!("`rx` without `ry` on `<{element}>`")),
        (false, true) => Err(format!("`ry` without `rx` on `<{element}>`")),
        _ => Ok(()),
    }
}

/// A `class` value of `element`: one or two names from its [`SVG_CLASSES`] list, separated
/// by one space, none before or after; the `svg`'s is `rd` and then exactly one of
/// `rd-flowchart`, `rd-sequence` and `rd-state`.
fn svg_classes_allowed(element: &str, value: &str) -> Result<(), String> {
    let bad = || format!("a bad `class` value on `<{element}>`");
    if element == "svg" {
        return match value.strip_prefix("rd ") {
            Some("rd-flowchart" | "rd-sequence" | "rd-state") => Ok(()),
            _ => Err(bad()),
        };
    }
    let allowed = SVG_CLASSES
        .iter()
        .find(|(e, _)| *e == element)
        .map_or(&[][..], |(_, c)| c);
    let (first, second) = match value.split_once(' ') {
        Some((a, b)) => (a, Some(b)),
        None => (value, None),
    };
    if first.is_empty() || second.is_some_and(|b| b.is_empty() || b.contains(' ')) {
        return Err(bad());
    }
    for class in std::iter::once(first).chain(second) {
        if !allowed.contains(&class) {
            return Err(format!(
                "the class `{class}` is not allowed on `<{element}>`"
            ));
        }
    }
    Ok(())
}

/// Whether `value` is in the grammar of `element`'s `attribute` (lowercase, as tokenized;
/// not `class`, which [`svg_classes_allowed`] checks). Every other pair is refused.
fn svg_value_allowed(element: &str, attribute: &str, value: &str) -> bool {
    match (element, attribute) {
        ("svg", "role") => value == "img",
        ("svg", "viewbox") => is_view_box(value),
        ("svg", "font-family") => value == "Inter, sans-serif",
        ("svg", "font-size") => value == "14",
        ("svg", "width" | "height")
        | ("rect", "x" | "y" | "width" | "height" | "rx" | "ry")
        | ("text", "x" | "y") => is_number(value),
        ("path", "d") => is_path(value),
        ("text", "text-anchor") => value == "middle" || value == "start",
        _ => false,
    }
}

/// `^[0-9]{1,6}\.[0-9]{2}$`: a length, with exactly two decimals.
fn is_number(v: &str) -> bool {
    number_end(v.as_bytes(), 0) == Some(v.len())
}

/// `^0 0 N N$`, `N` as in [`is_number`].
fn is_view_box(v: &str) -> bool {
    let b = v.as_bytes();
    b.starts_with(b"0 0 ") && pair_end(b, 4) == Some(b.len())
}

/// `^M N N( (L N N|M N N|H N|V N|Z))*$`, `N` as in [`is_number`]: one space before each
/// command letter, the letter directly before its first number, one space between a
/// command's two numbers (`M16.00 20.00 L30.00 40.00 Z`).
fn is_path(v: &str) -> bool {
    let b = v.as_bytes();
    if b.first() != Some(&b'M') {
        return false;
    }
    let Some(mut i) = pair_end(b, 1) else {
        return false;
    };
    while i < b.len() {
        if b[i] != b' ' {
            return false;
        }
        let next = match b.get(i + 1) {
            Some(b'L' | b'M') => pair_end(b, i + 2),
            Some(b'H' | b'V') => number_end(b, i + 2),
            Some(b'Z') => Some(i + 2),
            _ => None,
        };
        let Some(next) = next else {
            return false;
        };
        i = next;
    }
    true
}

/// The end of `N N` (`[0-9]{1,6}\.[0-9]{2} [0-9]{1,6}\.[0-9]{2}`) starting at `i` of `b`.
fn pair_end(b: &[u8], i: usize) -> Option<usize> {
    let j = number_end(b, i)?;
    if b.get(j) != Some(&b' ') {
        return None;
    }
    number_end(b, j + 1)
}

/// The end of `[0-9]{1,6}\.[0-9]{2}` starting at `i` of `b`; the byte after it is not looked
/// at.
fn number_end(b: &[u8], i: usize) -> Option<usize> {
    let digits = b
        .get(i..)?
        .iter()
        .take(7)
        .take_while(|c| c.is_ascii_digit())
        .count();
    if !(1..=6).contains(&digits) {
        return None;
    }
    let j = i + digits;
    let decimals = b.get(j + 1..j + 3)?;
    (b[j] == b'.' && decimals.iter().all(u8::is_ascii_digit)).then_some(j + 3)
}

/// The final pass's configuration: what the writer and Rubrica write, nothing
/// else.
static FINAL: LazyLock<ammonia::Builder<'static>> = LazyLock::new(|| {
    let tags: HashSet<&str> = [
        // Blocks.
        "p",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "blockquote",
        "ul",
        "ol",
        "li",
        "pre",
        "hr",
        "table",
        "thead",
        "tbody",
        "tr",
        "th",
        "td",
        "section",
        // Inlines.
        "code",
        "em",
        "strong",
        "del",
        "a",
        "img",
        "br",
        "input",
        "span",
        "sup",
    ]
    .into_iter()
    .chain(RAW_TAGS.iter().copied())
    // Diagrams: the `figure` the writer wraps one in, and the SVG vocabulary.
    .chain(["figure"])
    .chain(SVG_ELEMENTS)
    .collect();
    let attrs = |a: &[&'static str]| a.iter().copied().collect::<HashSet<_>>();
    let mut tag_attributes: HashMap<&str, HashSet<&str>> = HashMap::new();
    for h in ["h1", "h2", "h3", "h4", "h5", "h6"] {
        tag_attributes.insert(h, attrs(&["id"]));
    }
    // The `@sec` anchor's id. A span's classes are an allow-list below (ammonia refuses a
    // tag that is in both places).
    tag_attributes.insert("span", attrs(&["id"]));
    // `language-x`; `mermaid`.
    tag_attributes.insert("code", attrs(&["class"]));
    tag_attributes.insert("pre", attrs(&["class"]));
    tag_attributes.insert(
        "a",
        attrs(&[
            "href",
            "title",
            // Footnotes: the reference's id, and the back reference.
            "id",
            "class",
            "data-footnote-ref",
            "data-footnote-backref",
            "data-footnote-backref-idx",
            "aria-label",
        ]),
    );
    tag_attributes.insert("img", attrs(&["src", "alt", "title"]));
    tag_attributes.insert("ul", attrs(&["class"]));
    tag_attributes.insert("ol", attrs(&["start", "class"]));
    tag_attributes.insert("li", attrs(&["id", "class"]));
    tag_attributes.insert("sup", attrs(&["class"]));
    tag_attributes.insert("section", attrs(&["class", "data-footnotes"]));
    tag_attributes.insert("th", attrs(&["align"]));
    tag_attributes.insert("td", attrs(&["align"]));
    tag_attributes.insert("input", attrs(&["class", "checked", "disabled", "type"]));
    tag_attributes.insert("details", attrs(&["open"]));
    // The SVG vocabulary's attributes but `class` (an allow-list below), named as the tree
    // builder names them: `viewBox`, not the tokenizer's `viewbox`.
    for (element, names) in SVG_ATTRIBUTES {
        let names: HashSet<&str> = names
            .iter()
            .filter(|a| **a != "class")
            .map(|a| if *a == "viewbox" { "viewBox" } else { a })
            .collect();
        if !names.is_empty() {
            tag_attributes.insert(element, names);
        }
    }

    // `sec` is the anchor `heading` writes; the rest are the highlighter's. ammonia rewrites
    // a span's class to this subset; the highlighter's output is already inside it.
    let mut span_classes: HashSet<&str> = HL_CLASSES.into_iter().collect();
    span_classes.insert("sec");
    let mut classes = HashMap::from([("span", span_classes), ("figure", attrs(&["diagram"]))]);
    for (element, names) in SVG_CLASSES {
        classes.insert(element, attrs(names));
    }

    let mut b = ammonia::Builder::empty();
    // `empty()` empties the tags only: its generic attributes, `lang` and
    // `title` on every element, are emptied here, so the lists above are
    // the whole allowlist (`a[title]` was redundant until then).
    b.tags(tags)
        .generic_attributes(HashSet::new())
        .tag_attributes(tag_attributes)
        .allowed_classes(classes)
        // The SVG vocabulary's value grammars; ammonia's lists only name attributes. A
        // `figure`'s class is the diagram's or none (ammonia would leave `class=""`).
        .attribute_filter(|element, attribute, value| {
            let keep = match (element, attribute) {
                ("figure", "class") => value == "diagram",
                ("svg" | "rect" | "path" | "text", "class") => true,
                ("svg", "viewBox") => svg_value_allowed("svg", "viewbox", value),
                ("svg" | "rect" | "path" | "text", _) => {
                    svg_value_allowed(element, attribute, value)
                }
                _ => true,
            };
            keep.then_some(Cow::Borrowed(value))
        })
        .url_schemes(URL_SCHEMES.iter().copied().collect())
        .url_relative(ammonia::UrlRelative::PassThrough)
        // Rubrica writes no `target`, so `noopener` would change nothing;
        // ammonia's default `noreferrer` would also strip the Referer from
        // every internal link.
        .link_rel(None)
        .strip_comments(true)
        .clean_content_tags(["script", "style"].into_iter().collect());
    b
});

/// Pass 2: the whole rendered body, its serializer cost bounded first
/// ([`bound_serializer_cost`]). Returns the HTML and the attributes left out
/// for their cost.
pub(crate) fn clean(html: &str) -> (String, Vec<DroppedAttribute>) {
    let (html, dropped) = bound_serializer_cost(html);
    (FINAL.clean(&html).to_string(), dropped)
}

/// The final pass alone, for tests of what it keeps: `clean`'s output without the dropped
/// attributes. Not for rendering: `render` is the one path.
#[doc(hidden)]
pub fn final_pass(html: &str) -> String {
    clean(html).0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// html5ever's tags of a piece: (end tag, attribute names), in order.
    fn html5ever_tags(raw: &str) -> Vec<(bool, Vec<String>)> {
        struct Tags(RefCell<Vec<(bool, Vec<String>)>>);
        impl TokenSink for Tags {
            type Handle = ();
            fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
                if let Token::TagToken(tag) = token {
                    self.0.borrow_mut().push((
                        tag.kind == TagKind::EndTag,
                        tag.attrs.iter().map(|a| a.name.local.to_string()).collect(),
                    ));
                }
                TokenSinkResult::Continue
            }
        }
        let tokenizer = Tokenizer::new(Tags(RefCell::default()), TokenizerOpts::default());
        let input = BufferQueue::default();
        input.push_back(raw.into());
        let _ = tokenizer.feed(&input);
        tokenizer.end();
        tokenizer.sink.0.into_inner()
    }

    /// [`scan_tags`]'s closed tags: their attribute names, lowercased and
    /// the first of each name kept (as html5ever does), and the most
    /// attributes any tag started, closed or not.
    fn scanned(raw: &str) -> (Vec<Vec<String>>, usize) {
        let names = RefCell::new(Vec::new());
        let mut tags = Vec::new();
        let mut max = 0;
        scan_tags(
            raw.as_bytes(),
            // html5ever reads a NUL in a name as U+FFFD.
            |a, b| {
                names
                    .borrow_mut()
                    .push(raw[a..b].to_ascii_lowercase().replace('\0', "\u{fffd}"))
            },
            |count, closed| {
                let mut seen = Vec::new();
                for n in names.borrow_mut().drain(..) {
                    if !seen.contains(&n) {
                        seen.push(n);
                    }
                }
                if closed {
                    tags.push(seen);
                }
                max = max.max(count);
            },
        );
        (tags, max)
    }

    #[test]
    fn tag_scan_matches_html5ever() {
        // Pieces from the characters every tag, value and comment state
        // turns on.
        const PIECES: &[&str] = &[
            "<",
            ">",
            "/",
            "!",
            "-",
            "--",
            "?",
            "=",
            "\"",
            "'",
            " ",
            "\t",
            "\n",
            "\r",
            "a",
            "b",
            "ab",
            "<a",
            "</b",
            "<!--",
            "-->",
            "--!>",
            "--!-",
            "<!-- --!-> ",
            "<!",
            "<?",
            "<!DOCTYPE",
            "<![CDATA[",
            "x",
            "=\"v\"",
            "='v'",
            "<p ",
            "<sub",
            "é",
            "&amp;",
        ];
        let mut seed = 0x2545_f491_u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as usize
        };
        for _ in 0..200_000 {
            let len = 1 + next() % 24;
            let raw: String = (0..len).map(|_| PIECES[next() % PIECES.len()]).collect();
            let want = html5ever_tags(&raw);
            let (got, _) = scanned(&raw);
            let want: Vec<Vec<String>> = want.into_iter().map(|(_, a)| a).collect();
            assert_eq!(got, want, "{raw:?}");
        }
    }

    /// The WHATWG tokenizer's states that decide where a tag, an attribute
    /// or a comment starts and ends, as the spec writes them (§13.2.5),
    /// tracked independently of [`scan_tags`] so a walk can be steered
    /// through every transition. Doctypes are read to their `>` (every
    /// doctype state ends there); `<![CDATA[` outside foreign content is a
    /// bogus comment.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum W {
        Data,
        TagOpen,
        EndTagOpen,
        TagName,
        BeforeAttrName,
        AttrName,
        AfterAttrName,
        BeforeAttrValue,
        ValueDq,
        ValueSq,
        ValueUnq,
        AfterValueQuoted,
        SelfClosing,
        Bogus,
        Doctype,
        CommentStart,
        CommentStartDash,
        Comment,
        CommentLt,
        CommentLtBang,
        CommentLtBangDash,
        CommentLtBangDashDash,
        CommentEndDash,
        CommentEnd,
        CommentEndBang,
    }

    impl W {
        /// The state after `s[i..]`'s first character, and how many bytes
        /// it took (a markup declaration reads `--`, `DOCTYPE` or
        /// `[CDATA[` at once).
        fn step(self, s: &[u8], i: usize) -> (W, usize) {
            let c = s[i];
            let ws = matches!(c, b'\t' | b'\n' | b'\x0c' | b'\r' | b' ');
            let mut st = self;
            // Reconsuming: the same character in the next state.
            loop {
                let next = match (st, c) {
                    (W::Data, b'<') => W::TagOpen,
                    (W::Data, _) => W::Data,
                    (W::TagOpen, b'!') => {
                        let rest = &s[i + 1..];
                        if rest.starts_with(b"--") {
                            return (W::CommentStart, 3);
                        }
                        if rest.len() >= 7 && rest[..7].eq_ignore_ascii_case(b"doctype") {
                            return (W::Doctype, 8);
                        }
                        W::Bogus
                    }
                    (W::TagOpen, b'/') => W::EndTagOpen,
                    (W::TagOpen, b'?') => W::Bogus,
                    (W::TagOpen, c) if c.is_ascii_alphabetic() => W::TagName,
                    (W::TagOpen, _) => {
                        st = W::Data;
                        continue;
                    }
                    (W::EndTagOpen, c) if c.is_ascii_alphabetic() => W::TagName,
                    (W::EndTagOpen, b'>') => W::Data,
                    (W::EndTagOpen, _) => W::Bogus,
                    (W::TagName, _) if ws => W::BeforeAttrName,
                    (W::TagName, b'/') => W::SelfClosing,
                    (W::TagName, b'>') => W::Data,
                    (W::TagName, _) => W::TagName,
                    (W::BeforeAttrName, _) if ws => W::BeforeAttrName,
                    (W::BeforeAttrName, b'/' | b'>') => {
                        st = W::AfterAttrName;
                        continue;
                    }
                    (W::BeforeAttrName, _) => W::AttrName,
                    (W::AttrName, _) if ws => W::AfterAttrName,
                    (W::AttrName, b'/' | b'>') => {
                        st = W::AfterAttrName;
                        continue;
                    }
                    (W::AttrName, b'=') => W::BeforeAttrValue,
                    (W::AttrName, _) => W::AttrName,
                    (W::AfterAttrName, _) if ws => W::AfterAttrName,
                    (W::AfterAttrName, b'/') => W::SelfClosing,
                    (W::AfterAttrName, b'=') => W::BeforeAttrValue,
                    (W::AfterAttrName, b'>') => W::Data,
                    (W::AfterAttrName, _) => W::AttrName,
                    (W::BeforeAttrValue, _) if ws => W::BeforeAttrValue,
                    (W::BeforeAttrValue, b'"') => W::ValueDq,
                    (W::BeforeAttrValue, b'\'') => W::ValueSq,
                    (W::BeforeAttrValue, b'>') => W::Data,
                    (W::BeforeAttrValue, _) => {
                        st = W::ValueUnq;
                        continue;
                    }
                    (W::ValueDq, b'"') | (W::ValueSq, b'\'') => W::AfterValueQuoted,
                    (W::ValueDq | W::ValueSq, _) => st,
                    (W::ValueUnq, _) if ws => W::BeforeAttrName,
                    (W::ValueUnq, b'>') => W::Data,
                    (W::ValueUnq, _) => W::ValueUnq,
                    (W::AfterValueQuoted, _) if ws => W::BeforeAttrName,
                    (W::AfterValueQuoted, b'/') => W::SelfClosing,
                    (W::AfterValueQuoted, b'>') => W::Data,
                    (W::AfterValueQuoted, _) => {
                        st = W::BeforeAttrName;
                        continue;
                    }
                    (W::SelfClosing, b'>') => W::Data,
                    (W::SelfClosing, _) => {
                        st = W::BeforeAttrName;
                        continue;
                    }
                    (W::Bogus | W::Doctype, b'>') => W::Data,
                    (W::Bogus | W::Doctype, _) => st,
                    (W::CommentStart, b'-') => W::CommentStartDash,
                    (W::CommentStart, b'>') => W::Data,
                    (W::CommentStart, _) => {
                        st = W::Comment;
                        continue;
                    }
                    (W::CommentStartDash, b'-') => W::CommentEnd,
                    (W::CommentStartDash, b'>') => W::Data,
                    (W::CommentStartDash, _) => {
                        st = W::Comment;
                        continue;
                    }
                    (W::Comment, b'<') => W::CommentLt,
                    (W::Comment, b'-') => W::CommentEndDash,
                    (W::Comment, _) => W::Comment,
                    (W::CommentLt, b'!') => W::CommentLtBang,
                    (W::CommentLt, b'<') => W::CommentLt,
                    (W::CommentLt, _) => {
                        st = W::Comment;
                        continue;
                    }
                    (W::CommentLtBang, b'-') => W::CommentLtBangDash,
                    (W::CommentLtBang, _) => {
                        st = W::Comment;
                        continue;
                    }
                    (W::CommentLtBangDash, b'-') => W::CommentLtBangDashDash,
                    (W::CommentLtBangDash, _) => {
                        st = W::CommentEndDash;
                        continue;
                    }
                    (W::CommentLtBangDashDash, _) => {
                        st = W::CommentEnd;
                        continue;
                    }
                    (W::CommentEndDash, b'-') => W::CommentEnd,
                    (W::CommentEndDash, _) => {
                        st = W::Comment;
                        continue;
                    }
                    (W::CommentEnd, b'>') => W::Data,
                    (W::CommentEnd, b'!') => W::CommentEndBang,
                    (W::CommentEnd, b'-') => W::CommentEnd,
                    (W::CommentEnd, _) => {
                        st = W::Comment;
                        continue;
                    }
                    (W::CommentEndBang, b'-') => W::CommentEndDash,
                    (W::CommentEndBang, b'>') => W::Data,
                    (W::CommentEndBang, _) => {
                        st = W::Comment;
                        continue;
                    }
                };
                return (next, 1);
            }
        }

        /// What can come next: each character (or markup declaration) the
        /// state turns on, and one it does not.
        fn moves(self) -> &'static [&'static str] {
            const WS: [&str; 5] = [" ", "\t", "\n", "\x0c", "\r"];
            match self {
                W::Data => &["<", "a", " ", "&", ">", "\n", "-", "!"],
                W::TagOpen => &[
                    "!", "/", "?", "a", "B", "1", " ", ">", "<", "!--", "!DOCTYPE", "![CDATA[",
                    "!-",
                ],
                W::EndTagOpen => &["a", ">", " ", "1", "?", "<"],
                W::TagName => &[
                    WS[0], WS[1], WS[2], WS[3], WS[4], "/", ">", "a", "A", "\0", "<", "=", "\"",
                    "'",
                ],
                W::BeforeAttrName | W::AfterAttrName => &[
                    WS[0], WS[1], WS[3], WS[4], "/", ">", "=", "a", "\"", "'", "<", "-",
                ],
                W::AttrName => &[
                    " ", "\n", "/", ">", "=", "a", "A", "\"", "'", "<", "\0", "-",
                ],
                W::BeforeAttrValue => &[" ", "\t", "\"", "'", ">", "a", "=", "<", "`", "-"],
                W::ValueDq => &["\"", "&", "a", "'", ">", "<", "\0", "\n", "-->", "--!>"],
                W::ValueSq => &["'", "&", "a", "\"", ">", "<", "-->"],
                W::ValueUnq => &[
                    " ", "\r", "&", ">", "\"", "'", "<", "=", "`", "a", "\0", "-",
                ],
                W::AfterValueQuoted => &[" ", "\x0c", "/", ">", "a", "\"", "=", "-"],
                W::SelfClosing => &[">", "a", "/", " ", "\""],
                W::Bogus | W::Doctype => &[">", "a", "-", "\0", "<", "\"", "--", "PUBLIC", " "],
                W::CommentStart | W::CommentStartDash => &["-", ">", "a", "<", "!", " "],
                W::Comment => &["<", "-", "a", "!", ">", "\0", " ", "\""],
                W::CommentLt
                | W::CommentLtBang
                | W::CommentLtBangDash
                | W::CommentLtBangDashDash => &["!", "<", "a", "-", ">", "\""],
                W::CommentEndDash | W::CommentEnd | W::CommentEndBang => {
                    &["-", ">", "!", "a", "<", " ", "\""]
                }
            }
        }
    }

    /// Pieces walked through [`W`]'s states, with an occasional piece from
    /// anywhere ([`PIECES`]'s) and an occasional `<sub a b …` to count.
    fn state_walk(next: &mut impl FnMut() -> usize, pieces: &[&str]) -> String {
        let mut out = String::new();
        let mut st = W::Data;
        for _ in 0..1 + next() % 40 {
            let piece = match next() % 20 {
                0 => pieces[next() % pieces.len()],
                1 => "<sub a b c",
                2 => " a b c",
                _ => {
                    let m = st.moves();
                    m[next() % m.len()]
                }
            };
            let at = out.len();
            out.push_str(piece);
            let b = out.as_bytes();
            let mut i = at;
            while i < b.len() {
                let (s, took) = st.step(b, i);
                st = s;
                i += took;
            }
        }
        out
    }

    #[test]
    fn tag_scan_matches_html5ever_on_walks_through_every_state() {
        // Steered through each tag, attribute and comment state's every
        // transition. 300,000 here; RUBRICA_SCAN_WALKS sets more (10,000,000
        // in release with no difference).
        let walks: usize = std::env::var("RUBRICA_SCAN_WALKS")
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(300_000);
        let pieces = [
            "<",
            ">",
            "-",
            "!",
            "--!-",
            "<!-- --!-> ",
            "\"",
            "'",
            "a",
            " ",
            "/",
            "=",
        ];
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as usize
        };
        for _ in 0..walks {
            let raw = state_walk(&mut next, &pieces);
            let want: Vec<Vec<String>> = html5ever_tags(&raw).into_iter().map(|(_, a)| a).collect();
            let (got, _) = scanned(&raw);
            assert_eq!(got, want, "{raw:?}");
        }
    }

    #[test]
    fn a_comment_ended_by_bang_dash_is_still_open() {
        // `--!` then `-` is comment end dash (WHATWG; html5ever 0.40.1's
        // tokenizer/mod.rs:2135), not comment end: `--!->` closes nothing,
        // so the `<sub>` after the `-->` is a tag, and its attributes count.
        let raw = "<!-- --!-> <p title=\"--><sub a b c>z\" >";
        assert_eq!(
            html5ever_tags(raw),
            [(false, vec!["a".into(), "b".into(), "c".into()])]
        );
        assert_eq!(max_attributes(raw), 3);
        assert_eq!(max_attributes("<!-- --!--> <p a b>"), 2);
        assert_eq!(max_attributes("<!-- --!> <p a b>"), 2);
    }

    #[test]
    fn the_final_pass_allows_no_attribute_its_lists_do_not_name() {
        // ammonia's default generic attributes, `lang` and `title`, are off.
        assert_eq!(
            FINAL
                .clean("<p title=\"t\" lang=\"en\">a</p><code title=\"t\">c</code>")
                .to_string(),
            "<p>a</p><code>c</code>"
        );
        assert_eq!(
            FINAL
                .clean("<a href=\"/u\" title=\"t\" lang=\"en\">l</a>")
                .to_string(),
            "<a href=\"/u\" title=\"t\">l</a>"
        );
    }

    #[test]
    fn serializer_splits_never_part_a_crlf() {
        // `&#13;&#10;` is `\r\n` in the text the writer writes; html5ever reads
        // `\r\n` as one line end and `\r<!---->\n` as two. Wherever the
        // run's bound falls, the output is the unsplit one.
        for k in TEXT_RUN - 8..TEXT_RUN + 8 {
            let html = format!("<p>{}\r\n{}</p>", "x".repeat(k), "y".repeat(3 * TEXT_RUN));
            assert_eq!(clean(&html).0, FINAL.clean(&html).to_string(), "{k}");
        }
    }

    #[test]
    fn the_scan_counts_every_attribute_started() {
        assert_eq!(max_attributes("<p a b c>"), 3);
        // Duplicates cost the tokenizer as much: they count.
        assert_eq!(max_attributes("<p a a a a>"), 4);
        assert_eq!(max_attributes("<p/a/b/c>"), 3);
        assert_eq!(max_attributes("<p a=\"1\"b='2'c=3 d>"), 4);
        assert_eq!(max_attributes("</p a b>"), 2);
        // Cut off by the end of the piece: started all the same.
        assert_eq!(max_attributes("<p a b c"), 3);
        // Not attributes: in a value, a comment, a doctype, text.
        assert_eq!(max_attributes("<p t=\"a b c\">"), 1);
        assert_eq!(max_attributes("<!-- <p a b c> -->"), 0);
        assert_eq!(max_attributes("<!--> <p a b>"), 2);
        assert_eq!(max_attributes("<!DOCTYPE <p a b c>"), 0);
        assert_eq!(max_attributes("a < b c d"), 0);
        // The largest tag counts.
        assert_eq!(max_attributes("<p a><q a b c><r a b>"), 3);
    }

    #[test]
    fn serializer_splits_leave_references_and_characters_whole() {
        let text = format!("<p>{}</p>", "&amp;é".repeat(1_000));
        let (out, dropped) = bound_serializer_cost(&text);
        assert_eq!(dropped, []);
        assert!(out.contains("<!---->"));
        assert_eq!(out.replace("<!---->", ""), text);
        for piece in out.split("<!---->").skip(1) {
            assert!(
                piece.starts_with("&amp;") || piece.starts_with('é'),
                "{piece:?}"
            );
        }
        // Short runs, tags and attributes are left alone, byte for byte.
        let html = "<p>a &amp; b</p><a href=\"/u?a=1&amp;b=2\" title=\"t\">l</a>";
        assert!(
            matches!(bound_serializer_cost(html), (Cow::Borrowed(h), d) if h == html && d.is_empty())
        );
    }

    /// A diagram in the vocabulary: every element, every attribute, text that needs escaping.
    const S: &str = concat!(
        "<svg role=\"img\" viewBox=\"0 0 10.00 10.00\" width=\"10.00\" height=\"10.00\" ",
        "class=\"rd rd-flowchart\" font-family=\"Inter, sans-serif\" font-size=\"14\">",
        "<title>t</title><desc>a --&gt; b</desc><g class=\"rd-nodes\"><g class=\"rd-node\">",
        "<rect class=\"rd-shape\" x=\"1.00\" y=\"1.00\" width=\"5.00\" height=\"5.00\" ",
        "rx=\"5.00\" ry=\"5.00\"></rect><path class=\"rd-shape-line\" d=\"M1.00 1.00 V5.00 ",
        "Z\"></path><text class=\"rd-text\" x=\"2.00\" y=\"3.00\" text-anchor=\"middle\">",
        "a &amp; b</text></g></g></svg>"
    );

    /// S with `x` inside its inner `g`, after the `text`.
    fn inner(x: &str) -> String {
        S.replacen("</text></g>", &format!("</text>{x}</g>"), 1)
    }

    #[test]
    fn the_final_pass_keeps_a_diagram_byte_for_byte() {
        let html = format!("<figure class=\"diagram\">{S}</figure>");
        assert_eq!(final_pass(&html), html);
    }

    #[test]
    fn the_final_pass_strips_svg_outside_the_vocabulary() {
        let hostile = inner(concat!(
            "<foreignObject><p>x</p></foreignObject>",
            "<script>alert(1)</script>",
            "<style>*{}</style>",
            "<use href=\"#a\"></use>",
            "<image href=\"x\"></image>",
            "<animate attributeName=\"x\"></animate>",
            "<set to=\"x\"></set>",
            "<circle r=\"1\"></circle>",
        ))
        .replace(
            "<rect class=\"rd-shape\" x=\"1.00\"",
            "<rect class=\"rd-shape evil\" x=\"1e9\" style=\"fill:red\"",
        )
        .replace("<svg role", "<svg onload=\"x\" role")
        .replace(
            "<g class=\"rd-node\">",
            "<g class=\"rd-node\" transform=\"scale(2)\">",
        );
        let out = final_pass(&format!("<figure class=\"x\">{hostile}</figure>"));
        for gone in [
            "foreignobject",
            "foreignObject",
            "<script",
            "<style",
            "<use",
            "<image",
            "<animate",
            "<set",
            "<circle",
            "style=",
            "onload",
            "transform",
            "1e9",
            "evil",
            "class=\"x\"",
        ] {
            assert!(!out.contains(gone), "{gone} in {out}");
        }
        assert!(out.contains("<rect class=\"rd-shape\""), "{out}");
        assert!(out.contains("<figure>"), "{out}");
    }

    #[test]
    fn an_svg_link_survives_the_final_pass_and_is_refused_by_the_check() {
        // ammonia allows `a` by its local name, not its namespace: an SVG `a` inside an
        // allowed `svg` keeps its `href`. Only the check before the writer refuses it.
        let svg = inner("<a href=\"/x\">x</a>");
        let out = final_pass(&format!("<figure class=\"diagram\">{svg}</figure>"));
        assert!(out.contains("<a href=\"/x\">"), "{out}");
        assert_eq!(svg_allowed(&svg), Err("`<a>` is not allowed".to_string()));
    }

    /// ammonia's mutation XSS (RUSTSEC-2026-0533) needs a MathML text integration point or an SVG
    /// `foreignObject` together with a raw-text element; the vocabulary admits SVG `title`,
    /// so neither layer may ever admit the first.
    #[test]
    fn no_mathml_element_or_foreign_object_is_ever_admitted() {
        const MATHML: [&str; 34] = [
            "math",
            "maction",
            "annotation",
            "annotation-xml",
            "menclose",
            "merror",
            "mfenced",
            "mfrac",
            "mglyph",
            "mi",
            "mlabeledtr",
            "mmultiscripts",
            "mn",
            "mo",
            "mover",
            "mpadded",
            "mphantom",
            "mprescripts",
            "mroot",
            "mrow",
            "ms",
            "mspace",
            "msqrt",
            "mstyle",
            "msub",
            "msubsup",
            "msup",
            "mtable",
            "mtd",
            "mtext",
            "mtr",
            "munder",
            "munderover",
            "semantics",
        ];
        let tags = FINAL.clone_tags();
        let classes = FINAL.clone_allowed_classes();
        let attributes = FINAL.clone_tag_attributes();
        for name in MATHML.iter().chain(&["foreignObject", "foreignobject"]) {
            assert!(!SVG_ELEMENTS.contains(name), "SVG_ELEMENTS has {name}");
            assert!(
                !SVG_ATTRIBUTES.iter().any(|(e, _)| e == name),
                "SVG_ATTRIBUTES has {name}"
            );
            assert!(
                !SVG_CLASSES.iter().any(|(e, _)| e == name),
                "SVG_CLASSES has {name}"
            );
            assert!(!tags.contains(name), "the final pass allows <{name}>");
            assert!(
                !classes.contains_key(name),
                "the final pass has classes for <{name}>"
            );
            assert!(
                !attributes.contains_key(name),
                "the final pass has attributes for <{name}>"
            );
        }
        let out = final_pass(&format!(
            "<figure class=\"diagram\">{}</figure>",
            inner("<foreignObject><p>x</p></foreignObject><math><mi>y</mi><mtext>z</mtext></math>")
        ));
        for gone in ["foreign", "<math", "<mi", "<mtext"] {
            assert!(!out.contains(gone), "{gone} in {out}");
        }
        assert_eq!(
            svg_allowed(&inner("<foreignObject></foreignObject>")),
            Err("`<foreignobject>` is not allowed".to_string())
        );
        assert_eq!(
            svg_allowed(&inner("<math><mi>x</mi></math>")),
            Err("`<math>` is not allowed".to_string())
        );
    }
}
