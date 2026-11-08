//! Native heading ids: NFKC, lowercase, alphanumerics and `-`, deduplicated
//! `-1`, `-2`, current Unicode. The compat profile keeps the uslug port in
//! [`crate::slug`].

use std::collections::{HashMap, HashSet};

use unicode_normalization::UnicodeNormalization;

use crate::slug::DOM_CLOBBER;

/// The slug of a heading's text: NFKC-normalised; lowercased (`char::to_lowercase`);
/// every maximal run of characters that are not `char::is_alphanumeric` (spaces, `_`,
/// `'`, `.`, `!`, `-` itself, variation selectors, …) becomes one `-`; a leading or
/// trailing `-` is trimmed; empty (only punctuation or symbols, `!!!`) is `section`.
/// Letters outside ASCII stay as they are (`Café` → `café`), as GitHub's ids keep them.
pub(crate) fn native_id(text: &str) -> String {
    let mut out = String::new();
    for c in text.nfkc().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    if out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("section");
    }
    out
}

/// Per-page dedupe: the second `x` is `x-1`, the third `x-2` (the smallest free suffix,
/// as [`crate::slug::Slugs`] probes). An id that would clobber a DOM global (`location`,
/// `cookie`, …: [`crate::slug::DOM_CLOBBER`]) is counted but not given: the heading has
/// no `id`, exactly as the compat path does, safety before GitHub parity.
#[derive(Debug, Default)]
pub(crate) struct NativeSlugs {
    seen: HashSet<String>,
    /// Per base slug, the suffix to probe first: every suffix below it was taken when it
    /// was last probed, and `seen` only grows (linear over many duplicates).
    next_suffix: HashMap<String, u32>,
}

impl NativeSlugs {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The id of the next heading of this page, `None` where it would clobber the DOM.
    pub(crate) fn heading_id(&mut self, text: &str) -> Option<String> {
        let mut slug = native_id(text);
        if self.seen.contains(&slug) {
            let next = self.next_suffix.entry(slug.clone()).or_insert(1);
            while self.seen.contains(&format!("{slug}-{next}")) {
                *next += 1;
            }
            slug = format!("{slug}-{next}");
            *next += 1;
        }
        self.seen.insert(slug.clone());
        DOM_CLOBBER
            .binary_search(&slug.as_str())
            .is_err()
            .then_some(slug)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// One page's ids, in order.
    fn ids(texts: &[&str]) -> Vec<Option<String>> {
        let mut slugs = NativeSlugs::new();
        texts.iter().map(|t| slugs.heading_id(t)).collect()
    }

    #[test]
    fn native_ids_are_nfkc_lowercase_alnum_dash_deduped() {
        assert_eq!(native_id("Ünïcode  Test!"), "ünïcode-test");
        assert_eq!(
            ids(&["Ünïcode  Test!", "Ünïcode  Test!"]),
            vec![Some("ünïcode-test".into()), Some("ünïcode-test-1".into())]
        );
        assert_eq!(native_id("\u{fb01}"), "fi");
        assert_eq!(native_id("Café Déjà"), "café-déjà");
        assert_eq!(native_id("!!!"), "section");
        assert_eq!(
            ids(&["!!!", "???"]),
            vec![Some("section".into()), Some("section-1".into())]
        );
        // The separators (Design rule 5).
        assert_eq!(native_id("Foo_bar"), "foo-bar");
        assert_eq!(native_id("a -- b"), "a-b");
        assert_eq!(native_id("It's v1.2"), "it-s-v1-2");
        assert_eq!(native_id("  spaced  "), "spaced");
        assert_eq!(native_id("日本語の見出し"), "日本語の見出し");
        assert_eq!(
            ids(&["x", "x-1", "x", "x"]),
            vec![
                Some("x".into()),
                Some("x-1".into()),
                Some("x-2".into()),
                Some("x-3".into())
            ]
        );
    }

    #[test]
    fn a_native_id_that_clobbers_the_dom_is_not_given() {
        assert_eq!(native_id("Location"), "location");
        // Counted anyway: the next `Location` is `location-1`, which is safe.
        assert_eq!(
            ids(&["Location", "Location", "cookie"]),
            vec![None, Some("location-1".into()), None]
        );
    }
}
