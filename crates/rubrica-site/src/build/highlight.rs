//! Build-time syntax highlighting of fenced code, with syntect.
//!
//! [`syntect_html`] is the [`rubrica_render::Highlighter`] every site renders with
//! ([`crate::render_options`]). It writes classes, never inline styles, and only the ten
//! the theme colours ([`HL_CLASSES`]); the sanitizer's allow-list on `span` is the second
//! guard. A line over [`MAX_HIGHLIGHT_LINE_BYTES`] or a block over
//! [`MAX_HIGHLIGHT_BLOCK_BYTES`] never reaches the regex engine: it is written plain and
//! reported (`code-block-not-highlighted`).

use std::sync::LazyLock;

use rubrica_render::sanitize::HL_CLASSES;
use rubrica_render::Highlight;
use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// The longest line, in bytes, that is highlighted.
pub const MAX_HIGHLIGHT_LINE_BYTES: usize = 2_000;
/// The longest block, in bytes, that is highlighted.
pub const MAX_HIGHLIGHT_BLOCK_BYTES: usize = 64 << 10;

/// Loaded once per process.
static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);

/// syntect over `code` when `lang` names a syntax it knows and the block is within the
/// caps; [`Highlight::Plain`] for an unknown language (or if syntect fails: deterministic,
/// and not a cap); [`Highlight::OverCap`] when a line or the block is over its cap. The caps
/// are checked first, before the language lookup.
pub fn syntect_html(lang: &str, code: &str) -> Highlight {
    let bytes = code.trim_end_matches('\n').len();
    let longest_line = code.lines().map(str::len).max().unwrap_or(0);
    if longest_line > MAX_HIGHLIGHT_LINE_BYTES || bytes > MAX_HIGHLIGHT_BLOCK_BYTES {
        return Highlight::OverCap {
            bytes,
            longest_line,
            block_cap: MAX_HIGHLIGHT_BLOCK_BYTES,
            line_cap: MAX_HIGHLIGHT_LINE_BYTES,
        };
    }
    let Some(syntax) = SYNTAXES.find_syntax_by_token(lang) else {
        return Highlight::Plain;
    };
    let mut generator = ClassedHTMLGenerator::new_with_class_style(
        syntax,
        &SYNTAXES,
        ClassStyle::SpacedPrefixed { prefix: "hl-" },
    );
    for line in LinesWithEndings::from(code) {
        if generator
            .parse_html_for_line_which_includes_newline(line)
            .is_err()
        {
            return Highlight::Plain;
        }
    }
    match keep_styled_spans(&generator.finalize()) {
        Some(html) => Highlight::Html(html),
        None => Highlight::Plain,
    }
}

/// syntect's classed output has exactly three forms: `<span class="…">`, `</span>`, and
/// text in which `<` is always `&lt;`. Scans on `<`: at a span open, keeps the classes that
/// are in [`HL_CLASSES`] and writes the span when any remain, else nothing; at `</span>`
/// writes it only if its open was written. Any other `<` form, a `</span>` with nothing
/// open, or a span left open gives `None` (the caller writes the block plain: fail-safe).
fn keep_styled_spans(html: &str) -> Option<String> {
    let mut out = String::with_capacity(html.len());
    let mut written: Vec<bool> = Vec::new();
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        rest = &rest[lt..];
        if let Some(open) = rest.strip_prefix("<span class=\"") {
            let end = open.find("\">")?;
            let mut kept = String::new();
            for class in open[..end].split(' ') {
                if HL_CLASSES.contains(&class) {
                    if !kept.is_empty() {
                        kept.push(' ');
                    }
                    kept.push_str(class);
                }
            }
            if kept.is_empty() {
                written.push(false);
            } else {
                out.push_str("<span class=\"");
                out.push_str(&kept);
                out.push_str("\">");
                written.push(true);
            }
            rest = &open[end + 2..];
        } else {
            let after = rest.strip_prefix("</span>")?;
            if written.pop()? {
                out.push_str("</span>");
            }
            rest = after;
        }
    }
    out.push_str(rest);
    written.is_empty().then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unstyled_spans_are_dropped_with_their_close() {
        let html = "<span class=\"hl-source hl-rust\"><span class=\"hl-storage hl-type hl-function hl-rust\">fn</span> x</span>";
        assert_eq!(
            keep_styled_spans(html).as_deref(),
            Some("<span class=\"hl-storage\">fn</span> x")
        );
    }

    #[test]
    fn an_unexpected_form_is_refused() {
        assert_eq!(keep_styled_spans("<b>x</b>"), None);
        assert_eq!(keep_styled_spans("x</span>"), None);
        assert_eq!(keep_styled_spans("<span class=\"hl-meta\">x"), None);
        assert_eq!(keep_styled_spans("<span style=\"x\">y</span>"), None);
    }
}
