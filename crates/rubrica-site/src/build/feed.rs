//! The changelog's Atom 1.0 feed (`site/DESIGN.md`, "The changelog feed"):
//! one entry per `<h2>` of the source page, in the page's order (newest
//! first, as a changelog is written). An entry's `id` and `link` are
//! `<base_url><page url>#<heading id>`, its `title` `<name> <heading text>`,
//! its `content` the section's HTML (the render's, up to the next `<h2>`),
//! and its `updated` the date of the earliest commit, by committer time
//! (ties by id), among the versions of the file that have that heading, so
//! no release date is typed (not the last in the walk's order, which
//! is not time's when a branch's clock is behind). The feed's own `updated`
//! is its newest entry's. Every string is XML-escaped ([`xml`]), and a
//! character XML cannot hold is U+FFFD.

use std::collections::HashMap;

use rubrica_render::{render_page, RenderOptions};

use super::html::{url, xml};
use super::{BuildConfig, BuildError, Commit, FeedConfig, History};
use crate::Page;

/// The `<h2>`s of a render: (id as written, start of the tag, end of
/// `</h2>`), in order.
fn h2s(html: &str) -> Vec<(Option<String>, usize, usize)> {
    let mut out = Vec::new();
    let mut search = 0;
    while let Some(rel) = html[search..].find("<h2") {
        let at = search + rel;
        let next = html.as_bytes().get(at + 3);
        if !matches!(next, Some(b'>' | b' ')) {
            search = at + 3;
            continue;
        }
        let open_end = html[at..].find('>').map_or(html.len(), |e| at + e + 1);
        let id = html[at..open_end].find(" id=\"").map(|i| {
            let v = &html[at + i + 5..open_end];
            v[..v.find('"').unwrap_or(v.len())].to_owned()
        });
        let close = html[open_end..]
            .find("</h2>")
            .map_or(html.len(), |e| open_end + e + 5);
        out.push((id, at, close));
        search = close;
    }
    out
}

/// The ids of the `##` headings of a version of the page.
fn version_ids(bytes: &[u8], opts: &RenderOptions) -> Vec<String> {
    let Ok(src) = std::str::from_utf8(bytes) else {
        return Vec::new();
    };
    match render_page(src, opts) {
        Ok((_, r, _)) => r
            .headings
            .into_iter()
            .filter(|h| h.level == 2)
            .filter_map(|h| h.id)
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// The feed's XML.
pub(crate) fn feed(
    page: &Page,
    cfg: &BuildConfig,
    feed: &FeedConfig,
    history: &History,
    opts: &RenderOptions,
) -> Result<String, BuildError> {
    let base = cfg
        .base_url
        .as_deref()
        .ok_or_else(|| BuildError::Config("a feed needs base_url".into()))?;
    // The earliest committer time among the versions with an id dates it.
    let mut dated: HashMap<String, &Commit> = HashMap::new();
    for (commit, bytes) in &history.feed_versions {
        for id in version_ids(bytes, opts) {
            let d = dated.entry(id).or_insert(commit);
            if (commit.time, &commit.id) < (d.time, &d.id) {
                *d = commit;
            }
        }
    }
    let page_url = format!("{base}{}", url(&page.url));
    let h2_texts: HashMap<&str, &str> = page
        .rendered
        .headings
        .iter()
        .filter(|h| h.level == 2)
        .filter_map(|h| Some((h.id.as_deref()?, h.text.as_str())))
        .collect();
    let html = &page.rendered.html;
    let sections = h2s(html);
    let mut entries = String::new();
    let mut newest: Option<&Commit> = None;
    for (i, (raw_id, _, end)) in sections.iter().enumerate() {
        // A heading with no id (DOMPurify's list) can be no entry: nothing
        // could link to it.
        let Some(raw_id) = raw_id else { continue };
        let id = raw_id
            .replace("&quot;", "\"")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&nbsp;", "\u{A0}")
            .replace("&amp;", "&");
        let Some(text) = h2_texts.get(id.as_str()) else {
            continue;
        };
        let content_end = sections.get(i + 1).map_or(html.len(), |s| s.1);
        let content = html[*end..content_end].trim();
        let commit = dated.get(&id).copied().unwrap_or(&history.commit);
        if newest.is_none_or(|n| commit.time > n.time) {
            newest = Some(commit);
        }
        let link = format!("{page_url}#{id}");
        entries.push_str(&format!(
            "  <entry>\n    <id>{l}</id>\n    <title>{t}</title>\n    <link rel=\"alternate\" href=\"{l}\"/>\n    <updated>{u}</updated>\n    <content type=\"html\">{c}</content>\n  </entry>\n",
            l = xml(&link),
            t = xml(&format!("{} {text}", cfg.name)),
            u = xml(&commit.iso()),
            c = xml(content),
        ));
    }
    let updated = newest.unwrap_or(&history.commit).iso();
    let self_url = format!("{base}{}", feed.url);
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<feed xmlns=\"http://www.w3.org/2005/Atom\">\n  <id>{s}</id>\n  <title>{t}</title>\n  <link rel=\"self\" href=\"{s}\"/>\n  <link rel=\"alternate\" href=\"{p}\"/>\n  <updated>{u}</updated>\n  <author><name>{n}</name></author>\n{entries}</feed>\n",
        s = xml(&self_url),
        t = xml(&format!("{} releases", cfg.name)),
        p = xml(&page_url),
        u = xml(&updated),
        n = xml(&cfg.name),
    ))
}
