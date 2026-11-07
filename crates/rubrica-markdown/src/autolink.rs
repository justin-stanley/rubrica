//! GFM extended autolinks (`www.`, `http://`, `https://`, `ftp://`, email) found in text
//! runs and turned into `Link` events, in one forward pass per run.
//!
//! [`Autolinks`] adapts an `(Event, Range<usize>)` stream: consecutive `Text` events are
//! merged into runs by [`crate::textmerge::TextMerge`], each run outside a link, image or
//! code block is scanned, and the run comes out as `Text` gaps and
//! `Start(Link)`/`Text`/`End(Link)` triples with ranges mapped back to the source. The rules
//! are the GFM spec's ("Autolinks (extension)"); where the spec leaves a nested candidate
//! open, Rubrica's rule is the algorithm below, pinned by `tests/autolink.rs` and stated in
//! `docs/spec/autolinks.md`. Every `LinkType::Email` link out of the adapter carries
//! `mailto:`, the parser's own `<a@b.c>` autolinks included.
//!
//! # What the scan reads
//!
//! The scan reads the run's **source bytes** (`src[run.range]`), not its decoded text: an
//! entity is its `&name;` spelling there and an escape keeps its backslash, so the `&name;`
//! exclusion, the terminators and the delimiters see what the author wrote, as cmark-gfm's
//! do. The links found are mapped back to the decoded text through the run's pieces
//! ([`crate::textmerge::TextRun`]): the emitted link text and destination are decoded output,
//! and the ranges are the source bytes. A link's boundary never falls inside an entity's
//! bytes (no candidate starts inside one, and the trim drops an entity whole or not at all),
//! and a boundary at an escape's backslash, which is in no piece, maps to the piece beside it.
//!
//! # The scan
//!
//! `scan(view, delimited_at_start)` walks the bytes with a cursor `pos` and a `window`. At a
//! candidate start (a delimited `www.`, `http://`, `https://`, `ftp://`, or any `@`) it runs
//! one check that reads forward to the candidate's end (`consumed_end`) and, for email,
//! backward to `window`; then `pos` moves to the link's end on success, or to at least
//! `consumed_end` on failure. `window` moves to `pos` after every email candidate and every
//! success; a failed `www.`/URL candidate leaves it, so its bytes may still be an email's
//! local part (`www.afoo@b.c` is an email link, as in cmark-gfm). A candidate is
//! **delimited** when it is preceded by ASCII whitespace (space, tab, line feed, form feed,
//! carriage return, and vertical tab, which `is_ascii_whitespace` leaves out) or one of `*`,
//! `_`, `~`, `(`; at the run start the caller says: a run that begins a block or follows a
//! line break or an emphasis delimiter is delimited, one that follows a code span, inline
//! HTML, a footnote reference or the end of a link or image is not.
//!
//! # Invariants
//!
//! - **Progress**: every iteration of the dispatch loop increases `pos`.
//! - **No rescan behind a span**: after a candidate (success or failure) `pos >= consumed_end`,
//!   so no byte is read by a forward scan after `pos` has passed it; the backward local-part
//!   loop reads only bytes in `[window, c)`, and `window` moves past them at the end of the
//!   candidate, so it reads a byte at most once in the whole scan.
//! - **Bounded steps**: `steps ≤ 7·n + 16` for a run of `n` bytes, with a *step* defined as in
//!   [`scan_counting`]; `docs/spec/autolinks.md` derives the bound, `tests/autolink.rs`,
//!   the private linearity test and the fuzz target assert it.
//! - **Equivalence on the spec**: the 11 GFM autolink examples pass; the nested-candidate
//!   cases are defined by this algorithm (Rubrica's rule) and pinned by tests.
//!
//! The trailing-punctuation trim counts `(` and `)` once before its loop and updates the
//! counts as bytes are dropped: a re-count per dropped `)` is quadratic on a candidate that
//! ends in many `)`.

use std::collections::VecDeque;
use std::ops::Range;

use crate::textmerge::{Merged, TextMerge, TextRun};
use crate::{CowStr, Event, LinkType, Tag, TagEnd};

