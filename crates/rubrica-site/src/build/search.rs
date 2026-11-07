//! The search index, `_search.json`: `[{url, title,
//! description, headings: [{id, text}], body}]`, one entry per published
//! page that is no redirect stub, in path order; `body` is the page's plain
//! text. Written through [`ScriptSafe`] ([`super::html::script_safe_json`]'s
//! escapes, streamed, so the index is not held twice), so no
//! string in it can close a `<script>` or open a comment wherever it is
//! read.

use serde::Serialize;

use std::io::Write as _;

use super::html::ScriptSafe;
use crate::Site;

#[derive(Serialize)]
struct Entry<'a> {
    url: &'a str,
    title: &'a str,
    description: Option<&'a str>,
    headings: Vec<Heading<'a>>,
    body: &'a str,
}

#[derive(Serialize)]
struct Heading<'a> {
    id: &'a str,
    text: &'a str,
}

/// The index's bytes.
pub(crate) fn index(site: &Site) -> Vec<u8> {
    let entries: Vec<Entry> = site
        .pages()
        .values()
        .filter(|p| p.front.redirect_to.is_none())
        .map(|p| Entry {
            url: &p.url,
            title: &p.front.title,
            description: p.front.description.as_deref(),
            headings: p
                .rendered
                .headings
                .iter()
                .filter_map(|h| {
                    Some(Heading {
                        id: h.id.as_deref()?,
                        text: &h.text,
                    })
                })
                .collect(),
            body: &p.rendered.plain_text,
        })
        .collect();
    let mut w = ScriptSafe::new(Vec::new());
    serde_json::to_writer(&mut w, &entries).expect("the index serializes");
    w.flush().expect("a Vec takes every write");
    w.out
}
