//! The text a reader sees (search input): today's `plain_text` fold
//! (`markdown.rs:1883-1933`) over events, and the line clean-up `render_with` does after
//! it (`markdown.rs:546-551`).

use std::ops::Range;

use rubrica_markdown::{CodeBlockKind, Event, Tag, TagEnd};

use super::is_mermaid;

/// Text and code spans literal; a soft or hard break a space; a code block's content
/// literal plus `\n` (not a Mermaid fence's); images, inline HTML and HTML blocks
/// skipped; a table cell's end a space; every other block's end (a rule too) `\n`.
/// Then each line trimmed, empty lines dropped, joined with `\n`.
pub(crate) fn text(events: &[(Event, Range<usize>)]) -> String {
    let mut out = String::new();
    // The subtree being skipped: the `End` that closes it, and how many of the same tag
    // opened inside it (an image in an image's text).
    let mut skip: Option<(TagEnd, usize)> = None;
    // The open block tags, innermost last: inline content directly in an `Item` is a
    // tight item's paragraph, which the parser emits no `Paragraph` events for (the
    // blessed pre-switch render had a paragraph there); its end is a line end too (`pending`).
    let mut open: Vec<TagEnd> = Vec::new();
    let mut pending = false;
    for (event, _) in events {
        if let Some((until, nested)) = &mut skip {
            match event {
                Event::Start(tag) if tag.to_end() == *until => *nested += 1,
                Event::End(end) if end == until => {
                    if *nested == 0 {
                        skip = None;
                        // The skipped block was pushed when it opened; a block in a tight
                        // item ends the item's paragraph before it and after it.
                        open.pop();
                    } else {
                        *nested -= 1;
                    }
                }
                _ => {}
            }
            continue;
        }
        let tight = open.last() == Some(&TagEnd::Item);
        match event {
            Event::Start(tag) if is_block(tag.to_end()) => {
                if pending {
                    out.push('\n');
                    pending = false;
                }
                open.push(tag.to_end());
            }
            Event::End(end) if is_block(*end) => {
                if pending {
                    out.push('\n');
                    pending = false;
                }
                open.pop();
            }
            Event::Rule if pending => {
                out.push('\n');
                pending = false;
            }
            _ if tight => pending = true,
            _ => {}
        }
        match event {
            Event::Text(t) => out.push_str(t),
            Event::Code(c) => out.push_str(c),
            Event::SoftBreak | Event::HardBreak => out.push(' '),
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) if is_mermaid(info) => {
                skip = Some((TagEnd::CodeBlock, 0));
            }
            Event::Start(Tag::Image { .. }) => skip = Some((TagEnd::Image, 0)),
            Event::Start(Tag::HtmlBlock) => skip = Some((TagEnd::HtmlBlock, 0)),
            Event::End(TagEnd::TableCell) => out.push(' '),
            Event::End(end) if is_block(*end) => out.push('\n'),
            Event::Rule => out.push('\n'),
            _ => {}
        }
    }
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether a tag is a block, over the tags the parser emits (a table cell is handled
/// before this is asked).
fn is_block(end: TagEnd) -> bool {
    matches!(
        end,
        TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::BlockQuote(_)
            | TagEnd::CodeBlock
            | TagEnd::HtmlBlock
            | TagEnd::List(_)
            | TagEnd::Item
            | TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell
            | TagEnd::MetadataBlock(_)
    )
}
#[cfg(test)]
mod tests {
    use super::super::testing::compat;
    use super::*;
    use crate::markdown::{render, RenderOptions};

    /// 50 inputs and their plain text. The single-tilde and task-marker shapes are not here: on them the
    /// fold matches `render` only after the compat adapters, and they belong to the
    /// characterisation tests.
    const PLAIN: &[(&str, &str)] = &[
        ("para\n", "para"),
        ("two\nlines\n", "two lines"),
        ("hard  \nbreak\n", "hard break"),
        ("back\\\nslash\n", "back slash"),
        ("a *b* **c** ~~d~~\n", "a b c d"),
        ("# h\n\npara\n", "h\npara"),
        ("h\n===\n\npara\n", "h\npara"),
        ("- a\n- b\n", "a\nb"),
        ("1. a\n2. b\n", "a\nb"),
        ("- a\n\n- b\n", "a\nb"),
        ("- a\n  - b\n    - c\n", "a\nb\nc"),
        ("> quote\n", "quote"),
        ("> a\n>\n> b\n", "a\nb"),
        ("> > nested\n", "nested"),
        ("```\ncode\nblock\n```\n", "code\nblock"),
        ("    indented\n", "indented"),
        ("```mermaid\ngraph TD\n```\n", ""),
        ("```rust\nfn x() {}\n```\nafter\n", "fn x() {}\nafter"),
        ("`span` here\n", "span here"),
        ("![alt](x.png)\n", ""),
        ("text ![alt](x.png) more\n", "text  more"),
        ("[link](/u)\n", "link"),
        ("www.x.com and a@b.c\n", "www.x.com and a@b.c"),
        ("<b>bold</b> text\n", "bold text"),
        ("<div>\nblock\n</div>\n", ""),
        ("<!-- comment -->\n", ""),
        ("| a | b |\n|---|---|\n| c | d |\n", "a b\nc d"),
        ("| a |\n|---|\n", "a"),
        ("***\n", ""),
        ("a\n\n***\n\nb\n", "a\nb"),
        ("a &amp; b &lt; c\n", "a & b < c"),
        ("a \\* b\n", "a * b"),
        ("&#65;&#x42;\n", "AB"),
        ("ref[^1]\n\n[^1]: note\n", "ref\nnote"),
        ("[a]: /u\n[a]\n", "a"),
        ("  leading\n", "leading"),
        ("trailing   \n", "trailing"),
        ("\n\n\nblank lines\n\n\n", "blank lines"),
        ("a\n\n\n\nb\n", "a\nb"),
        ("日本語\n", "日本語"),
        ("tab\tinside\n", "tab\tinside"),
        ("x\u{a0}y\n", "x\u{a0}y"),
        ("- [ ]x\n", "[ ]x"),
        ("- [x]:\n", "[x]:"),
        ("~~a~~ ~~b~~\n", "a b"),
        ("# `c` *e*\n", "c e"),
        ("- a\n\n  para\n\n  ```\n  code\n  ```\n", "a\npara\ncode"),
        ("<kbd>k</kbd>\n", "k"),
        ("a <!-- c --> b\n", "a  b"),
        ("> - a\n> - b\n", "a\nb"),
    ];

    /// A block the fold skips (an HTML block, a Mermaid fence) still ends a tight
    /// item's paragraph: the text after it starts a new line (today's `plain_text`).
    #[test]
    fn tight_items_keep_their_line_ends_around_skipped_blocks() {
        for (md, want) in [
            ("- <!-- x -->\n  b\n  - c\n", "b\nc"),
            ("- ```mermaid\n  g\n  ```\n  b\n  - c\n", "b\nc"),
            ("- a\n  <!-- x -->\n  b\n  - c\n", "a\nb\nc"),
        ] {
            assert_eq!(text(&compat(md)), want, "{md:?}");
            assert_eq!(
                text(&compat(md)),
                render(md, &RenderOptions::wikijs_compat()).plain_text,
                "{md:?}: today's render"
            );
        }
    }

    #[test]
    fn plain_text_fold_matches_todays() {
        assert_eq!(PLAIN.len(), 50);
        for (md, want) in PLAIN {
            let got = text(&compat(md));
            assert_eq!(got, *want, "{md:?}: the literal");
        }
    }
}