/// The event stream with extended autolinks found in its text runs.
#[derive(Debug)]
pub struct Autolinks<'a, I> {
    inner: TextMerge<'a, I>,
    /// The document: the scan reads a run's source bytes through it.
    src: &'a str,
    queue: VecDeque<(Event<'a>, Range<usize>)>,
    link_depth: usize,
    code_depth: usize,
    /// Whether a run starting now is delimited: set by the last non-text event.
    start_delimited: bool,
}

impl<'a, I: Iterator<Item = (Event<'a>, Range<usize>)>> Autolinks<'a, I> {
    /// `iter` is the event stream of `src` with offsets (`parse_with_offsets`).
    pub fn new(iter: I, src: &'a str) -> Self {
        Autolinks {
            inner: TextMerge::new(iter),
            src,
            queue: VecDeque::new(),
            link_depth: 0,
            code_depth: 0,
            start_delimited: true,
        }
    }

    /// Queue the run's gaps and links, each with its source range. `found` is in the
    /// coordinates of the run's source view, `src[run.range]`.
    fn emit_run(&mut self, run: TextRun, found: Vec<Found>) {
        let base = run.range.start;
        let mut prev = 0;
        for link in found {
            // A link's boundary never falls inside an entity's bytes: no candidate starts
            // inside one, and the trim drops one whole or not at all.
            debug_assert!(
                !run.inside_non_verbatim(base + link.start)
                    && !run.inside_non_verbatim(base + link.end),
                "a link boundary inside a non-verbatim piece: {}..{} in {:?}",
                link.start,
                link.end,
                run.range
            );
            let start = run.text_index_before(base + link.start);
            let end = run.text_index_after(base + link.end);
            if end <= start {
                // Cannot happen: a link holds at least its prefix and one domain byte, both
                // verbatim. Left as text rather than an empty link.
                debug_assert!(false, "an empty link at {}..{}", link.start, link.end);
                continue;
            }
            if start > prev {
                self.queue.push_back((
                    Event::Text(CowStr::from(run.text[prev..start].to_owned())),
                    run.offset_of(prev)..run.offset_after(start),
                ));
            }
            let text = &run.text[start..end];
            let dest_url = match link.kind {
                Kind::Www => format!("http://{text}"),
                Kind::Url => text.to_owned(),
                Kind::Email => format!("mailto:{text}"),
            };
            let link_type = match link.kind {
                Kind::Www | Kind::Url => LinkType::Autolink,
                Kind::Email => LinkType::Email,
            };
            let range = run.offset_of(start)..run.offset_after(end);
            self.queue.push_back((
                Event::Start(Tag::Link {
                    link_type,
                    dest_url: CowStr::from(dest_url),
                    title: CowStr::Borrowed(""),
                    id: CowStr::Borrowed(""),
                }),
                range.clone(),
            ));
            self.queue
                .push_back((Event::Text(CowStr::from(text.to_owned())), range.clone()));
            self.queue.push_back((Event::End(TagEnd::Link), range));
            prev = end;
        }
        if prev < run.text.len() {
            let range = run.offset_of(prev)..run.offset_after(run.text.len());
            self.queue.push_back((
                Event::Text(CowStr::from(run.text[prev..].to_owned())),
                range,
            ));
        }
    }
}

