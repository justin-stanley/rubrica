//! Headings and task items collected from the events in one walk, in document order:
//! today's heading walk (`markdown.rs:500-534`) with `heading_text` (`heading.rs:43-63`)
//! as a fold over a heading's inner events.

use std::ops::Range;

use rubrica_markdown::{Event, Tag, TagEnd};

use super::slug::NativeSlugs;
use super::{Events, Profile};
use crate::heading::Heading;
use crate::lines::LineTable;
use crate::markdown::TaskItem;
use crate::sec::RenderFinding;
use crate::slug::Slugs;

/// What the walk collected.
pub(crate) struct Walk {
    /// Every heading, with its id, `sec` not yet set.
    pub(crate) headings: Vec<Heading>,
    /// Every task item, with the index of the heading it sits under.
    pub(crate) task_items: Vec<TaskItem>,
    /// `InvisibleHeading`, in document order.
    pub(crate) findings: Vec<RenderFinding>,
}

/// The rendered text of a heading, from the events between its `Start` and `End`: what
/// Wiki.js slugs (the blessed pre-switch render's heading text). Text and code spans are literal; a soft
/// or hard break is `"\n"`; an image's subtree, an inline HTML tag and a footnote
/// reference contribute nothing; emphasis, strong, strikethrough and links contribute
/// their content. Trimmed.
pub(crate) fn text(inner: &[(Event, Range<usize>)]) -> String {
    let mut out = String::new();
    // How many images are open: their subtrees contribute nothing.
    let mut images = 0usize;
    for (event, _) in inner {
        match event {
            Event::Start(Tag::Image { .. }) => images += 1,
            Event::End(TagEnd::Image) => images = images.saturating_sub(1),
            _ if images > 0 => {}
            Event::Text(t) => out.push_str(t),
            Event::Code(c) => out.push_str(c),
            Event::SoftBreak | Event::HardBreak => out.push('\n'),
            _ => {}
        }
    }
    out.trim().to_owned()
}

/// Walks `events` once: each heading's text, `InvisibleHeading` where the text is
/// non-empty and all invisible, its id (`Slugs::heading_id` in compat,
/// [`super::slug::NativeSlugs`] in native; `None` where the id would clobber the DOM,
/// which the dedupe counts anyway), and each task item (at its marker's line) in
/// document order with the headings, so `section` is the heading before it.
pub(crate) fn collect(events: &Events<'_>, table: &LineTable, profile: Profile) -> Walk {
    let mut slugs = Slugs::new();
    let mut native = NativeSlugs::new();
    let mut walk = Walk {
        headings: Vec::new(),
        task_items: Vec::new(),
        findings: Vec::new(),
    };
    let mut i = 0;
    while i < events.len() {
        let (event, range) = &events[i];
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                let end = events[i..]
                    .iter()
                    .position(|(e, _)| matches!(e, Event::End(TagEnd::Heading(_))))
                    .map_or(events.len(), |k| i + k);
                let text = text(&events[i + 1..end]);
                let line = table.line(range.start);
                if !text.is_empty() && text.chars().all(is_invisible) {
                    walk.findings.push(RenderFinding::InvisibleHeading { line });
                }
                let id = match profile {
                    Profile::WikijsCompat => {
                        let id = slugs.heading_id(&text);
                        (!id.dropped).then_some(id.id)
                    }
                    Profile::Native => native.heading_id(&text),
                };
                walk.headings.push(Heading {
                    level: *level as u8,
                    text,
                    id,
                    line,
                    sec: None,
                });
                i = end + 1;
                continue;
            }
            // The marker's own line: a tab-indented nested item's `Start(Item)` range
            // begins at the line end before it.
            Event::TaskListMarker(checked) => walk.task_items.push(TaskItem {
                line: table.line(range.start),
                checked: *checked,
                section: walk.headings.len().checked_sub(1),
            }),
            _ => {}
        }
        i += 1;
    }
    walk
}
/// Characters no reader sees: whitespace and Unicode's default-ignorable code points
/// (the same set the original `is_invisible` in `markdown.rs` used).
pub(crate) fn is_invisible(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '\u{ad}'
                | '\u{34f}'
                | '\u{61c}'
                | '\u{115f}'..='\u{1160}'
                | '\u{17b4}'..='\u{17b5}'
                | '\u{180b}'..='\u{180f}'
                | '\u{200b}'..='\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2060}'..='\u{206f}'
                | '\u{3164}'
                | '\u{fe00}'..='\u{fe0f}'
                | '\u{feff}'
                | '\u{ffa0}'
                | '\u{fff0}'..='\u{fff8}'
                | '\u{1bca0}'..='\u{1bca3}'
                | '\u{1d173}'..='\u{1d17a}'
                | '\u{e0000}'..='\u{e0fff}'
        )
}

