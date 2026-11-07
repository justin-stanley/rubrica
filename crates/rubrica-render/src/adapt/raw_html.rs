//! Pass 1 of the sanitizer on every raw HTML event: today's `sanitize_raw_html`
//! (`markdown.rs:701-717`), one [`filter_raw_html`] call per node.

use std::mem;

use rubrica_markdown::{CowStr, Event, Tag, TagEnd};

use super::Events;
use crate::lines::LineTable;
use crate::sanitize::{filter_raw_html, RawState};
use crate::sec::RenderFinding;

/// Filters each HTML block and each `InlineHtml` event through [`filter_raw_html`] with
/// the line it starts on, in document order through one `RawState`, and returns what the
/// filter found. A block is filtered whole, as today's node is: its `Html` line events
/// are concatenated, filtered once at the block's start line, and replaced by one `Html`
/// event at the first line's place (a tag or comment that spans lines is one token to
/// the tokenizer either way; filtered per line, `<!--⏎-->` would be a dropped `<!--` and
/// the text `--&gt;`); the block's other events (an indented block's indent `Text`)
/// stay where they are. An `InlineHtml` is one tag and is filtered per event.
pub(crate) fn filter(events: &mut Events<'_>, table: &LineTable) -> Vec<RenderFinding> {
    let mut state = RawState::default();
    let taken = mem::take(events);
    let mut out: Events<'_> = Vec::with_capacity(taken.len());
    let mut i = 0;
    while i < taken.len() {
        let (event, range) = &taken[i];
        match event {
            Event::Start(Tag::HtmlBlock) => {
                let end = taken[i..]
                    .iter()
                    .position(|(e, _)| matches!(e, Event::End(TagEnd::HtmlBlock)))
                    .map_or(taken.len(), |k| i + k);
                let literal: String = taken[i + 1..end]
                    .iter()
                    .filter_map(|(e, _)| match e {
                        Event::Html(t) => Some(&**t),
                        _ => None,
                    })
                    .collect();
                let filtered = filter_raw_html(&literal, table.line(range.start), &mut state);
                out.push(taken[i].clone());
                // The other events (an indented block's indent `Text`) stay where they
                // are; the filtered text takes the first `Html`'s place.
                let mut filtered = Some(filtered);
                for pair in &taken[i + 1..end] {
                    match &pair.0 {
                        Event::Html(_) => {
                            if let Some(f) = filtered.take() {
                                out.push((Event::Html(CowStr::from(f)), range.clone()));
                            }
                        }
                        _ => out.push(pair.clone()),
                    }
                }
                i = end;
            }
            Event::InlineHtml(raw) => {
                let filtered = filter_raw_html(raw, table.line(range.start), &mut state);
                out.push((Event::InlineHtml(CowStr::from(filtered)), range.clone()));
                i += 1;
            }
            _ => {
                out.push(taken[i].clone());
                i += 1;
            }
        }
    }
    *events = out;
    state.findings
}

#[cfg(test)]
mod tests {
    use rubrica_markdown::{Event, Tag, TagEnd};

    use super::super::testing::compat;
    use super::*;
    use crate::sanitize::{filter_raw_html, RawState, MAX_RAW_ATTRIBUTES};

    /// The `Html` texts of `ev`, in order.
    fn html_texts(ev: &Events<'_>) -> Vec<String> {
        ev.iter()
            .filter_map(|(e, _)| match e {
                Event::Html(t) => Some(t.to_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn raw_html_is_filtered_per_event_with_its_line() {
        // Line 1: a kept tag. Line 5: a tag with too many attributes, written as text with
        // a finding at its line.
        let attrs = (0..=MAX_RAW_ATTRIBUTES)
            .map(|i| format!(" a{i}=v"))
            .collect::<String>();
        let line1 = "<details>\n";
        let line5 = format!("<kbd{attrs}>\n");
        let md = format!("{line1}\n\n\n{line5}");
        let mut ev = compat(&md);
        let findings = filter(&mut ev, &LineTable::new(&md));
        let mut state = RawState::default();
        let direct = vec![
            filter_raw_html(line1, 1, &mut state),
            filter_raw_html(&line5, 5, &mut state),
        ];
        assert_eq!(html_texts(&ev), direct);
        assert_eq!(findings, state.findings);
        assert_eq!(
            findings,
            vec![RenderFinding::RawHtmlTooManyAttributes { line: 5 }]
        );
    }

    /// An indented HTML block's indent is a `Text` inside the block, before its
    /// `Html`; the filter leaves it where it is and puts the filtered text at the
    /// first `Html` event's position.
    #[test]
    fn an_indented_html_blocks_indent_stays_first() {
        let md = " <div>\n\npara\n";
        let mut ev = compat(md);
        let findings = filter(&mut ev, &LineTable::new(md));
        assert_eq!(findings, vec![]);
        let direct = filter_raw_html("<div>\n", 1, &mut RawState::default());
        assert_eq!(
            ev[..4],
            [
                (Event::Start(Tag::HtmlBlock), 1..7),
                (Event::Text(" ".into()), 1..1),
                (Event::Html(direct.into()), 1..7),
                (Event::End(TagEnd::HtmlBlock), 1..7),
            ]
        );
    }

    #[test]
    fn a_multi_line_html_block_is_filtered_whole() {
        let md = "<!--\n-->\n\npara\n";
        let mut ev = compat(md);
        assert_eq!(html_texts(&ev), vec!["<!--\n", "-->\n"]);
        let findings = filter(&mut ev, &LineTable::new(md));
        assert_eq!(findings, vec![]);
        // One comment, dropped whole (its trailing line end is text and stays, as in
        // today's call on the block's literal): one `Html` event over the block's range.
        let direct = filter_raw_html("<!--\n-->\n", 1, &mut RawState::default());
        assert_eq!(direct, "\n");
        assert_eq!(
            ev[..3],
            [
                (Event::Start(Tag::HtmlBlock), 0..9),
                (Event::Html(direct.into()), 0..9),
                (Event::End(TagEnd::HtmlBlock), 0..9),
            ]
        );
    }
}
