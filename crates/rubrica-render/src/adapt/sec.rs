//! `@sec` markers over events: today's `take_markers` (`sec.rs:535-626`) and
//! `attach_markers` (`markdown.rs:731-794`), ported decision for decision. [`take`] runs
//! before the raw HTML filter, as today (the filter drops comments, so a marker filtered
//! first would be no marker); [`attach`] runs after the headings are collected, since it
//! reads their ids and text.

use std::collections::{HashMap, HashSet};
use std::mem;

use rubrica_markdown::{Event, Tag, TagEnd};

use super::headings::is_invisible;
use super::Events;
use crate::heading::{escape_attr, Heading};
use crate::lines::LineTable;
use crate::sanitize::{attribute_cost, MAX_ATTRIBUTE_COST};
use crate::sec::{parse_marker, HarvestedMarker, RenderFinding, SecHidden, SecMarker};

/// A marker found in the events, and the heading it names, if any (today's
/// `crate::sec::Found`, with the heading's ordinal in place of its node).
pub(crate) struct Found {
    pub(crate) marker: Result<SecMarker, usize>,
    /// The ordinal (index into the collected headings) of the ATX heading that
    /// immediately follows the marker.
    pub(crate) heading: Option<usize>,
    /// Inside a blockquote, list item or footnote definition.
    pub(crate) in_container: bool,
    /// `check-packages.cjs` harvests this marker too, on its line.
    pub(crate) seen_by_check_packages: bool,
}

/// The markers of a page: those the events had (taken out), check-packages' ids, and
/// what it harvests that the parse does not see (today's `crate::sec::Markers`).
pub(crate) struct Taken {
    pub(crate) found: Vec<Found>,
    pub(crate) secs: Vec<SecMarker>,
    pub(crate) findings: Vec<RenderFinding>,
}

