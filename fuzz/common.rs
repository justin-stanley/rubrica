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

/// True when our GFM parse of `s` opens a table whose header row is a lazy continuation
/// line: the source of the header's line, up to the table's start, holds fewer `>`
/// markers than the block quotes enclosing the table.
///
/// Rubrica opens such a table inside the quote, as cmark-gfm does (the header row
/// continues the quote's paragraph lazily and the delimiter row is inside the quote);
/// upstream never lets a table interrupt a paragraph through a lazy line. The same
/// documented deviation as `table_without_leading_pipe`, reached through a lazy line.
pub fn table_with_lazy_header(s: &str) -> bool {
    let mut quotes = 0usize;
    for (e, range) in parse_with_offsets(s, GFM) {
        match e {
            Event::Start(Tag::BlockQuote(_)) => quotes += 1,
            Event::End(TagEnd::BlockQuote(_)) => quotes -= 1,
            Event::Start(Tag::Table(_)) if quotes > 0 => {
                let line_start = s[..range.start].rfind(['\n', '\r']).map_or(0, |i| i + 1);
                let markers = s[line_start..range.start].matches('>').count();
                if markers < quotes {
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