#[cfg(test)]
mod tests {
    use rubrica_markdown::{Tag, TagEnd};

    use super::super::testing::compat;
    use super::*;

    /// 50 headings and their rendered text.
    const HEADINGS: &[(&str, &str)] = &[
        ("# Plain\n", "Plain"),
        ("## Two words here\n", "Two words here"),
        ("### Trailing hashes ###\n", "Trailing hashes"),
        (
            "#### Lead  and  inner   spaces   \n",
            "Lead  and  inner   spaces",
        ),
        ("# `code span`\n", "code span"),
        ("# a `b` c\n", "a b c"),
        ("# ![alt](x.png)\n", ""),
        ("# before ![alt](x.png) after\n", "before  after"),
        ("# <b>bold</b> tag\n", "bold tag"),
        ("# text <!-- c --> more\n", "text  more"),
        ("# ref[^1]\n\n[^1]: note\n", "ref"),
        ("# *em* **strong** ~~del~~\n", "em strong del"),
        ("# [link](/u) and [ref][r]\n\n[r]: /v\n", "link and ref"),
        ("# a &amp; b &lt; c\n", "a & b < c"),
        ("# a \\* b \\_ c\n", "a * b _ c"),
        ("# &#65;&#x42;C\n", "ABC"),
        ("# &nbsp;nbsp padded&nbsp;\n", "nbsp padded"),
        ("#\n", ""),
        ("# #\n", ""),
        ("##  \n", ""),
        ("Setext one\n==========\n", "Setext one"),
        ("Setext two\n----------\n", "Setext two"),
        ("Soft\nbreak\n===\n", "Soft\nbreak"),
        ("Hard  \nbreak\n===\n", "Hard\nbreak"),
        ("Back\\\nslash\n===\n", "Back\nslash"),
        ("# www.example.com\n", "www.example.com"),
        ("# mail me at a@b.c\n", "mail me at a@b.c"),
        ("# <https://x.y/z>\n", "https://x.y/z"),
        ("# 日本語の見出し\n", "日本語の見出し"),
        ("# Ünïcode Test!\n", "Ünïcode Test!"),
        ("# \u{200b}\u{200b}\n", "\u{200b}\u{200b}"),
        ("# a\u{200b}b\n", "a\u{200b}b"),
        ("# emoji 🎉 party\n", "emoji 🎉 party"),
        ("# [![badge](b.png)](/u)\n", ""),
        ("# ![outer ![inner](i.png)](o.png) x\n", "x"),
        ("# a<br>b\n", "ab"),
        ("# `a`b`c`\n", "abc"),
        ("# 1. not a list\n", "1. not a list"),
        ("# - not a bullet\n", "- not a bullet"),
        ("# > not a quote\n", "> not a quote"),
        ("# It's v1.2\n", "It's v1.2"),
        ("# tab\tinside\n", "tab\tinside"),
        ("> # quoted heading\n", "quoted heading"),
        ("- # in a list item\n", "in a list item"),
        ("# a *b `c` d* e\n", "a b c d e"),
        ("# __u__ _i_\n", "u i"),
        ("# x [^1] y [^1]\n\n[^1]: n\n", "x  y"),
        ("# &copy; 2026\n", "© 2026"),
        ("# one\\\ntwo\n", "one\\"),
        ("# trailing space \n", "trailing space"),
    ];

    /// The events between the first heading's `Start` and `End`.
    fn inner(ev: &Events<'_>) -> Vec<(Event<'static>, Range<usize>)> {
        let start = ev
            .iter()
            .position(|(e, _)| matches!(e, Event::Start(Tag::Heading { .. })))
            .expect("a heading");
        let end = ev
            .iter()
            .position(|(e, _)| matches!(e, Event::End(TagEnd::Heading(_))))
            .expect("its end");
        ev[start + 1..end]
            .iter()
            .map(|(e, r)| (e.clone().into_static(), r.clone()))
            .collect()
    }

    /// The item's `Start` range of a tab-indented nested item begins at the line end
    /// before it; the task's line is the marker's own line.
    #[test]
    fn a_tab_indented_task_item_is_on_its_own_line() {
        let md = "- a\n\t- [ ] b\n";
        let mut ev = compat(md);
        let table = LineTable::new(md);
        super::super::tasks::mark(&mut ev, md, &table, Profile::WikijsCompat);
        let walk = collect(&ev, &table, Profile::WikijsCompat);
        assert_eq!(
            walk.task_items,
            vec![TaskItem {
                line: 2,
                checked: false,
                section: None
            }]
        );
    }

    #[test]
    fn heading_text_fold_matches_the_ast_walk() {
        assert_eq!(HEADINGS.len(), 50);
        for (md, want) in HEADINGS {
            let ev = compat(md);
            let got = text(&inner(&ev));
            assert_eq!(got, *want, "{md:?}: the literal");
        }
    }
}