/// Takes every marker-shaped HTML block out of the events (attached or not, as today's
/// `take_markers` detaches every one): its `Html` text is blanked and its other events
/// dropped, so the block stays as `Start(HtmlBlock)`, `Html("")`, `End` and writes
/// nothing, while the paragraphs on either side of it in a tight item (which has no
/// `Paragraph` events) stay apart as today's nodes do. The heading each names: the next
/// event after the block is `Start(Heading)`, it is ATX (a single-line heading, or one
/// whose source at its start is `#{1,6}` then a blank or the end; a setext heading whose
/// text starts with `#` is neither), it starts on the line after the marker's, and the
/// marker's line did not end in a bare `\r`. A block inside a container is
/// `in_container`. Each is paired with `harvest` by line and
/// id; a harvested marker with no pair is `SecNotRendered` (by what the events have on
/// its line: a code block, another HTML block, or text) or `SecWithoutId`.
pub(crate) fn take(
    events: &mut Events<'_>,
    body: &str,
    table: &LineTable,
    harvest: Vec<HarvestedMarker>,
    bare_cr: &[usize],
) -> Taken {
    let key = |line: usize, m: Option<&SecMarker>| (line, m.map(|m| m.id.clone()));
    // The last line of a block whose range ends after its last byte.
    let last_line =
        |range: &std::ops::Range<usize>| table.line(range.end.saturating_sub(1).max(range.start));
    let mut found = Vec::new();
    // The event index ranges of the marker blocks, to blank.
    let mut blocks: Vec<std::ops::Range<usize>> = Vec::new();
    // The lines of each code block and each HTML block that is no marker.
    let mut hidden: Vec<(usize, usize, SecHidden)> = Vec::new();
    let mut depth = 0usize;
    let mut heading_ordinal = 0usize;
    for (i, (event, range)) in events.iter().enumerate() {
        match event {
            Event::Start(
                Tag::BlockQuote(_) | Tag::List(_) | Tag::Item | Tag::FootnoteDefinition(_),
            ) => {
                depth += 1;
            }
            Event::End(
                TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item | TagEnd::FootnoteDefinition,
            ) => {
                depth -= 1;
            }
            Event::Start(Tag::Heading { .. }) => heading_ordinal += 1,
            Event::Start(Tag::CodeBlock(_)) => {
                hidden.push((table.line(range.start), last_line(range), SecHidden::Code));
            }
            Event::Start(Tag::HtmlBlock) => {
                let (start, end) = (table.line(range.start), last_line(range));
                let close = events[i..]
                    .iter()
                    .position(|(e, _)| matches!(e, Event::End(TagEnd::HtmlBlock)))
                    .map_or(events.len() - 1, |k| i + k);
                let literal: String = events[i + 1..close]
                    .iter()
                    .filter_map(|(e, _)| match e {
                        Event::Html(t) => Some(&**t),
                        _ => None,
                    })
                    .collect();
                let Some(marker) = parse_marker(&literal, start) else {
                    hidden.push((start, end, SecHidden::HtmlBlock));
                    continue;
                };
                let heading = events.get(close + 1).filter(|(e, h)| {
                    matches!(e, Event::Start(Tag::Heading { .. }))
                        && is_atx(body, table, h)
                        && table.line(h.start) == end + 1
                        && bare_cr.binary_search(&end).is_err()
                });
                blocks.push(i..close + 1);
                found.push(Found {
                    marker,
                    heading: heading.map(|_| heading_ordinal),
                    in_container: depth > 0,
                    seen_by_check_packages: false,
                });
            }
            _ => {}
        }
    }
    if !blocks.is_empty() {
        let taken = mem::take(events);
        let (mut k, mut blanked) = (0, usize::MAX);
        for (i, (event, range)) in taken.into_iter().enumerate() {
            while k < blocks.len() && i >= blocks[k].end {
                k += 1;
            }
            if k < blocks.len() && blocks[k].contains(&i) {
                // The block's `Start` and `End` stay; its first `Html` is blanked, the
                // rest (more `Html` lines, an indent `Text`) dropped.
                match event {
                    Event::Start(Tag::HtmlBlock) | Event::End(TagEnd::HtmlBlock) => {
                        events.push((event, range));
                    }
                    Event::Html(_) if blanked != k => {
                        blanked = k;
                        events.push((Event::Html("".into()), range));
                    }
                    _ => {}
                }
                continue;
            }
            events.push((event, range));
        }
    }
    // Pair by (line, id): each harvested marker with at most one found.
    let mut harvested: HashMap<(usize, Option<String>), usize> = HashMap::new();
    for h in &harvest {
        *harvested.entry(key(h.line, h.marker.as_ref())).or_default() += 1;
    }
    let mut parsed: HashMap<(usize, Option<String>), usize> = HashMap::new();
    for f in &mut found {
        let k = match &f.marker {
            Ok(m) => key(m.line, Some(m)),
            Err(line) => key(*line, None),
        };
        if let Some(n) = harvested.get_mut(&k).filter(|n| **n > 0) {
            *n -= 1;
            f.seen_by_check_packages = true;
            *parsed.entry(k).or_default() += 1;
        }
    }
    hidden.sort_unstable_by_key(|h| (h.0, h.1));
    let why = |line: usize| {
        let at = hidden.partition_point(|h| h.0 <= line);
        at.checked_sub(1)
            .map(|i| hidden[i])
            .filter(|h| h.1 >= line)
            .map_or(SecHidden::Text, |h| h.2)
    };
    let mut findings = Vec::new();
    for h in &harvest {
        let k = key(h.line, h.marker.as_ref());
        if let Some(n) = parsed.get_mut(&k).filter(|n| **n > 0) {
            *n -= 1;
            continue;
        }
        findings.push(match &h.marker {
            Some(m) => RenderFinding::SecNotRendered {
                id: m.id.clone(),
                line: h.line,
                why: why(h.line),
            },
            None => RenderFinding::SecWithoutId { line: h.line },
        });
    }
    Taken {
        found,
        secs: harvest.into_iter().filter_map(|h| h.marker).collect(),
        findings,
    }
}

