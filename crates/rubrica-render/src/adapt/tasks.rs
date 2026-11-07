//! Task items as markdown-it-task-lists 2.1.1 finds them (compat): today's
//! `wikijs_task_items` (`markdown.rs:935-1014`), on events parsed without
//! `ENABLE_TASKLISTS`. Native keeps pulldown's markers, whatever its rule says.

use std::mem;

use rubrica_markdown::{CowStr, Event, Tag, TagEnd};

use super::{Events, Profile};
use crate::lines::LineTable;

/// Compat: for each `Start(Item)` whose first block is a paragraph (a `Start(Paragraph)`,
/// or a `Text` directly in a tight item) and whose first inline is a `Text` run, the item
/// is a task when the source bytes at the run's start are `[ ] `, `[x] ` or `[X] ` and
/// either the rest of that line is non-blank or the paragraph goes on to another line (a
/// `SoftBreak` or `HardBreak` follows in it). Then a `TaskListMarker(checked)` over the
/// three marker bytes is inserted after the `Start(Item)` and the run loses its first
/// four bytes (three when the parser already dropped the trailing space before a line
/// end), and is dropped when empty: the writer emits the one space. An item whose first
/// inline is not a plain `Text` (`[x]` is a link when `[x]:` is defined) stays a plain
/// item. Native: nothing.
pub(crate) fn mark(events: &mut Events<'_>, body: &str, table: &LineTable, profile: Profile) {
    if profile == Profile::Native {
        return;
    }
    let mut out: Events<'_> = Vec::with_capacity(events.len());
    let taken = mem::take(events);
    let mut i = 0;
    while i < taken.len() {
        let (event, range) = &taken[i];
        if !matches!(event, Event::Start(Tag::Item)) {
            out.push(taken[i].clone());
            i += 1;
            continue;
        }
        // The first block: a paragraph (`Start(Paragraph)`, or a tight item's text).
        let paragraph = matches!(taken.get(i + 1), Some((Event::Start(Tag::Paragraph), _)));
        let text_at = if paragraph { i + 2 } else { i + 1 };
        let Some(task) = task_of(&taken, text_at, body, table) else {
            out.push(taken[i].clone());
            i += 1;
            continue;
        };
        let (text, text_range) = &taken[text_at];
        out.push((Event::Start(Tag::Item), range.clone()));
        out.push((
            Event::TaskListMarker(task.checked),
            text_range.start..text_range.start + 3,
        ));
        if paragraph {
            out.push(taken[i + 1].clone());
        }
        let Event::Text(t) = text else {
            unreachable!("task_of checked the first inline is a Text");
        };
        if task.cut < t.len() {
            out.push((
                Event::Text(CowStr::from(t[task.cut..].to_owned())),
                text_range.start + task.cut..text_range.end,
            ));
        }
        i = text_at + 1;
    }
    *events = out;
}

/// What makes an item at `events[text_at]` a task.
struct Task {
    checked: bool,
    /// How many bytes come off the run: the marker and its space, or the marker alone
    /// when the run ends there.
    cut: usize,
}

/// The markdown-it rule on the first inline `events[text_at]` of an item's paragraph.
fn task_of(events: &Events<'_>, text_at: usize, body: &str, table: &LineTable) -> Option<Task> {
    let (Event::Text(text), range) = events.get(text_at)? else {
        return None;
    };
    let raw = body.get(range.start..)?.as_bytes();
    let checked = match raw {
        [b'[', s @ (b' ' | b'x' | b'X'), b']', b' ', ..] => *s != b' ',
        _ => return None,
    };
    // Trimmed, the content must still start with the space: something follows on this
    // line, or on a later line of the paragraph.
    let line_end = table.line_range(table.line(range.start)).end;
    let rest = &body[range.start + 4..line_end.max(range.start + 4)];
    if rest.trim().is_empty() && !continues(events, text_at + 1) {
        return None;
    }
    // The run must spell the marker as the source does (an entity does not).
    if !text.starts_with(&body[range.start..range.start + 3]) {
        return None;
    }
    let cut = if text.as_bytes().get(3) == Some(&b' ') {
        4
    } else {
        3
    };
    Some(Task { checked, cut })
}

