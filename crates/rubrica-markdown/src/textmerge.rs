//! Consecutive `Event::Text` events merged into one run, with the pieces kept so a byte in
//! the run maps back to a source offset exactly where the piece is verbatim.
//!
//! The parser splits a paragraph's text at escapes and entities: `a\*b` is `Text("a")@0..1`
//! and `Text("*b")@2..4` (the backslash at 1 is in no range), `&amp;x` is `Text("&")@0..5`
//! and `Text("x")@5..6`. A run is the concatenation of such consecutive pieces; each piece
//! remembers where its text sits in the run and where its bytes sit in the source. A piece is
//! **verbatim** when its text is as long as its range (no escape, entity or tab inside it);
//! then a text index inside it maps to an exact source byte. A non-verbatim piece (an
//! entity: one decoded character, a longer range) maps to the whole entity. The length test
//! can misfire: an entity whose decoded form is as long as its spelling (`&nvap;`, six bytes
//! either way) counts as verbatim, and an offset inside it would be off by the difference in
//! layout; it is harmless, since the autolink scan never puts a link boundary inside an
//! entity's bytes (it drops a trailing reference whole, `&name;` and `&#…;` alike).
//!
//! This is Rubrica's own text merge, written from the parser's observable output (the
//! vendored core's `utils.rs` is left out on purpose; see `UPSTREAM`).

use std::ops::Range;

use crate::Event;

/// Consecutive `Event::Text` events merged into one run, with the pieces
/// kept so a byte in the run maps back to a source offset exactly where
/// the piece is verbatim (no escape, entity or tab inside it).
#[derive(Debug)]
pub struct TextMerge<'a, I> {
    iter: I,
    pending: Option<(Event<'a>, Range<usize>)>,
}

impl<'a, I: Iterator<Item = (Event<'a>, Range<usize>)>> TextMerge<'a, I> {
    /// Merges the `(event, range)` pairs of `iter`, usually an [`OffsetIter`](crate::OffsetIter).
    pub fn new(iter: I) -> Self {
        TextMerge {
            iter,
            pending: None,
        }
    }

    fn pull(&mut self) -> Option<(Event<'a>, Range<usize>)> {
        self.pending.take().or_else(|| self.iter.next())
    }
}

/// One merged run of text: the pieces' text concatenated, the source range from the first
/// piece's start to the last piece's end, and the pieces themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRun {
    /// The pieces' text, concatenated.
    pub text: String,
    /// The source range from the first piece's start to the last piece's end.
    pub range: Range<usize>,
    /// The `Text` events the run was merged from, in order.
    pub pieces: Vec<Piece>,
}

/// One `Text` event of a run: its text occupies `text_start..text_start + text_len` of the
/// run's text and came from `range` of the source. Verbatim iff `text_len == range.len()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    /// Where the piece starts in the run's text, in bytes.
    pub text_start: usize,
    /// The piece's length in the run's text, in bytes.
    pub text_len: usize,
    /// The piece's byte range in the source.
    pub range: Range<usize>,
}

impl Piece {
    fn is_verbatim(&self) -> bool {
        self.text_len == self.range.len()
    }
}

impl TextRun {
    /// The piece holding text index `i` (`text_start <= i < text_start + text_len`); for
    /// `i == text.len()`, the last piece. Pieces tile the text, so there always is one.
    fn piece_holding(&self, i: usize) -> &Piece {
        // The first piece whose end is past `i`; past every piece, the last one.
        let k = self
            .pieces
            .partition_point(|p| p.text_start + p.text_len <= i);
        &self.pieces[k.min(self.pieces.len() - 1)]
    }

    /// The source offset where text index `i` (0 ≤ i ≤ text.len()) *starts*: take the piece
    /// holding `i` (the one with `text_start <= i < text_start + text_len`; for `i == text.len()`
    /// the last piece). A **verbatim** piece (`text_len == range.len()`) gives the exact byte
    /// `range.start + (i - text_start)`; a non-verbatim piece (an entity: one decoded char,
    /// a longer range) gives `range.start`, or `range.end` when `i == text.len()`.
    pub fn offset_of(&self, i: usize) -> usize {
        debug_assert!(i <= self.text.len());
        let p = self.piece_holding(i);
        if p.is_verbatim() {
            p.range.start + (i - p.text_start)
        } else if i == self.text.len() {
            p.range.end
        } else {
            p.range.start
        }
    }

    /// The source offset where a span *ending* at text index `i` (1 ≤ i ≤ text.len()) ends:
    /// take the piece holding `i - 1`; verbatim gives `range.start + (i - text_start)`
    /// (one past the byte of `i - 1`), non-verbatim gives `range.end`. (Two functions, so a
    /// span ending at a piece boundary is not pushed into the next piece: pieces are not
    /// always adjacent, `a\*b` leaves byte 1 in no piece.)
    pub fn offset_after(&self, i: usize) -> usize {
        debug_assert!(i >= 1 && i <= self.text.len());
        let p = self.piece_holding(i - 1);
        if p.is_verbatim() {
            p.range.start + (i - p.text_start)
        } else {
            p.range.end
        }
    }

