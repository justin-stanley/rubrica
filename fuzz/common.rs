//! Shared by the targets that compare with upstream pulldown-cmark 0.13.6 (`mod common;` with
//! a `#[path]`).

use rubrica_markdown::{parse_with_offsets, Event, Tag, TagEnd, GFM};

/// True when our GFM parse of `s` opens a table whose header row does not start with a `|`
/// (after its indent and any container markers).
///
/// Rubrica deliberately lets such a table interrupt a paragraph, as cmark-gfm does, while
/// upstream opens one there only when the header row starts with a pipe (a documented
/// deviation, see `docs/spec/deviations.md`). Targets that compare our output with
/// upstream's with tables on skip exactly these inputs; every other input, and every
/// table whose header row starts with a pipe, is still compared. A table at the start of
/// a block, where upstream agrees, is skipped too: the narrowest test that needs no
/// knowledge of upstream's parse.
pub fn table_without_leading_pipe(s: &str) -> bool {
    parse_with_offsets(s, GFM).any(|(e, range)| {
        matches!(e, Event::Start(Tag::Table(_)))
            && s[range.start..]
                .trim_start_matches([' ', '\t'])
                .as_bytes()
                .first()
                != Some(&b'|')
    })
}

/// The column after `s[from..to]`, a tab advancing to the next multiple of 4.
fn columns(s: &str, from: usize, to: usize) -> usize {
    s[from..to].chars().fold(0, |col, c| {
        if c == '\t' {
            (col / 4 + 1) * 4
        } else {
            col + 1
        }
    })
}

/// True when our GFM parse of `s` opens a table whose header row is a lazy continuation
/// line: the source of the header's line, up to the table's start, holds fewer `>`
/// markers than the block quotes enclosing the table, or is indented less than the content
/// of a list item enclosing it whose first line is an earlier one.
///
/// Rubrica opens such a table inside the quote or item, as cmark-gfm does (the header row
/// continues the container's paragraph lazily and the delimiter row is inside the
/// container); upstream never lets a table interrupt a paragraph through a lazy line. The
/// same documented deviation as `table_without_leading_pipe`, reached through a lazy line.
pub fn table_with_lazy_header(s: &str) -> bool {
    // The open containers: `None` for a block quote, `Some((line start, content column))`
    // for a list item, the column where its content starts on its first line.
    let mut open: Vec<Option<(usize, usize)>> = Vec::new();
    let line_start_of = |at: usize| s[..at].rfind(['\n', '\r']).map_or(0, |i| i + 1);
    let skip_blanks =
        |at: usize| at + (s[at..].len() - s[at..].trim_start_matches([' ', '\t']).len());
    for (e, range) in parse_with_offsets(s, GFM) {
        match e {
            Event::Start(Tag::BlockQuote(_)) => open.push(None),
            Event::Start(Tag::Item) => {
                let line = line_start_of(range.start);
                let marker = skip_blanks(range.start);
                let digits = s[marker..].bytes().take_while(u8::is_ascii_digit).count();
                let content = skip_blanks(marker + digits + 1);
                open.push(Some((line, columns(s, line, content))));
            }
            Event::End(TagEnd::BlockQuote(_) | TagEnd::Item) => {
                open.pop();
            }
            Event::Start(Tag::Table(_)) if !open.is_empty() => {
                let line_start = line_start_of(range.start);
                let quotes = open.iter().filter(|c| c.is_none()).count();
                if s[line_start..range.start].matches('>').count() < quotes {
                    return true;
                }
                let indent = columns(s, line_start, range.start);
                if open
                    .iter()
                    .flatten()
                    .any(|&(line, content)| line < line_start && indent < content)
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// The documented table divergence from upstream, either way it is reached.
pub fn documented_table_divergence(s: &str) -> bool {
    table_without_leading_pipe(s) || table_with_lazy_header(s)
}
