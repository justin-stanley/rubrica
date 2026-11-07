//! Byte offsets to lines: the [`LineTable`] every reported line in the new
//! pipeline comes through, built once per page from the normalised body.

use std::ops::Range;

/// Byte offset to (line, column), both 1-based; columns count UTF-8
/// characters, not bytes. Built once per page in O(n); each lookup is a
/// binary search over line starts plus a scan of at most one line for the
/// column (bounded by the line's length; a lookup that needs no column —
/// every caller but the strict check — stops at the search).
///
/// Lines are counted as `render` counts them: every `\n` ends one, and a
/// text of `k` `\n`s has `k + 1` lines, so `"a\n"` has two, the second
/// empty (markdown-it counts the same way).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineTable {
    /// The byte offset each line starts at: `starts[0] == 0`, strictly
    /// increasing, one past each `\n`.
    starts: Vec<usize>,
    /// The length of the text the table was built from.
    len: usize,
}

impl LineTable {
    /// `text` is the normalised body (`\n` line ends only): every `\n` ends a line.
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            text.bytes()
                .enumerate()
                .filter(|&(_, b)| b == b'\n')
                .map(|(i, _)| i + 1),
        );
        LineTable {
            starts,
            len: text.len(),
        }
    }

    /// The 1-based line holding byte `offset` (`offset == len` is the last line;
    /// an offset past `len` is clamped and is a bug caught by `debug_assert!`).
    pub fn line(&self, offset: usize) -> usize {
        debug_assert!(
            offset <= self.len,
            "offset {offset} is past the end of a text of {} bytes",
            self.len
        );
        let offset = offset.min(self.len);
        // The number of line starts at or before `offset`: `starts[0] == 0`
        // is one of them, so this is the largest `i` with `starts[i] <=
        // offset`, plus one.
        self.starts.partition_point(|&start| start <= offset)
    }

    /// The 1-based line and UTF-8 column of `offset` in `text` (the text it was built
    /// from). An offset inside a multibyte character is that character's column (`"é"`,
    /// offset 1 → (1, 1)); no caller passes one today.
    pub fn line_col(&self, text: &str, offset: usize) -> (usize, usize) {
        debug_assert_eq!(
            text.len(),
            self.len,
            "the table was built from a text of {} bytes",
            self.len
        );
        let line = self.line(offset);
        let start = self.starts[line - 1];
        let offset = offset.min(text.len()).max(start);
        // Characters, not bytes: every byte but a UTF-8 continuation byte
        // (`10xxxxxx`) starts one.
        let chars_before = text.as_bytes()[start..offset]
            .iter()
            .filter(|&&b| !(0x80..0xC0).contains(&b))
            .count();
        (
            line,
            chars_before + usize::from(text.is_char_boundary(offset)),
        )
    }

    /// The number of lines: one more than the text's `\n`s.
    pub fn line_count(&self) -> usize {
        self.starts.len()
    }

    /// The byte range of line `n` (1-based), without its `\n`.
    ///
    /// # Panics
    ///
    /// When `n` is 0 or past [`LineTable::line_count`].
    pub fn line_range(&self, n: usize) -> Range<usize> {
        assert!(
            n >= 1 && n <= self.starts.len(),
            "line {n} of a text with {} lines",
            self.starts.len()
        );
        let start = self.starts[n - 1];
        let end = self.starts.get(n).map_or(self.len, |&next| next - 1);
        start..end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_inside_a_multibyte_char_is_its_column() {
        // `é` is two bytes: offset 1 is inside it, and is still column 1.
        let text = "é\n";
        let table = LineTable::new(text);
        assert_eq!(table.line_col(text, 0), (1, 1));
        assert_eq!(table.line_col(text, 1), (1, 1));
        assert_eq!(table.line_col(text, 2), (1, 2));
        assert_eq!(table.line_col(text, 3), (2, 1));
    }
}