/// Whether the paragraph holding `events[from - 1]` goes on to another line: a line
/// break before its end (the end of the paragraph's inlines: a block `Start` or `End`).
fn continues(events: &Events<'_>, from: usize) -> bool {
    for (event, _) in &events[from..] {
        match event {
            Event::SoftBreak | Event::HardBreak => return true,
            Event::Start(tag) if !inline(tag) => return false,
            Event::End(tag) if !inline_end(*tag) => return false,
            _ => {}
        }
    }
    false
}

fn inline(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Superscript
            | Tag::Subscript
            | Tag::Link { .. }
            | Tag::Image { .. }
    )
}

fn inline_end(tag: TagEnd) -> bool {
    matches!(
        tag,
        TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::Link
            | TagEnd::Image
    )
}

#[cfg(test)]
mod tests {
    use rubrica_markdown::{Event, Tag, TagEnd};

    use super::super::testing::{compat, native};
    use super::*;

    fn marked(md: &str) -> Events<'_> {
        let mut ev = compat(md);
        mark(&mut ev, md, &LineTable::new(md), Profile::WikijsCompat);
        ev
    }

    fn has_marker(ev: &Events<'_>) -> bool {
        ev.iter()
            .any(|(e, _)| matches!(e, Event::TaskListMarker(_)))
    }

    #[test]
    fn compat_task_marker_needs_a_space_and_content() {
        assert_eq!(
            marked("- [ ] foo\n"),
            vec![
                (Event::Start(Tag::List(None)), 0..10),
                (Event::Start(Tag::Item), 0..10),
                (Event::TaskListMarker(false), 2..5),
                (Event::Text("foo".into()), 6..9),
                (Event::End(TagEnd::Item), 0..10),
                (Event::End(TagEnd::List(false)), 0..10),
            ]
        );
        let ev = marked("- [ ]x\n");
        assert!(!has_marker(&ev));
        assert_eq!(ev[2], (Event::Text("[ ]x".into()), 2..6));
        // A tab is not the space.
        assert!(!has_marker(&marked("- [x]\tfoo\n")));
        // Nothing after the marker.
        assert!(!has_marker(&marked("- [ ]\n")));
        // Checked, loose: the marker sits between the item and its paragraph.
        assert_eq!(
            marked("- [x] foo\n\n- bar\n")[..5],
            [
                (Event::Start(Tag::List(None)), 0..17),
                (Event::Start(Tag::Item), 0..11),
                (Event::TaskListMarker(true), 2..5),
                (Event::Start(Tag::Paragraph), 2..10),
                (Event::Text("foo".into()), 6..9),
            ]
        );
    }

    #[test]
    fn compat_task_marker_at_end_of_paragraph_is_text() {
        let ev = marked("- [ ] \n");
        assert!(!has_marker(&ev));
        assert_eq!(ev[2], (Event::Text("[ ]".into()), 2..5));
        // The paragraph continues: a task, its first run (just the marker) dropped.
        assert_eq!(
            marked("- [ ] \n  more\n"),
            vec![
                (Event::Start(Tag::List(None)), 0..14),
                (Event::Start(Tag::Item), 0..14),
                (Event::TaskListMarker(false), 2..5),
                (Event::SoftBreak, 6..7),
                (Event::Text("more".into()), 9..13),
                (Event::End(TagEnd::Item), 0..14),
                (Event::End(TagEnd::List(false)), 0..14),
            ]
        );
    }

    #[test]
    fn native_task_items_are_pulldowns() {
        let md = "- [ ]\n";
        let before = native(md);
        assert!(has_marker(&before), "pulldown marks `- [ ]`");
        let mut after = before.clone();
        mark(&mut after, md, &LineTable::new(md), Profile::Native);
        assert_eq!(after, before);
    }
}