/// Whether the heading starting at `h` is ATX (today's `!nh.setext`): a single-line
/// heading, or one whose source at its start is one to six `#` then a blank, tab or the
/// end (a setext heading's text may start with `#`, and spans its underline's line).
fn is_atx(body: &str, table: &LineTable, h: &std::ops::Range<usize>) -> bool {
    if table.line(h.end.saturating_sub(1).max(h.start)) == table.line(h.start) {
        return true;
    }
    let b = &body.as_bytes()[h.start..];
    let hashes = b.iter().take_while(|&&c| c == b'#').count();
    (1..=6).contains(&hashes) && matches!(b.get(hashes), None | Some(b' ' | b'\t' | b'\n'))
}

/// Puts each marker on the heading it names, if its anchor can go there (today's
/// `attach_markers`, decision for decision): the marker has an id, is outside any
/// container, `check-packages.cjs` harvests it, its id is the first on the page, it
/// names a heading, `sec-<id>` is no heading's id, the heading has visible text, and the
/// anchor's id is not too costly to write. Returns the findings for those that cannot.
pub(crate) fn attach(found: Vec<Found>, headings: &mut [Heading]) -> Vec<RenderFinding> {
    let heading_ids: HashSet<&str> = headings.iter().filter_map(|h| h.id.as_deref()).collect();
    let mut first_line: HashMap<String, usize> = HashMap::new();
    let (mut findings, mut attach) = (Vec::new(), Vec::new());
    for f in found {
        let marker = match f.marker {
            Ok(m) => m,
            Err(line) => {
                findings.push(RenderFinding::SecWithoutId { line });
                continue;
            }
        };
        let (id, line) = (marker.id.clone(), marker.line);
        if f.in_container {
            findings.push(RenderFinding::SecInContainer { id, line });
            continue;
        }
        if !f.seen_by_check_packages {
            findings.push(RenderFinding::SecNotSeenByCheckPackages { id, line });
            continue;
        }
        if let Some(&first) = first_line.get(&id) {
            findings.push(RenderFinding::DuplicateSecId { id, line, first });
            continue;
        }
        first_line.insert(id.clone(), line);
        let Some(i) = f.heading else {
            findings.push(RenderFinding::SecWithoutHeading { id, line });
            continue;
        };
        if heading_ids.contains(marker.anchor().as_str()) {
            findings.push(RenderFinding::SecAnchorIsAHeadingId { id, line });
            continue;
        }
        if headings[i].text.chars().all(is_invisible) {
            findings.push(RenderFinding::SecOnEmptyHeading { id, line });
            continue;
        }
        // As the final pass would read the anchor's `id="…"`: one it would drop is not
        // written, so `Heading::sec` is what the HTML has.
        if attribute_cost(escape_attr(&marker.anchor()).as_bytes()) > MAX_ATTRIBUTE_COST {
            findings.push(RenderFinding::AttributesTooCostly {
                element: "span".into(),
                attribute: "id".into(),
                line,
            });
            continue;
        }
        attach.push((i, marker));
    }
    for (i, marker) in attach {
        headings[i].sec = Some(marker);
    }
    findings
}

#[cfg(test)]
mod tests {
    use rubrica_markdown::Event;

    use super::super::testing::compat;
    use super::super::{heading_attrs, headings, Profile};
    use super::*;
    use crate::sec::check_packages_markers;

    /// `take` over `md`'s compat events with its own harvest; the events after, and the
    /// result.
    fn taken(md: &str) -> (Events<'_>, Taken) {
        let mut ev = compat(md);
        let table = LineTable::new(md);
        let out = take(&mut ev, md, &table, check_packages_markers(md), &[]);
        (ev, out)
    }

    /// Whether any `Html` event still carries the marker's text (a taken marker block
    /// stays as `Start(HtmlBlock)`, `Html("")`, `End`, so the blocks around it in a
    /// tight item keep their paragraphs apart).
    fn has_marker_text(ev: &Events<'_>) -> bool {
        ev.iter()
            .any(|(e, _)| matches!(e, Event::Html(t) if t.contains("@sec")))
    }

