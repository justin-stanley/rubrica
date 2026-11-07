//! The footnote definitions the writer writes, decided once, before every other adapter:
//! the content of every other definition is taken out of the events, so nothing the
//! writer would drop reaches a heading, link, task item, `@sec` marker or the plain text
//! (the adapters once collected them, and a heading id from a dropped
//! definition was in `Rendered::headings` and nowhere in the HTML).
//!
//! The writer's rule (`writer.rs`, `Plan::new` and `footnotes_section`): the definition of
//! a name it writes is the first one within [`MAX_CONTAINER_DEPTH`] (names case-folded,
//! as the parser matches labels); it writes the definitions of the names referenced from
//! the body, then those referenced from a definition it writes, transitively; a reference
//! in an image's alt text is not written, so it references nothing. A definition the rule
//! leaves out keeps its `Start` and `End`, empty: the writer still sees a definition past
//! the bound (its name is reported unlinked when every definition of it is), and writes
//! exactly the definitions left with content.

use std::collections::{HashMap, HashSet};

use rubrica_markdown::{Event, Tag, TagEnd};
use unicase::UniCase;

use super::Events;
use crate::writer::MAX_CONTAINER_DEPTH;

/// One `Start(FootnoteDefinition)` .. `End(FootnoteDefinition)`, by event index.
struct Def {
    key: UniCase<String>,
    start: usize,
    end: usize,
    deep: bool,
}

fn is_container(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::BlockQuote(_) | Tag::List(_) | Tag::Item | Tag::FootnoteDefinition(_)
    )
}

fn is_container_end(tag: TagEnd) -> bool {
    matches!(
        tag,
        TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item | TagEnd::FootnoteDefinition
    )
}

/// The names referenced in `events[from..to]` as the writer writes them: outside an
/// image's alt text and outside a definition nested in the range.
fn references(
    events: &Events<'_>,
    from: usize,
    to: usize,
    def_end: &HashMap<usize, usize>,
) -> Vec<UniCase<String>> {
    let mut names = Vec::new();
    let mut image = 0usize;
    let mut i = from;
    while i < to {
        match &events[i].0 {
            Event::Start(Tag::FootnoteDefinition(_)) => {
                i = def_end.get(&i).map_or(to, |&end| end + 1);
                continue;
            }
            Event::Start(Tag::Image { .. }) => image += 1,
            Event::End(TagEnd::Image) => image = image.saturating_sub(1),
            Event::FootnoteReference(name) if image == 0 => {
                names.push(UniCase::new(name.to_string()));
            }
            _ => {}
        }
        i += 1;
    }
    names
}

/// Takes the content of every footnote definition the writer does not write out of
/// `events` (see the module documentation).
pub(crate) fn keep_written_definitions(events: &mut Events<'_>) {
    let mut defs: Vec<Def> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut depth = 0usize;
    for (i, (event, _)) in events.iter().enumerate() {
        match event {
            Event::Start(tag) => {
                if is_container(tag) {
                    depth += 1;
                }
                if let Tag::FootnoteDefinition(name) = tag {
                    defs.push(Def {
                        key: UniCase::new(name.to_string()),
                        start: i,
                        end: i,
                        deep: depth > MAX_CONTAINER_DEPTH,
                    });
                    open.push(defs.len() - 1);
                }
            }
            Event::End(tag) => {
                if *tag == TagEnd::FootnoteDefinition {
                    if let Some(d) = open.pop() {
                        defs[d].end = i;
                    }
                }
                if is_container_end(*tag) {
                    depth = depth.saturating_sub(1);
                }
            }
            _ => {}
        }
    }
    if defs.is_empty() {
        return;
    }
    let def_end: HashMap<usize, usize> = defs.iter().map(|d| (d.start, d.end)).collect();
    let mut chosen: HashMap<UniCase<String>, usize> = HashMap::new();
    for (d, def) in defs.iter().enumerate() {
        if !def.deep {
            chosen.entry(def.key.clone()).or_insert(d);
        }
    }
    let mut written = vec![false; defs.len()];
    let mut seen: HashSet<UniCase<String>> = HashSet::new();
    let mut queue = references(events, 0, events.len(), &def_end);
    while let Some(key) = queue.pop() {
        if !seen.insert(key.clone()) {
            continue;
        }
        if let Some(&d) = chosen.get(&key) {
            written[d] = true;
            queue.extend(references(events, defs[d].start + 1, defs[d].end, &def_end));
        }
    }
    if written.iter().all(|&w| w) {
        return;
    }
    let mut drop = vec![false; events.len()];
    for (def, &w) in defs.iter().zip(&written) {
        if !w {
            drop[def.start + 1..def.end]
                .iter_mut()
                .for_each(|x| *x = true);
        }
    }
    let mut i = 0;
    events.retain(|_| {
        i += 1;
        !drop[i - 1]
    });
}

#[cfg(test)]
mod tests {
    use rubrica_markdown::{events, GFM};

    use super::*;

    fn kept(md: &str) -> Vec<String> {
        let mut ev: Events<'_> = events(md, GFM).collect();
        keep_written_definitions(&mut ev);
        ev.iter()
            .filter_map(|(e, _)| match e {
                Event::Text(t) => Some(t.to_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn only_the_written_definitions_keep_their_content() {
        // The first of two.
        assert_eq!(kept("x[^a]\n\n[^a]: one\n\n[^A]: two\n"), ["x", "one"]);
        // Unreferenced, and referenced only from an unreferenced one.
        assert_eq!(kept("x\n\n[^a]: one[^b]\n\n[^b]: two\n"), ["x"]);
        // Referenced from a written one.
        assert_eq!(
            kept("x[^a]\n\n[^a]: one[^b]\n\n[^b]: two\n"),
            ["x", "one", "two"]
        );
    }
}
