//! The theme pass: what the page template adds to a body, after the
//! sanitizer (`site/DESIGN.md`, "What the build must emit"), never by
//! widening it:
//!
//! - each heading that has an id, wrapped, its `¶` anchor beside it (a
//!   sibling, so the heading's accessible name stays its text):
//!   `<div class="heading heading--2"><h2 id="x">…</h2><a class="anchor"
//!   href="#x" aria-label="Section link: …">¶</a></div>`;
//! - each table, wrapped: `<div class="table-scroll"><table>…</table></div>`.
//!
//! A heading whose id DOMPurify's rules drop (`Heading::id` `None`) has no
//! `id`, and is left as it is.
//!
//! Why matching tags on the HTML is sound: the input is
//! [`rubrica_render::Rendered::html`], ammonia's output, written by
//! html5ever's serializer. In it every start tag is `<name` then
//! attributes, each value in double quotes with `"` escaped, then `>`;
//! text has `<` escaped; there are no comments, and no element whose
//! content is raw text (`script`, `style`, `textarea`, `title`: ammonia
//! keeps none), so a `<` outside a quoted value always starts a tag. And
//! every `<h1>`–`<h6>` and `<table>` there is Rubrica's own: the
//! sanitizer's first pass drops every raw tag but `br sub sup kbd details
//! summary`, with every attribute but `open` (`raw_html_cannot_set_an_id_or_a_class`
//! and `the_raw_allowlist_is_br_sub_sup_kbd_details_summary_without_attributes`
//! in rubrica-render), so page source can neither make one nor set an id.
//! The scan is one pass, linear in the HTML.

use std::collections::HashMap;

use rubrica_render::Heading;

use super::html::escape;

/// A body after the theme pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Themed {
    /// The HTML.
    pub html: String,
    /// Headings wrapped.
    pub headings: usize,
    /// Tables wrapped.
    pub tables: usize,
}

/// One tag of the serializer's output: its name (lowercase ASCII), whether
/// it closes, and its `id` attribute's value as written (escaped).
struct Tag<'a> {
    name: &'a str,
    end: bool,
    id: Option<&'a str>,
    /// The tag's bytes, `<` to `>`.
    len: usize,
}

/// The tag at `html[at..]` (which starts with `<`), if it is one.
fn tag_at(html: &str, at: usize) -> Option<Tag<'_>> {
    let b = html.as_bytes();
    let mut i = at + 1;
    let end = b.get(i) == Some(&b'/');
    if end {
        i += 1;
    }
    let name_start = i;
    while i < b.len() && b[i].is_ascii_alphanumeric() {
        i += 1;
    }
    if i == name_start {
        return None;
    }
    let name = &html[name_start..i];
    let mut id = None;
    // Attributes: ` name` or ` name="value"`, to the `>`.
    loop {
        match b.get(i)? {
            b'>' => {
                return Some(Tag {
                    name,
                    end,
                    id,
                    len: i + 1 - at,
                })
            }
            b'"' => {
                // A value: to its closing quote (the serializer escapes `"`
                // inside it).
                let close = html[i + 1..].find('"')? + i + 1;
                let value = &html[i + 1..close];
                if html[..i].ends_with(" id=") {
                    id = Some(value);
                }
                i = close + 1;
            }
            _ => i += 1,
        }
    }
}

/// An attribute value as the serializer escapes it, read back.
fn unescape_attr(v: &str) -> String {
    v.replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", "\u{A0}")
        .replace("&amp;", "&")
}

/// `html` ([`rubrica_render::Rendered::html`]) with each heading that has an
/// id wrapped, its anchor beside it, and each table wrapped; `headings` are
/// the render's (for each anchor's label, the heading's text).
pub fn theme_body(html: &str, headings: &[Heading]) -> Themed {
    let texts: HashMap<&str, &str> = headings
        .iter()
        .filter_map(|h| Some((h.id.as_deref()?, h.text.as_str())))
        .collect();
    let mut out = String::with_capacity(html.len() + html.len() / 8);
    let (mut wrapped_headings, mut tables) = (0, 0);
    // The heading being wrapped: its tag name and its id as written.
    let mut open_heading: Option<(&str, &str, &str)> = None;
    let mut done = 0;
    let mut search = 0;
    while let Some(rel) = html[search..].find('<') {
        let at = search + rel;
        let Some(tag) = tag_at(html, at) else {
            search = at + 1;
            continue;
        };
        let is_heading = matches!(tag.name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
        let after = at + tag.len;
        match (tag.end, tag.name) {
            (false, "table") => {
                out.push_str(&html[done..at]);
                out.push_str("<div class=\"table-scroll\">");
                out.push_str(&html[at..after]);
                done = after;
            }
            (true, "table") => {
                out.push_str(&html[done..after]);
                out.push_str("</div>");
                done = after;
                tables += 1;
            }
            (false, name) if is_heading && open_heading.is_none() => {
                if let Some(raw) = tag.id {
                    let id = unescape_attr(raw);
                    if let Some(text) = texts.get(id.as_str()) {
                        out.push_str(&html[done..at]);
                        out.push_str("<div class=\"heading heading--");
                        out.push_str(&name[1..]);
                        out.push_str("\">");
                        out.push_str(&html[at..after]);
                        done = after;
                        open_heading = Some((name, raw, text));
                    }
                }
            }
            (true, name) if is_heading => {
                if let Some((open, raw, text)) = open_heading {
                    if open == name {
                        out.push_str(&html[done..after]);
                        out.push_str("<a class=\"anchor\" href=\"#");
                        // The id as the serializer escaped it: the same
                        // characters, the same escaping.
                        out.push_str(raw);
                        out.push_str("\" aria-label=\"Section link: ");
                        out.push_str(&escape(text));
                        out.push_str("\">¶</a></div>");
                        done = after;
                        open_heading = None;
                        wrapped_headings += 1;
                    }
                }
            }
            _ => {}
        }
        search = after;
    }
    out.push_str(&html[done..]);
    Themed {
        html: out,
        headings: wrapped_headings,
        tables,
    }
}