impl<'a, I: Iterator<Item = (Event<'a>, Range<usize>)>> Iterator for Autolinks<'a, I> {
    type Item = (Event<'a>, Range<usize>);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(item) = self.queue.pop_front() {
                return Some(item);
            }
            match self.inner.next()? {
                Merged::Other(mut event, range) => {
                    // Every `LinkType::Email` link out of `events` carries `mailto:`: the
                    // parser's own `<a@b.c>` autolink gets the prefix the scanner's have.
                    if let Event::Start(Tag::Link {
                        link_type: LinkType::Email,
                        dest_url,
                        ..
                    }) = &mut event
                    {
                        if !dest_url.starts_with("mailto:") {
                            *dest_url = CowStr::from(format!("mailto:{dest_url}"));
                        }
                    }
                    match &event {
                        Event::Start(Tag::Link { .. } | Tag::Image { .. }) => self.link_depth += 1,
                        Event::End(TagEnd::Link | TagEnd::Image) => {
                            self.link_depth = self.link_depth.saturating_sub(1);
                        }
                        // A code block's content is `Text` events and must never be
                        // scanned; an inline code span is one `Event::Code`, never a run.
                        Event::Start(Tag::CodeBlock(_)) => self.code_depth += 1,
                        Event::End(TagEnd::CodeBlock) => {
                            self.code_depth = self.code_depth.saturating_sub(1);
                        }
                        _ => {}
                    }
                    // A run that follows one of these starts right after its bytes: not a
                    // line start, not whitespace, not a delimiter.
                    self.start_delimited = !matches!(
                        event,
                        Event::Code(_)
                            | Event::InlineHtml(_)
                            | Event::Html(_)
                            | Event::InlineMath(_)
                            | Event::DisplayMath(_)
                            | Event::FootnoteReference(_)
                            | Event::End(TagEnd::Link | TagEnd::Image)
                            | Event::Start(Tag::Superscript | Tag::Subscript)
                            | Event::End(TagEnd::Superscript | TagEnd::Subscript)
                    );
                    return Some((event, range));
                }
                Merged::Run(run) => {
                    let found = if self.link_depth > 0 || self.code_depth > 0 {
                        Vec::new()
                    } else {
                        scan(&self.src[run.range.clone()], self.start_delimited)
                    };
                    if found.is_empty() {
                        let range = run.range.clone();
                        return Some((Event::Text(CowStr::from(run.text)), range));
                    }
                    self.emit_run(run, found);
                }
            }
        }
    }
}

/// A link the scan found: `view[start..end]` of the run's source view, and what kind it is.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

/// The kind of an extended autolink: `www.` (scheme `http` inserted), a scheme URL, or an
/// email address (`mailto:` added).
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Www,
    Url,
    Email,
}

/// The scan of one run's source view: the links found, as `(start, end, kind)` in view
/// coordinates, in order, and the number of **steps** it took. `delimited_at_start` says
/// whether a candidate at index 0 is delimited (see the module doc). Public but hidden: the
/// linearity tests (here, in the private suite and in the fuzz crate) call it; nothing else
/// may.
///
/// A step is one execution of a loop body in: the dispatch loop (`while pos < n`); every
/// forward scan (the domain scan, the email domain scan, the path scan); the one validity
/// pass over a scanned domain (segments, underscores); the backward local-part loop; and in
/// the trim the paren pre-count pass, the removal loop and the entity walk-back loop. The
/// case-insensitive prefix compares are **not** steps (each is at most 8 bytes and happens
/// once per dispatch), nor is a one-byte look-ahead. Nothing else counts.
#[doc(hidden)]
pub fn scan_counting(view: &str, delimited_at_start: bool) -> (Vec<Found>, usize) {
    let mut scanner = Scanner {
        b: view.as_bytes(),
        steps: 0,
        delimited_at_start,
    };
    let found = scanner.run();
    (found, scanner.steps)
}

/// GFM's "non-space": ASCII whitespace, with `\x0B` (vertical tab), which
/// `is_ascii_whitespace` leaves out.
fn is_space(x: u8) -> bool {
    x.is_ascii_whitespace() || x == 0x0B
}

/// `pat` at `b[c..]`, ASCII case-insensitively, without allocating.
fn starts_ci(b: &[u8], c: usize, pat: &[u8]) -> bool {
    b.len() >= c + pat.len() && b[c..c + pat.len()].eq_ignore_ascii_case(pat)
}

/// A byte of a `www.`/URL domain: alphanumeric, `_`, `-` or `.`.
fn is_domain(x: u8) -> bool {
    x.is_ascii_alphanumeric() || matches!(x, b'_' | b'-' | b'.')
}

/// A byte of an email's local part: alphanumeric, `.`, `-`, `_` or `+`.
fn is_local(x: u8) -> bool {
    x.is_ascii_alphanumeric() || matches!(x, b'.' | b'-' | b'_' | b'+')
}

