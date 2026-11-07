//! The search's rules, natively: the old page script's scoring, ranking, link and snippet
//! rules, worked out by hand for a fixed index.
//!
//! The rules: every word of the query, lowercased, must match the page somewhere; a word
//! scores 10 in the title, 4 in the first heading holding it (at most once per word) and 1
//! in the body; the first heading that matched any word is the hit's anchor; hits sort by
//! score, highest first, then by title in byte order; at most 50 are written, and the status
//! counts them all. The snippet is up to 60 characters before the first word's first match
//! in the body and 100 from it, whitespace runs collapsed to one space, with `…` before
//! (when cut) and always after.

use rubrica_wasm::search::{self, Page};

const INDEX: &str = r#"[
  {"url": "/alpha", "title": "Alpha Guide", "description": null,
   "headings": [{"id": "install", "text": "Install steps"}, {"id": "usage", "text": "Usage"}],
   "body": "Alpha is a tool. Install it with care."},
  {"url": "/beta", "title": "Beta", "description": "the second",
   "headings": [{"id": "alpha-notes", "text": "Alpha notes"}],
   "body": "Beta mentions alpha twice: alpha."},
  {"url": "/gamma", "title": "Gamma", "description": null, "headings": [],
   "body": "Nothing about the first letter here, but install appears; a shared word."},
  {"url": "/delta", "title": "Delta install", "description": null,
   "headings": [{"id": "Über straße", "text": "Install über"}],
   "body": "delta body"},
  {"url": "/epsilon", "title": "epsilon", "description": null, "headings": [],
   "body": "epsilon, a shared word"}
]"#;

fn index(json: &str) -> Vec<Page> {
    serde_json::from_str(json).expect("the index parses")
}

/// Each hit as (link text, href, score, snippet).
fn hits(pages: &[Page], query: &str) -> (Vec<(String, String, u32, String)>, String) {
    let words = search::words(search::trim(query));
    let all = search::ranked(pages, &words);
    let status = search::status_line(all.len());
    let shown = search::rank(pages, &words)
        .iter()
        .map(|h| {
            (
                h.text(),
                h.href(),
                h.total,
                search::snippet(&h.page.body, &words[0]),
            )
        })
        .collect();
    (shown, status)
}

fn hit(text: &str, href: &str, total: u32, snippet: &str) -> (String, String, u32, String) {
    (text.into(), href.into(), total, snippet.into())
}