    /// Whether source offset `s` is strictly inside a non-verbatim piece (an entity's
    /// bytes), where no text index corresponds to it.
    pub fn inside_non_verbatim(&self, s: usize) -> bool {
        let k = self.pieces.partition_point(|p| p.range.end <= s);
        self.pieces
            .get(k)
            .is_some_and(|p| !p.is_verbatim() && s > p.range.start)
    }

    /// The text index where a span that *starts* at source offset `s` starts: inside a
    /// verbatim piece the exact index; at a non-verbatim piece's start its `text_start`,
    /// inside one the index after it; in a gap between pieces (an escape's backslash) the
    /// next piece's start; past every piece, `text.len()`.
    pub fn text_index_before(&self, s: usize) -> usize {
        let k = self.pieces.partition_point(|p| p.range.end <= s);
        let Some(p) = self.pieces.get(k) else {
            return self.text.len();
        };
        if s <= p.range.start {
            p.text_start
        } else if p.is_verbatim() {
            p.text_start + (s - p.range.start)
        } else {
            p.text_start + p.text_len
        }
    }

    /// The text index where a span that *ends* at source offset `s` ends: inside a verbatim
    /// piece the exact index; at or inside a non-verbatim piece its end (a decoded character
    /// is taken whole); in a gap after a piece that piece's end; before every piece, 0.
    pub fn text_index_after(&self, s: usize) -> usize {
        let k = self.pieces.partition_point(|p| p.range.start < s);
        let Some(p) = k.checked_sub(1).map(|k| &self.pieces[k]) else {
            return 0;
        };
        if s >= p.range.end || !p.is_verbatim() {
            p.text_start + p.text_len
        } else {
            p.text_start + (s - p.range.start)
        }
    }
}

/// What the merge yields: a run of text, or any other event unchanged.
#[derive(Debug)]
pub enum Merged<'a> {
    /// A run of merged text.
    Run(TextRun),
    /// Any other event, with its source range, unchanged.
    Other(Event<'a>, Range<usize>),
}

