//! Single tildes put back as text (compat): today's `unstrike_single_tildes`
//! (`markdown.rs:906-933`). markdown-it strikes through `~~x~~` only; GFM also strikes
//! `~x~`, which the corpus writes for "about" (`~$500`).

use rubrica_markdown::{Event, Tag, TagEnd};

use super::Events;

/// For each `Start(Strikethrough)` whose source at `range.start` is a `~` followed by a
/// byte that is not `~`, the `Start` becomes `Text("~")` over that byte and the matching
/// `End` becomes `Text("~")` over the span's last byte; Start/End are matched by depth.
pub(crate) fn unstrike_single(events: &mut Events<'_>, body: &str) {
    let b = body.as_bytes();
    // Per open strikethrough, innermost last: whether it is a single-tilde one.
    let mut open: Vec<bool> = Vec::new();
    for (event, range) in events.iter_mut() {
        match event {
            Event::Start(Tag::Strikethrough) => {
                let single =
                    b.get(range.start) == Some(&b'~') && b.get(range.start + 1) != Some(&b'~');
                open.push(single);
                if single {
                    *event = Event::Text("~".into());
                    *range = range.start..range.start + 1;
                }
            }
            Event::End(TagEnd::Strikethrough) => {
                let single = open.pop() == Some(true);
                if single {
                    *event = Event::Text("~".into());
                    *range = range.end - 1..range.end;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use rubrica_markdown::{Event, Tag, TagEnd};

    use super::super::testing::compat;
    use super::*;

    /// The events inside the one paragraph of `md`, after the adapter.
    fn inner(md: &str) -> Events<'_> {
        let mut ev = compat(md);
        unstrike_single(&mut ev, md);
        assert_eq!(
            ev.first().map(|e| &e.0),
            Some(&Event::Start(Tag::Paragraph))
        );
        assert_eq!(
            ev.last().map(|e| &e.0),
            Some(&Event::End(TagEnd::Paragraph))
        );
        ev[1..ev.len() - 1].to_vec()
    }

    #[test]
    fn single_tilde_becomes_text_double_stays() {
        assert_eq!(
            inner("~a~"),
            vec![
                (Event::Text("~".into()), 0..1),
                (Event::Text("a".into()), 1..2),
                (Event::Text("~".into()), 2..3),
            ]
        );
        let before = compat("~~a~~");
        let mut after = before.clone();
        unstrike_single(&mut after, "~~a~~");
        assert_eq!(after, before);
        assert!(after
            .iter()
            .any(|(e, _)| matches!(e, Event::Start(Tag::Strikethrough))));
    }

    #[test]
    fn nested_strikethroughs_match_their_ends() {
        assert_eq!(
            inner("~a ~~b~~ c~"),
            vec![
                (Event::Text("~".into()), 0..1),
                (Event::Text("a ".into()), 1..3),
                (Event::Start(Tag::Strikethrough), 3..8),
                (Event::Text("b".into()), 5..6),
                (Event::End(TagEnd::Strikethrough), 3..8),
                (Event::Text(" c".into()), 8..10),
                (Event::Text("~".into()), 10..11),
            ]
        );
    }
}