#[test]
fn score_rank_and_snippet_match_the_old_script() {
    let pages = index(INDEX);

    // 1. "alpha": /alpha has it in the title (10) and the body (1) = 11; /beta in a heading
    // (4, the anchor) and the body (1) = 5. No other page holds it.
    assert_eq!(
        hits(&pages, "alpha"),
        (
            vec![
                hit(
                    "Alpha Guide",
                    "/alpha",
                    11,
                    "Alpha is a tool. Install it with care.…"
                ),
                hit(
                    "Beta › Alpha notes",
                    "/beta#alpha-notes",
                    5,
                    "Beta mentions alpha twice: alpha.…"
                ),
            ],
            "2 page(s) match.".into()
        )
    );

    // 2. "install": /delta title 10 + heading 4 = 14 (the anchor is percent-encoded as
    // encodeURIComponent did); /alpha heading 4 + body 1 = 5; /gamma body 1. /delta's body
    // lacks the word: an empty snippet.
    assert_eq!(
        hits(&pages, "install"),
        (
            vec![
                hit(
                    "Delta install › Install über",
                    "/delta#%C3%9Cber%20stra%C3%9Fe",
                    14,
                    ""
                ),
                hit(
                    "Alpha Guide › Install steps",
                    "/alpha#install",
                    5,
                    "Alpha is a tool. Install it with care.…"
                ),
                hit(
                    "Gamma",
                    "/gamma",
                    1,
                    "Nothing about the first letter here, but install appears; a shared word.…"
                ),
            ],
            "3 page(s) match.".into()
        )
    );

    // 3. Every word must match: only /alpha has both, 11 + 5 = 16; its anchor is the first
    // heading that matched any word ("install"; "alpha" matched none).
    assert_eq!(
        hits(&pages, "alpha install"),
        (
            vec![hit(
                "Alpha Guide › Install steps",
                "/alpha#install",
                16,
                "Alpha is a tool. Install it with care.…"
            )],
            "1 page(s) match.".into()
        )
    );

    // 4. No page.
    assert_eq!(hits(&pages, "zzz"), (vec![], "No page matches.".into()));

    // 5. A tie at 1 orders by title in byte order: "Gamma" before "epsilon" (localeCompare
    // put "epsilon" first).
    assert_eq!(
        hits(&pages, "shared"),
        (
            vec![
                hit(
                    "Gamma",
                    "/gamma",
                    1,
                    "Nothing about the first letter here, but install appears; a shared word.…"
                ),
                hit("epsilon", "/epsilon", 1, "epsilon, a shared word…"),
            ],
            "2 page(s) match.".into()
        )
    );

    // 6. Case and whitespace: the query is trimmed, lowercased and split on any JavaScript
    // whitespace (U+3000 included). /beta: alpha 4 + 1, notes 4 (one heading per word) = 9;
    // /alpha lacks "notes" and is dropped.
    assert_eq!(
        search::words(search::trim("  ALPHA\u{3000}notes ")),
        ["alpha", "notes"]
    );
    assert_eq!(
        hits(&pages, "  ALPHA\u{3000}notes "),
        (
            vec![hit(
                "Beta › Alpha notes",
                "/beta#alpha-notes",
                9,
                "Beta mentions alpha twice: alpha.…"
            )],
            "1 page(s) match.".into()
        )
    );

    // At most 50 hits are written; the status counts every match. The anchor keeps the
    // characters encodeURIComponent leaves and encodes the rest.
    let many: Vec<String> = (0..60)
        .map(|i| {
            format!(
                r#"{{"url": "/p{i:02}", "title": "Page {i:02}", "description": null,
                   "headings": [{{"id": "x-_.!~*'()y/z?&#", "text": "Hash"}}], "body": "x"}}"#
            )
        })
        .collect();
    let many = index(&format!("[{}]", many.join(",")));
    let words = search::words("hash");
    assert_eq!(search::ranked(&many, &words).len(), 60);
    let shown = search::rank(&many, &words);
    assert_eq!(shown.len(), search::LIMIT);
    assert_eq!(shown.len(), 50);
    assert_eq!(shown[0].href(), "/p00#x-_.!~*'()y%2Fz%3F%26%23");
    assert_eq!(shown[0].text(), "Page 00 › Hash");
    assert_eq!(shown[49].page.title, "Page 49");
    assert_eq!(search::status_line(60), "60 page(s) match.");
}

#[test]
fn urls_that_are_not_root_relative_are_skipped() {
    let pages = index(
        r#"[
      {"url": "/ok", "title": "Match", "description": null, "headings": [], "body": ""},
      {"url": "//evil.example/x", "title": "Match", "description": null, "headings": [], "body": ""},
      {"url": "https://evil.example/", "title": "Match", "description": null, "headings": [], "body": ""},
      {"url": "javascript:alert(1)", "title": "Match", "description": null, "headings": [], "body": ""},
      {"url": "relative", "title": "Match", "description": null, "headings": [], "body": ""},
      {"url": "", "title": "Match", "description": null, "headings": [], "body": ""}
    ]"#,
    );
    let words = search::words("match");
    let urls: Vec<&str> = search::ranked(&pages, &words)
        .iter()
        .map(|h| h.page.url.as_str())
        .collect();
    assert_eq!(urls, ["/ok"]);
    assert_eq!(search::rank(&pages, &words).len(), 1);
}

#[test]
fn snippet_slices_by_character_and_adds_ellipses() {
    // 70 two-byte characters, the match, 120 more: the cut is 60 characters before and 100
    // from the match, counted in characters, never inside one.
    let body = format!("{}MATCH{}", "é".repeat(70), "ü".repeat(120));
    assert_eq!(
        search::snippet(&body, "match"),
        format!("…{}MATCH{}…", "é".repeat(60), "ü".repeat(95))
    );
    // Near the start: no leading ellipsis; a short body is whole.
    assert_eq!(
        search::snippet("日本語 match 終", "match"),
        "日本語 match 終…"
    );
    // Whitespace runs, of any JavaScript whitespace, become one space; at the edges too.
    assert_eq!(
        search::snippet("\u{a0}a \n\t b\u{2028}\u{feff}match  ", "match"),
        " a b match …"
    );
    // A body without the word has no snippet.
    assert_eq!(search::snippet("nothing here", "match"), "");
}