/// The scan over one run's bytes, counting steps as the module doc defines them.
struct Scanner<'b> {
    b: &'b [u8],
    steps: usize,
    delimited_at_start: bool,
}

/// A check's answer: the link found as `(start, end)` in the run, if any, and the end of
/// the bytes the check consumed, which the dispatch never rescans.
type Checked = (Option<(usize, usize)>, usize);

impl Scanner<'_> {
    /// A candidate may start at a delimited run start, after whitespace, or after `*`, `_`,
    /// `~`, `(`.
    fn delimited(&self, c: usize) -> bool {
        if c == 0 {
            return self.delimited_at_start;
        }
        let before = self.b[c - 1];
        is_space(before) || matches!(before, b'*' | b'_' | b'~' | b'(')
    }

    /// The dispatch loop.
    fn run(&mut self) -> Vec<Found> {
        let b = self.b;
        let n = b.len();
        let mut pos = 0;
        let mut window = 0;
        let mut out = Vec::new();
        while pos < n {
            self.steps += 1;
            let c = pos;
            let candidate = match b[c] {
                // `www.` is lower-case, as cmark-gfm reads it; the schemes are any case.
                b'w' if self.delimited(c) && b[c..].starts_with(b"www.") => {
                    Some((Kind::Www, c + 4))
                }
                b'h' | b'H' if self.delimited(c) && starts_ci(b, c, b"http://") => {
                    Some((Kind::Url, c + 7))
                }
                b'h' | b'H' if self.delimited(c) && starts_ci(b, c, b"https://") => {
                    Some((Kind::Url, c + 8))
                }
                b'f' | b'F' if self.delimited(c) && starts_ci(b, c, b"ftp://") => {
                    Some((Kind::Url, c + 6))
                }
                b'@' => Some((Kind::Email, c)),
                _ => None,
            };
            let Some((kind, body)) = candidate else {
                pos = c + 1;
                continue;
            };
            let (result, consumed_end) = match kind {
                Kind::Www | Kind::Url => self.check_domain(kind, c, body),
                Kind::Email => self.check_email(c, window),
            };
            match result {
                Some((start, end)) => {
                    out.push(Found { start, end, kind });
                    pos = end;
                    window = end;
                }
                None => {
                    pos = consumed_end.max(c + 1);
                    // A failed `www.`/URL candidate leaves `window`: its bytes may be an
                    // email's local part. An email candidate always moves it.
                    if kind == Kind::Email {
                        window = pos;
                    }
                }
            }
        }
        out
    }

    /// A `www.` or scheme candidate at `c` whose body (the domain) starts at `body`.
    fn check_domain(&mut self, kind: Kind, c: usize, body: usize) -> Checked {
        let b = self.b;
        let n = b.len();
        // The forward scan: the maximal domain run. Trailing `.`s are not part of the
        // domain for the validity check (the path takes them and the trim drops them):
        // `check_end` is one past the last non-`.` byte.
        let mut domain_end = body;
        let mut check_end = body;
        while domain_end < n && is_domain(b[domain_end]) {
            self.steps += 1;
            domain_end += 1;
            if b[domain_end - 1] != b'.' {
                check_end = domain_end;
            }
        }
        // Non-empty; a URL's first host byte is alphanumeric (cmark-gfm's rule; `http://-`
        // is text); a `www.` domain may start with `-` or `_` but not `.`.
        if check_end == body
            || b[body] == b'.'
            || (kind == Kind::Url && !b[body].is_ascii_alphanumeric())
        {
            return (None, domain_end);
        }
        // The validity pass: no empty segment, periods, and underscores in the last two
        // segments. For `www.` the whole domain is `www.` + body; `www` has no underscore
        // and its `.` only resets the counters, so the pass over the body alone decides
        // the same.
        let mut periods = 0;
        let mut underscores_last = 0;
        let mut underscores_before = 0;
        let mut after_period = false;
        let mut empty_segment = false;
        for &x in &b[body..check_end] {
            self.steps += 1;
            match x {
                b'.' => {
                    periods += 1;
                    empty_segment |= after_period;
                    after_period = true;
                    underscores_before = underscores_last;
                    underscores_last = 0;
                }
                b'_' => {
                    after_period = false;
                    underscores_last += 1;
                }
                _ => after_period = false,
            }
        }
        // A `www.` domain needs a period; a URL's may be short (`http://a`).
        if (kind == Kind::Www && periods == 0)
            || empty_segment
            || underscores_last > 0
            || underscores_before > 0
        {
            return (None, domain_end);
        }
        let link_end = self.path_end(domain_end);
        (self.trim(c, body, link_end).map(|end| (c, end)), link_end)
    }

    /// An `@` at `c`: the local part runs back to `window` at most, the domain forward.
    fn check_email(&mut self, c: usize, window: usize) -> Checked {
        let b = self.b;
        let n = b.len();
        let mut l = c;
        while l > window && is_local(b[l - 1]) {
            self.steps += 1;
            l -= 1;
        }
        if l == c {
            return (None, c + 1);
        }
        // The domain: alphanumerics, `-`, `_`, and a `.` only when an alphanumeric follows
        // (cmark-gfm's rule: `-@o.-h` is text, and a trailing `.` is never taken).
        let mut e = c + 1;
        let mut periods = 0;
        while e < n {
            let x = b[e];
            if x.is_ascii_alphanumeric() || matches!(x, b'-' | b'_') {
                // fine
            } else if x == b'.' && e + 1 < n && b[e + 1].is_ascii_alphanumeric() {
                periods += 1;
            } else {
                break;
            }
            self.steps += 1;
            e += 1;
        }
        let d = &b[c + 1..e];
        // Non-empty, at least one period, no empty segment (only a leading `.` can make
        // one now), and a last byte that is alphanumeric.
        if d.is_empty() || periods == 0 || d[0] == b'.' || !d[d.len() - 1].is_ascii_alphanumeric() {
            return (None, e);
        }
        (Some((l, e)), e)
    }

    /// The path: non-space non-`<` bytes after the domain.
    fn path_end(&mut self, from: usize) -> usize {
        let b = self.b;
        let n = b.len();
        let mut i = from;
        while i < n && !is_space(b[i]) && b[i] != b'<' {
            self.steps += 1;
            i += 1;
        }
        i
    }

    /// Extended autolink path validation on `b[start..end]`, linear: the parentheses are
    /// counted once and the counts follow the bytes dropped. `None` when nothing is left
    /// after the prefix (`end <= body`).
    fn trim(&mut self, start: usize, body: usize, end: usize) -> Option<usize> {
        let b = self.b;
        let mut open = 0usize;
        let mut close = 0usize;
        for &x in &b[start..end] {
            self.steps += 1;
            match x {
                b'(' => open += 1,
                b')' => close += 1,
                _ => {}
            }
        }
        let mut end = end;
        loop {
            self.steps += 1;
            if end <= body {
                return None;
            }
            match b[end - 1] {
                b'?' | b'!' | b'.' | b',' | b':' | b'*' | b'_' | b'~' => end -= 1,
                b')' if close > open => {
                    close -= 1;
                    end -= 1;
                }
                b';' => {
                    // `&` + one or more alphanumerics + `;` at the end is an entity reference
                    // (`&amp;`), as is `&#` + alphanumerics + `;` (`&#46;`, `&#x2e;`): either is
                    // dropped whole, never split; any other trailing `;` is dropped alone
                    // (cmark-gfm).
                    let mut i = end - 1;
                    while i > start && b[i - 1].is_ascii_alphanumeric() {
                        self.steps += 1;
                        i -= 1;
                    }
                    let has_name = i < end - 1;
                    let amp = if has_name && i > start && b[i - 1] == b'#' {
                        i - 1
                    } else {
                        i
                    };
                    if has_name && amp > start && b[amp - 1] == b'&' {
                        end = amp - 1;
                    } else {
                        end -= 1;
                    }
                }
                _ => break,
            }
        }
        Some(end)
    }
}

/// The scan without its step count: the unit of testing.
pub(crate) fn scan(view: &str, delimited_at_start: bool) -> Vec<Found> {
    scan_counting(view, delimited_at_start).0
}
