//! A page's table of contents: every heading, nested by level.
//!
//! Wiki.js's own TOC drops a heading whose parent level is absent (an `h3`
//! straight under an `h1`); Rubrica's keeps it, under the nearest heading of
//! a lower level. A heading whose id DOMPurify drops has no link.

use rubrica_render::Heading;

/// One entry of a page's table of contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    /// The heading's level, 1 to 6.
    pub level: u8,
    /// The heading's text.
    pub title: String,
    /// The heading's id, the link's fragment; `None` for a heading rendered
    /// with no id (shown as text).
    pub id: Option<String>,
    /// The heading's line in the file.
    pub line: usize,
    /// The headings under it.
    pub children: Vec<TocEntry>,
}

impl TocEntry {
    /// `#<id>`, or `None` for a heading with no id.
    pub fn href(&self) -> Option<String> {
        self.id.as_ref().map(|id| format!("#{id}"))
    }
}

/// The table of contents of `headings` (document order; lines in the file
/// are `heading.line + line_offset`). Each heading goes under the nearest
/// earlier heading of a lower level, or at the top when there is none.
/// Linear: one stack of open entries, at most six deep.
pub fn toc(headings: &[Heading], line_offset: usize) -> Vec<TocEntry> {
    let mut roots: Vec<TocEntry> = Vec::new();
    // Open entries, outermost first; each is finished into its parent (or
    // the roots) when a heading at its level or above arrives.
    let mut open: Vec<TocEntry> = Vec::new();
    fn close(open: &mut Vec<TocEntry>, roots: &mut Vec<TocEntry>) {
        let done = open.pop().expect("an open entry");
        match open.last_mut() {
            Some(parent) => parent.children.push(done),
            None => roots.push(done),
        }
    }
    for h in headings {
        while open.last().is_some_and(|e| e.level >= h.level) {
            close(&mut open, &mut roots);
        }
        open.push(TocEntry {
            level: h.level,
            title: h.text.clone(),
            id: h.id.clone(),
            line: h.line + line_offset,
            children: Vec::new(),
        });
    }
    while !open.is_empty() {
        close(&mut open, &mut roots);
    }
    roots
}

/// The entries in document order (a pre-order walk).
pub fn flatten(entries: &[TocEntry]) -> Vec<&TocEntry> {
    let mut out = Vec::new();
    let mut stack: Vec<&TocEntry> = entries.iter().rev().collect();
    while let Some(e) = stack.pop() {
        out.push(e);
        stack.extend(e.children.iter().rev());
    }
    out
}