    /// Headings collected, markers attached: the attach findings and the heading attrs.
    fn attached(
        md: &str,
    ) -> (
        Events<'_>,
        Vec<RenderFinding>,
        Vec<crate::writer::HeadingAttrs>,
    ) {
        let (ev, taken) = taken(md);
        let table = LineTable::new(md);
        let mut walk = headings::collect(&ev, &table, Profile::WikijsCompat);
        let findings = attach(taken.found, &mut walk.headings);
        let attrs = heading_attrs(&walk.headings);
        (ev, findings, attrs)
    }

    #[test]
    fn a_marker_before_an_atx_heading_is_removed_and_attached() {
        let md = "<!-- @sec id=a tags=x,y -->\n# h\n";
        let (ev, taken) = taken(md);
        assert!(!has_marker_text(&ev), "the marker's text is gone: {ev:?}");
        assert_eq!(taken.found.len(), 1);
        assert_eq!(taken.found[0].heading, Some(0));
        assert!(taken.found[0].seen_by_check_packages);
        assert!(!taken.found[0].in_container);
        assert_eq!(taken.findings, vec![]);
        assert_eq!(
            taken.secs,
            vec![SecMarker {
                id: "a".into(),
                tags: vec!["x".into(), "y".into()],
                line: 1
            }]
        );
        let (_, findings, attrs) = attached(md);
        assert_eq!(findings, vec![]);
        assert_eq!(attrs[0].sec_anchor.as_deref(), Some("sec-a"));
        assert_eq!(attrs[0].id.as_deref(), Some("h"));
    }

    #[test]
    fn a_marker_before_a_setext_heading_or_blank_line_is_a_finding() {
        for md in [
            "<!-- @sec id=a -->\nh\n===\n",
            "<!-- @sec id=a -->\n\n# h\n",
        ] {
            let (ev, findings, attrs) = attached(md);
            assert_eq!(
                findings,
                vec![RenderFinding::SecWithoutHeading {
                    id: "a".into(),
                    line: 1
                }],
                "{md:?}"
            );
            assert_eq!(attrs[0].sec_anchor, None, "{md:?}");
            // Every marker-shaped block is taken out, attached or not (today's rule), so
            // under `RawHtml::Escape` no comment text is written either.
            assert!(!has_marker_text(&ev), "{md:?}: the marker stays: {ev:?}");
        }
    }

    /// Today's `take_markers` detaches the marker node and the paragraphs on either
    /// side of it stay two paragraphs; in events a tight item has no `Paragraph`
    /// events, so the block must stay (blanked) to keep them apart.
    #[test]
    fn a_marker_in_a_tight_item_keeps_the_paragraphs_apart() {
        let md = "- a\n  <!-- @sec id=x -->\n  b\n";
        let (ev, taken) = taken(md);
        assert_eq!(taken.found.len(), 1);
        assert!(taken.found[0].in_container);
        assert!(!has_marker_text(&ev), "{ev:?}");
        assert_eq!(super::super::plain::text(&ev), "a\nb");
        let html = crate::writer::write(ev, &[], crate::writer::WriterOptions::WIKIJS_COMPAT).html;
        assert!(html.contains("<li>a\nb</li>"), "{html:?}");
    }

    /// A setext heading whose text starts with `#` is not an ATX heading: a marker
    /// before it names no heading (today's `!nh.setext`).
    #[test]
    fn a_setext_heading_starting_with_a_hash_is_not_atx() {
        let md = "<!-- @sec id=a -->\n#foo\n===\n";
        let (_, taken) = taken(md);
        assert_eq!(taken.found[0].heading, None);
        let (_, findings, attrs) = attached(md);
        assert_eq!(
            findings,
            vec![RenderFinding::SecWithoutHeading {
                id: "a".into(),
                line: 1
            }]
        );
        assert_eq!(attrs[0].sec_anchor, None);
    }

    #[test]
    fn a_marker_in_a_container_is_a_finding() {
        let md = "> <!-- @sec id=a -->\n> # h\n";
        let (_, taken) = taken(md);
        assert!(taken.found[0].in_container);
        let (_, findings, attrs) = attached(md);
        assert_eq!(
            findings,
            vec![RenderFinding::SecInContainer {
                id: "a".into(),
                line: 1
            }]
        );
        assert_eq!(attrs[0].sec_anchor, None);
    }
}