impl<'a, I: Iterator<Item = (Event<'a>, Range<usize>)>> Iterator for TextMerge<'a, I> {
    type Item = Merged<'a>;

    fn next(&mut self) -> Option<Merged<'a>> {
        let (event, range) = self.pull()?;
        let first = match event {
            Event::Text(text) => text,
            other => return Some(Merged::Other(other, range)),
        };
        let mut text = String::new();
        let mut pieces: Vec<Piece> = Vec::new();
        let mut push = |t: &str, r: Range<usize>, text: &mut String| {
            // An empty `Text` adds nothing to the run and is not a piece.
            if !t.is_empty() {
                pieces.push(Piece {
                    text_start: text.len(),
                    text_len: t.len(),
                    range: r,
                });
                text.push_str(t);
            }
        };
        push(&first, range, &mut text);
        loop {
            match self.pull() {
                Some((Event::Text(t), r)) => push(&t, r, &mut text),
                Some(other) => {
                    self.pending = Some(other);
                    break;
                }
                None => break,
            }
        }
        if pieces.is_empty() {
            // Empty runs are dropped, as upstream does.
            return self.next();
        }
        let range = pieces[0].range.start..pieces[pieces.len() - 1].range.end;
        Some(Merged::Run(TextRun {
            text,
            range,
            pieces,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse_with_offsets, Event, GFM};

    fn runs(md: &str) -> Vec<Merged<'_>> {
        TextMerge::new(parse_with_offsets(md, GFM)).collect()
    }

    fn text_runs(md: &str) -> Vec<TextRun> {
        runs(md)
            .into_iter()
            .filter_map(|m| match m {
                Merged::Run(run) => Some(run),
                Merged::Other(..) => None,
            })
            .collect()
    }

    #[test]
    fn a_single_text_is_a_run_of_one_piece() {
        let runs = text_runs("abc");
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        assert_eq!(run.text, "abc");
        assert_eq!(run.range, 0..3);
        assert_eq!(
            run.pieces,
            vec![Piece {
                text_start: 0,
                text_len: 3,
                range: 0..3
            }]
        );
        assert_eq!(run.offset_of(0), 0);
        assert_eq!(run.offset_of(3), 3);
        assert_eq!(run.offset_after(3), 3);
    }

    #[test]
    fn pieces_map_offsets_exactly_when_verbatim() {
        // `a\*b`: pieces `a`@0..1 and `*b`@2..4; the backslash at 1 is in no piece.
        let runs = text_runs("a\\*b");
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        assert_eq!(run.text, "a*b");
        assert_eq!(run.range, 0..4);
        assert_eq!(run.pieces.len(), 2);
        assert_eq!(run.offset_of(0), 0);
        assert_eq!(run.offset_of(1), 2);
        assert_eq!(run.offset_of(2), 3);
        assert_eq!(run.offset_of(3), 4);
        // A span ending at the piece boundary ends at 1, not in the next piece.
        assert_eq!(run.offset_after(1), 1);
        assert_eq!(run.offset_after(2), 3);
        assert_eq!(run.offset_after(3), 4);
        // `ab \* www.a.com x`: pieces `ab `@0..3 and `* www.a.com x`@4..17.
        let runs = text_runs("ab \\* www.a.com x");
        let run = &runs[0];
        assert_eq!(run.text, "ab * www.a.com x");
        // `www.a.com` is run text 5..14 and source bytes 6..15.
        assert_eq!(run.offset_of(5), 6);
        assert_eq!(run.offset_after(14), 15);
    }

    #[test]
    fn a_non_verbatim_piece_maps_to_its_start() {
        // `a&amp;b`: pieces `a`@0..1, `&`@1..6 (one byte of text, five of range), `b`@6..7.
        let runs = text_runs("a&amp;b");
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        assert_eq!(run.text, "a&b");
        assert_eq!(run.range, 0..7);
        assert_eq!(run.pieces.len(), 3);
        assert_eq!(run.offset_of(1), 1);
        assert_eq!(run.offset_after(2), 6);
        assert_eq!(run.offset_of(2), 6);
        // A run that ends in an entity: the end index maps to the entity's range end.
        let runs = text_runs("a&amp;");
        let run = &runs[0];
        assert_eq!(run.text, "a&");
        assert_eq!(run.offset_of(2), 6);
        assert_eq!(run.offset_after(2), 6);
    }

    #[test]
    fn source_offsets_map_back_to_text_indices() {
        // `a\*b`: pieces `a`@0..1 and `*b`@2..4; the backslash at 1 is in no piece.
        let runs = text_runs("a\\*b");
        let run = &runs[0];
        assert_eq!(run.text_index_before(0), 0);
        assert_eq!(run.text_index_before(1), 1); // the gap: the next piece's start
        assert_eq!(run.text_index_before(2), 1);
        assert_eq!(run.text_index_before(3), 2);
        assert_eq!(run.text_index_before(4), 3);
        assert_eq!(run.text_index_after(1), 1);
        assert_eq!(run.text_index_after(2), 1); // the gap: the previous piece's end
        assert_eq!(run.text_index_after(3), 2);
        assert_eq!(run.text_index_after(4), 3);
        // `a&amp;b`: pieces `a`@0..1, `&`@1..6, `b`@6..7: an entity maps whole.
        let runs = text_runs("a&amp;b");
        let run = &runs[0];
        assert_eq!(run.text_index_before(1), 1);
        assert_eq!(run.text_index_before(3), 2);
        assert_eq!(run.text_index_before(6), 2);
        assert_eq!(run.text_index_after(6), 2);
        assert_eq!(run.text_index_after(3), 2);
        assert_eq!(run.text_index_after(1), 1);
        assert_eq!(run.text_index_after(7), 3);
        assert_eq!(run.text_index_before(7), 3);
    }

    #[test]
    fn empty_runs_are_dropped() {
        // Every `Merged::Run` has text; the other events pass through in order.
        for md in ["", "a\n\nb", "\\*", "[a]: /u\n", "a <b></b> c", "foo \nbar"] {
            for m in runs(md) {
                if let Merged::Run(run) = m {
                    assert!(!run.text.is_empty(), "{md:?}: an empty run");
                    assert!(!run.pieces.is_empty(), "{md:?}: a run with no pieces");
                    assert!(
                        run.pieces.iter().all(|p| p.text_len > 0),
                        "{md:?}: an empty piece"
                    );
                }
            }
        }
        // The other events keep their ranges.
        let merged = runs("foo \nbar");
        let kinds: Vec<String> = merged
            .iter()
            .map(|m| match m {
                Merged::Run(run) => format!("Run({:?}@{:?})", run.text, run.range),
                Merged::Other(e, r) => format!("{e:?}@{r:?}"),
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                format!("{:?}@0..8", Event::Start(crate::Tag::Paragraph)),
                "Run(\"foo\"@0..3)".to_string(),
                format!("{:?}@4..5", Event::SoftBreak),
                "Run(\"bar\"@5..8)".to_string(),
                format!("{:?}@0..8", Event::End(crate::TagEnd::Paragraph)),
            ]
        );
    }
}
