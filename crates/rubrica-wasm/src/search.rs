//! The search itself, with no browser in it: the query's words, each page's score, the
//! ranking and the snippet. The page's script (`lib.rs`) only reads the query, fetches the
//! index and writes what these functions return.

use serde::Deserialize;

/// The most hits written into the page.
pub const LIMIT: usize = 50;

/// An entry of `_search.json`.
#[derive(Debug, Deserialize)]
pub struct Page {
    pub url: String,
    pub title: String,
    pub description: Option<String>,
    pub headings: Vec<Heading>,
    pub body: String,
}

/// A heading of a page, with its anchor.
#[derive(Debug, Deserialize)]
pub struct Heading {
    pub id: String,
    pub text: String,
}

/// A page that matched: its score, and the first heading that matched a word.
#[derive(Debug)]
pub struct Hit<'a> {
    pub page: &'a Page,
    pub total: u32,
    pub best: Option<&'a Heading>,
}

impl Hit<'_> {
    /// The link's text: the title, then the anchor heading's text after ` › `.
    pub fn text(&self) -> String {
        match self.best {
            None => self.page.title.clone(),
            Some(h) => format!("{} › {}", self.page.title, h.text),
        }
    }

    /// The link's target: the page's URL, then `#` and the anchor heading's id, encoded as
    /// `encodeURIComponent` encodes it.
    pub fn href(&self) -> String {
        match self.best {
            None => self.page.url.clone(),
            Some(h) => format!("{}#{}", self.page.url, encode_uri_component(&h.id)),
        }
    }
}

/// JavaScript's whitespace (`\s`, and what `trim` removes): WhiteSpace and LineTerminator.
/// Not `char::is_whitespace`, which takes U+0085 and leaves U+FEFF.
fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// `encodeURIComponent`: every byte of the UTF-8 percent-encoded but ASCII letters, digits
/// and `-_.!~*'()`.
fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The query, without the whitespace around it.
pub fn trim(query: &str) -> &str {
    query.trim_matches(is_js_space)
}

/// Every word of the query, lowercased.
pub fn words(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split(is_js_space)
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A page's score: a word in the title 10, in a heading 4 (the first heading holding it,
/// once), in the body 1; `None` unless every word is somewhere. The first heading that
/// matched any word is the hit's anchor.
pub fn score<'a>(page: &'a Page, words: &[String]) -> Option<Hit<'a>> {
    let title = page.title.to_lowercase();
    let body = page.body.to_lowercase();
    let headings: Vec<(String, &Heading)> = page
        .headings
        .iter()
        .map(|h| (h.text.to_lowercase(), h))
        .collect();
    let mut total = 0;
    let mut best = None;
    for w in words {
        let mut s = 0;
        if title.contains(w.as_str()) {
            s += 10;
        }
        if let Some((_, h)) = headings.iter().find(|(text, _)| text.contains(w.as_str())) {
            s += 4;
            best = best.or(Some(*h));
        }
        if body.contains(w.as_str()) {
            s += 1;
        }
        if s == 0 {
            return None;
        }
        total += s;
    }
    Some(Hit { page, total, best })
}

/// Every hit, best first: only pages whose URL is root-relative (`/`, not `//`), by score,
/// then by title in byte order (the old script's `localeCompare` ordered by locale).
pub fn ranked<'a>(pages: &'a [Page], words: &[String]) -> Vec<Hit<'a>> {
    let mut hits: Vec<Hit<'a>> = pages
        .iter()
        .filter(|p| p.url.starts_with('/') && !p.url.starts_with("//"))
        .filter_map(|p| score(p, words))
        .collect();
    // Stable, as Array.prototype.sort is: equal pages keep the index's order.
    hits.sort_by(|a, b| {
        b.total
            .cmp(&a.total)
            .then_with(|| a.page.title.cmp(&b.page.title))
    });
    hits
}

/// The hits written into the page: the first [`LIMIT`] of [`ranked`].
pub fn rank<'a>(pages: &'a [Page], words: &[String]) -> Vec<Hit<'a>> {
    let mut hits = ranked(pages, words);
    hits.truncate(LIMIT);
    hits
}

/// The words around the first match of `first_word` in `body`: up to 60 characters before
/// it and 100 from it, every whitespace run one space, `…` before when cut and always after;
/// empty when the body lacks the word. Counted in characters (the old script counted UTF-16
/// code units), so a cut never splits one.
pub fn snippet(body: &str, first_word: &str) -> String {
    let lower = body.to_lowercase();
    let Some(byte) = lower.find(first_word) else {
        return String::new();
    };
    let at = lower[..byte].chars().count();
    let chars: Vec<char> = body.chars().collect();
    let from = at.saturating_sub(60).min(chars.len());
    let to = (at + 100).clamp(from, chars.len());
    let mut out = String::new();
    if from > 0 {
        out.push('…');
    }
    let mut space = false;
    for &c in &chars[from..to] {
        if is_js_space(c) {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out.push('…');
    out
}

/// The status line for `n` matching pages.
pub fn status_line(n: usize) -> String {
    if n == 0 {
        "No page matches.".to_owned()
    } else {
        format!("{n} page(s) match.")
    }
}
