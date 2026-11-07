//! Headings: their rendered text, and the ids Wiki.js 2 gives them.

use crate::sec::SecMarker;

/// One heading of a page, in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// 1 to 6.
    pub level: u8,
    /// The heading's rendered text, what jQuery's `.text()` sees after
    /// markdown-it: the input to the slugger.
    pub text: String,
    /// The id, or `None` where DOMPurify would delete it (the heading is
    /// rendered with no `id` and cannot be linked to).
    pub id: Option<String>,
    /// 1-based source line.
    pub line: usize,
    /// The `@sec` marker naming this heading's section, whose anchor
    /// (`<span class="sec" id="sec-<id>">`) opens the heading. Not part of
    /// [`Heading::text`] or the id.
    pub sec: Option<SecMarker>,
}

/// `s` with `&`, `"`, `<` and `>` escaped, for an attribute value.
pub(crate) fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}
