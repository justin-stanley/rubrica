//! The event adapters: everything the render derives from the parse besides the HTML, as
//! passes over the `Vec<(Event, Range)>` of [`rubrica_markdown::events`], each reading
//! the source bytes at its span's offset. [`crate::markdown::render`] is
//! assembled from [`collect`] and the writer.
//!
//! The adapters run in today's order (`markdown.rs:477-568`), after the footnote
//! definitions the writer does not write are emptied ([`footnotes`]): single tildes, task items
//! (compat only), `@sec` markers taken out of the events and paired with the harvest,
//! raw HTML filtered, headings (with their ids and task items) collected, markers
//! attached to their headings, links and Mermaid sources collected, the plain text folded.
//! Findings are produced in today's order (markers, raw HTML, invisible headings, attach),
//! so a stable sort by line gives today's result.

pub(crate) mod footnotes;
pub(crate) mod headings;
pub(crate) mod plain;
pub(crate) mod raw_html;
pub(crate) mod sec;
pub(crate) mod slug;
pub(crate) mod tasks;
pub(crate) mod tildes;

use std::collections::HashSet;
use std::ops::Range;

use rubrica_markdown::{CodeBlockKind, Event, Options, Tag, TagEnd, GFM};

use crate::heading::Heading;
use crate::lines::LineTable;
use crate::markdown::{classify, Link, Profile, RawHtml, TaskItem};
use crate::sec::{HarvestedMarker, RenderFinding, SecMarker};
use crate::writer::{HeadingAttrs, Written};

/// The event vector the adapters work on.
pub(crate) type Events<'a> = Vec<(Event<'a>, Range<usize>)>;

/// The parser options per profile. Compat parses WITHOUT `ENABLE_TASKLISTS`: markdown-it's
/// task rule ([`tasks::mark`]) decides what is a task, not pulldown's (they differ on
/// `- [ ]\n` and `- [x]\tfoo`). Native is `GFM`.
pub(crate) fn parse_options(profile: Profile) -> Options {
    match profile {
        Profile::Native => GFM,
        Profile::WikijsCompat => GFM.difference(Options::ENABLE_TASKLISTS),
    }
}

/// What the adapters collected from a page body.
pub(crate) struct Collected<'a> {
    /// The events after the adapters, for the writer.
    pub events: Events<'a>,
    /// Every heading, with its id; `sec` set where a marker was attached.
    pub headings: Vec<Heading>,
    /// What the writer is told per heading: the id and the `@sec` anchor.
    pub heading_attrs: Vec<HeadingAttrs>,
    pub links: Vec<Link>,
    pub task_items: Vec<TaskItem>,
    pub mermaid_blocks: Vec<String>,
    /// The harvest's markers with an id.
    pub secs: Vec<SecMarker>,
    /// In today's order: markers, raw HTML, invisible headings, attach.
    pub findings: Vec<RenderFinding>,
    pub plain_text: String,
}

/// Parses `body` (line ends normalised, as `table` was built from it) under `profile` and
/// runs the adapters in today's order; `harvest` is check-packages' harvest of the body
/// as given and `bare_cr` the lines that ended in a bare `\r` before normalisation.
pub(crate) fn collect<'a>(
    body: &'a str,
    table: &LineTable,
    profile: Profile,
    raw: RawHtml,
    harvest: Vec<HarvestedMarker>,
    bare_cr: &[usize],
) -> Collected<'a> {
    let mut events: Events<'a> = rubrica_markdown::events(body, parse_options(profile)).collect();
    // First: what the writer will not write must reach no other adapter.
    footnotes::keep_written_definitions(&mut events);
    let mut findings = Vec::new();
    if profile == Profile::WikijsCompat {
        tildes::unstrike_single(&mut events, body);
    }
    tasks::mark(&mut events, body, table, profile);
    let taken = sec::take(&mut events, body, table, harvest, bare_cr);
    findings.extend(taken.findings);
    if raw == RawHtml::Sanitize {
        findings.extend(raw_html::filter(&mut events, table));
    }
    let walk = headings::collect(&events, table, profile);
    let mut headings = walk.headings;
    findings.extend(walk.findings);
    findings.extend(sec::attach(taken.found, &mut headings));
    let (links, mermaid_blocks) = links_and_mermaid(&events, table);
    let plain_text = plain::text(&events);
    Collected {
        heading_attrs: heading_attrs(&headings),
        events,
        headings,
        links,
        task_items: walk.task_items,
        mermaid_blocks,
        secs: taken.secs,
        findings,
        plain_text,
    }
}

/// Every link (`Start(Link)`, in document order: a reference-style link at the link's
/// position) and the source of every `mermaid` fence (its `Text` events concatenated).
fn links_and_mermaid(events: &Events<'_>, table: &LineTable) -> (Vec<Link>, Vec<String>) {
    let (mut links, mut mermaid) = (Vec::new(), Vec::new());
    let mut source: Option<String> = None;
    for (event, range) in events {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) => links.push(Link {
                href: dest_url.to_string(),
                line: table.line(range.start),
                kind: classify(dest_url),
            }),
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) if is_mermaid(info) => {
                source = Some(String::new());
            }
            Event::Text(t) => {
                if let Some(src) = &mut source {
                    src.push_str(t);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(src) = source.take() {
                    mermaid.push(src);
                }
            }
            _ => {}
        }
    }
    (links, mermaid)
}

/// A fence whose info string's first word is `mermaid` (the same test the original
/// `is_mermaid` in `markdown.rs` used). A form feed is whitespace to
/// `split_whitespace`, so today's `end_languages_at_form_feeds` rewrite changes nothing
/// here.
pub(crate) fn is_mermaid(info: &str) -> bool {
    info.split_whitespace().next() == Some("mermaid")
}

/// The position property, as a check over a finished event vector: every `Text`
/// outside a code block and an HTML block is spelled in the source exactly over its
/// range. The first violation is `Err((range, text))`; [`crate::markdown::render_strict`]
/// panics on `Err`.
///
/// The method: a walk over `body[range]` and the text together, from the range's first
/// byte. Each text byte is matched by the same source byte, or by an entity (`&…;`,
/// decoded through the parser on just that spelling, so the check has no entity table
/// of its own), and a run of whitespace matches a run of whitespace (a tab a container
/// half consumed). A run of `n` source backslashes matches `n` text backslashes
/// (verbatim, as an angle autolink's text is), `n/2` (each pair an escaped backslash;
/// an odd run's last one verbatim, or escaping the ASCII punctuation after it, which is
/// then matched too). Where a source `&` or `\` can be read two ways (`<http://a?x&amp;y>`
/// keeps its entity; `a\\b` is `a\b`), both are tried: the walk keeps a worklist of
/// `(source, text)` offsets, each pair visited once, so it is linear in the range. The
/// text must be spelled from `range.start` and some reading must end at `range.end`
/// (trailing whitespace aside). `events()` merges the parser's pieces across escapes
/// and entities, so `body[range] == text` would be wrong on `a \* b`; the walk
/// is what makes a merged run, a run the task adapter shortened or a `~` the tilde
/// adapter put back pass, and a text at the wrong offset fail. Exempt: code-block
/// content (the parser rewrites its indentation and tabs), an HTML block's `Text` (an
/// indented block's indent, which has an empty range), and any empty-range `Text`.
pub(crate) fn check_positions(
    events: &[(Event, Range<usize>)],
    body: &str,
) -> Result<(), (Range<usize>, String)> {
    let mut exempt = 0usize;
    for (event, range) in events {
        match event {
            Event::Start(Tag::CodeBlock(_) | Tag::HtmlBlock) => exempt += 1,
            Event::End(TagEnd::CodeBlock | TagEnd::HtmlBlock) => exempt = exempt.saturating_sub(1),
            Event::Text(text)
                if exempt == 0 && !range.is_empty() && !spelled(body, range, text) =>
            {
                return Err((range.clone(), text.to_string()));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Whether `text` is spelled over `body[range]` (see [`check_positions`]).
fn spelled(body: &str, range: &Range<usize>, text: &str) -> bool {
    let Some(src) = body.get(range.clone()) else {
        return false;
    };
    let (s, t) = (src.as_bytes(), text.as_bytes());
    // The readings still open, and every `(source, text)` pair already walked from.
    let mut open: Vec<(usize, usize)> = vec![(0, 0)];
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    while let Some((mut i, mut j)) = open.pop() {
        if !seen.insert((i, j)) {
            continue;
        }
        loop {
            if j == t.len() {
                if s[i..].iter().all(u8::is_ascii_whitespace) {
                    return true;
                }
                break;
            }
            let Some(&c) = s.get(i) else {
                break;
            };
            if c == b'\\' {
                let n = s[i..].iter().take_while(|&&b| b == b'\\').count();
                let m = t[j..].iter().take_while(|&&b| b == b'\\').count();
                if m >= n {
                    open.push((i + n, j + n));
                }
                if n % 2 == 0 {
                    if m >= n / 2 {
                        open.push((i + n, j + n / 2));
                    }
                } else {
                    if m >= n.div_ceil(2) {
                        open.push((i + n, j + n.div_ceil(2)));
                    }
                    // The odd one out escapes the punctuation after it.
                    let (p, q) = (i + n, j + n / 2);
                    if m >= n / 2
                        && s.get(p).is_some_and(u8::is_ascii_punctuation)
                        && s.get(p) == t.get(q)
                    {
                        open.push((p + 1, q + 1));
                    }
                }
                break;
            }
            if c == b'&' {
                if let Some((len, decoded)) = entity_at(&src[i..]) {
                    if t[j..].starts_with(decoded.as_bytes()) {
                        open.push((i + len, j + decoded.len()));
                    }
                }
            }
            if c == t[j] {
                i += 1;
                j += 1;
            } else if c.is_ascii_whitespace() && t[j].is_ascii_whitespace() {
                while s.get(i).is_some_and(u8::is_ascii_whitespace) {
                    i += 1;
                }
                while t.get(j).is_some_and(u8::is_ascii_whitespace) {
                    j += 1;
                }
            } else {
                break;
            }
        }
    }
    false
}

/// The entity reference `src` starts with, if the parser decodes one there: its length
/// and what it decodes to. The longest HTML5 entity name is 31 characters.
fn entity_at(src: &str) -> Option<(usize, String)> {
    let end = src.bytes().take(40).position(|b| b == b';')?;
    let spelling = &src[..=end];
    let decoded: String = rubrica_markdown::parse(spelling, Options::empty())
        .filter_map(|e| match e {
            Event::Text(t) => Some(t.to_string()),
            _ => None,
        })
        .collect();
    (decoded != spelling).then_some((spelling.len(), decoded))
}

/// The findings the writer's footnote records give: `FootnoteBackrefsBounded` for each
/// of [`Written::backrefs_bounded`], and `HeadingIdIsAFootnoteId` for each heading whose
/// id is one of [`Written::footnote_ids`] (today's `footnotes`, `markdown.rs:823-894`,
/// in its order: the bounded definitions in document order, then the headings).
pub(crate) fn footnote_findings(
    written: &Written,
    headings: &[Heading],
    table: &LineTable,
) -> Vec<RenderFinding> {
    let mut findings = Vec::new();
    for (name, references, src) in &written.backrefs_bounded {
        findings.push(RenderFinding::FootnoteBackrefsBounded {
            name: name.clone(),
            references: *references,
            line: table.line(*src),
        });
    }
    let ids: HashSet<&str> = written.footnote_ids.iter().map(String::as_str).collect();
    for h in headings {
        if let Some(id) = h.id.as_ref().filter(|id| ids.contains(id.as_str())) {
            findings.push(RenderFinding::HeadingIdIsAFootnoteId {
                id: id.clone(),
                line: h.line,
            });
        }
    }
    findings
}

/// `CodeBlockNotHighlighted` for each of [`Written::not_highlighted`], in document order.
pub(crate) fn not_highlighted_findings(written: &Written, table: &LineTable) -> Vec<RenderFinding> {
    written
        .not_highlighted
        .iter()
        .map(|n| RenderFinding::CodeBlockNotHighlighted {
            line: table.line(n.offset),
            bytes: n.bytes,
            longest_line: n.longest_line,
            block_cap: n.block_cap,
            line_cap: n.line_cap,
            page_budget: n.page_budget,
        })
        .collect()
}

/// What the writer is told per heading.
pub(crate) fn heading_attrs(headings: &[Heading]) -> Vec<HeadingAttrs> {
    headings
        .iter()
        .map(|h| HeadingAttrs {
            id: h.id.clone(),
            sec_anchor: h.sec.as_ref().map(SecMarker::anchor),
        })
        .collect()
}
#[cfg(test)]
pub(crate) mod testing {
    //! Helpers shared by the adapters' tests.

    use rubrica_markdown::{events, Options, GFM};

    use super::Events;

    /// The events of `md` parsed as the compat profile parses them: without
    /// `ENABLE_TASKLISTS` (spelled out here rather than through `parse_options`, so a
    /// test fails at its own adapter).
    pub(crate) fn compat(md: &str) -> Events<'_> {
        events(md, GFM.difference(Options::ENABLE_TASKLISTS)).collect()
    }

    /// The events of `md` parsed as the native profile parses them.
    pub(crate) fn native(md: &str) -> Events<'_> {
        events(md, GFM).collect()
    }
}

#[cfg(test)]
mod tests {
    use rubrica_markdown::{events, Event, Options, GFM};

    use super::*;

    #[test]
    fn compat_parses_without_tasklists_and_native_with_gfm() {
        assert_eq!(
            parse_options(Profile::WikijsCompat),
            GFM.difference(Options::ENABLE_TASKLISTS)
        );
        assert_eq!(parse_options(Profile::Native), GFM);
    }

    #[test]
    fn check_positions_accepts_escaped_backslashes_and_indented_html() {
        // `\\` is one text backslash: byte equality alone takes the first source
        // backslash as verbatim and then has no match for `b`.
        for md in [
            "a\\\\b\n",
            "a\\\\\\*b \\\\\\\\ c\n",
            // An angle autolink's text keeps its backslashes and entities verbatim.
            "<http://a/\\\\b?x&amp;y>\n",
            // An indented HTML block's indent is a `Text` with an empty range inside it.
            " <div>\n\npara\n",
            "  <!-- x -->\n",
        ] {
            let real: Events<'_> = events(md, GFM).collect();
            assert_eq!(check_positions(&real, md), Ok(()), "{md:?}: {real:?}");
        }
    }

    /// Every CommonMark 0.31.2 and GFM 0.29 example (1,324), parsed under both
    /// profiles and run through `collect`'s adapters, passes the position check.
    #[test]
    fn check_positions_is_ok_on_every_spec_example() {
        #[derive(serde::Deserialize)]
        struct Example {
            markdown: String,
        }
        let mut examples: Vec<Example> = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/spec/commonmark-0.31.2.json"
        )))
        .unwrap();
        examples.extend(
            serde_json::from_str::<Vec<Example>>(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/spec/gfm-0.29.json"
            )))
            .unwrap(),
        );
        assert_eq!(examples.len(), 1324);
        for ex in &examples {
            let md = ex.markdown.replace("\r\n", "\n").replace('\r', "\n");
            let table = LineTable::new(&md);
            for profile in [Profile::Native, Profile::WikijsCompat] {
                let c = collect(
                    &md,
                    &table,
                    profile,
                    RawHtml::Sanitize,
                    crate::sec::check_packages_markers(&md),
                    &[],
                );
                assert_eq!(
                    check_positions(&c.events, &md),
                    Ok(()),
                    "{md:?} under {profile:?}: {:?}",
                    c.events
                );
            }
        }
    }

    #[test]
    fn check_positions_rejects_a_wrong_range() {
        let wrong = vec![(Event::Text("b".into()), 0..1)];
        assert_eq!(check_positions(&wrong, "a"), Err((0..1, "b".to_string())));
        // A merged run over an escape: `Text("a*b")` at 0..4 over `a\*b` is placed.
        let real: Events<'_> = events("a\\*b", GFM).collect();
        assert_eq!(check_positions(&real, "a\\*b"), Ok(()));
    }
}
