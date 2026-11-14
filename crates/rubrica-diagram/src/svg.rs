//! Writing SVG: elements with explicit end tags, attributes in the order given, numbers with
//! two decimals, text escaped as html5ever's serializer writes it.
//!
//! Every attribute value is a number, a class list or a literal the drawing code chooses, so
//! values are written as given; only text (labels, titles, the source) comes from the author,
//! and it is escaped.

use crate::geom::Pt;

/// `n` centipixels with exactly two decimals: `1234` is `12.34`. `n` is not negative.
pub(crate) fn num(n: i64) -> String {
    format!("{}.{:02}", n.div_euclid(100), n.rem_euclid(100))
}

/// A path through `points`: `M x y`, then ` L x y` for each later point.
pub(crate) fn line(points: &[Pt]) -> String {
    let mut d = String::new();
    for (i, p) in points.iter().enumerate() {
        d.push_str(if i == 0 { "M" } else { " L" });
        d.push_str(&num(p.x));
        d.push(' ');
        d.push_str(&num(p.y));
    }
    d
}

/// A closed path through `points`: the line, then ` Z`.
pub(crate) fn closed(points: &[Pt]) -> String {
    line(points) + " Z"
}

/// An SVG document being written.
#[derive(Default)]
pub(crate) struct Writer {
    out: String,
}

impl Writer {
    /// A start tag with its attributes, in order.
    pub fn open(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.out.push('<');
        self.out.push_str(name);
        for (k, v) in attrs {
            self.out.push(' ');
            self.out.push_str(k);
            self.out.push_str("=\"");
            self.out.push_str(v);
            self.out.push('"');
        }
        self.out.push('>');
    }

    /// An end tag.
    pub fn close(&mut self, name: &str) {
        self.out.push_str("</");
        self.out.push_str(name);
        self.out.push('>');
    }

    /// An element with its attributes and no content.
    pub fn empty(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.open(name, attrs);
        self.close(name);
    }

    /// An element holding escaped text.
    pub fn text(&mut self, name: &str, attrs: &[(&str, &str)], text: &str) {
        self.open(name, attrs);
        escape_text(&mut self.out, text);
        self.close(name);
    }

    /// The document.
    pub fn finish(self) -> String {
        self.out
    }
}

/// Appends `s` escaped as text: `&`, `<`, `>` and U+00A0 as entities.
fn escape_text(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            _ => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_have_two_decimals() {
        assert_eq!(num(0), "0.00");
        assert_eq!(num(5), "0.05");
        assert_eq!(num(1234), "12.34");
        assert_eq!(num(99_999_999), "999999.99");
    }

    #[test]
    fn text_is_escaped_as_the_serializer_writes_it() {
        let mut w = Writer::default();
        w.text(
            "text",
            &[("class", "rd-text")],
            "a & b < c > d\u{a0}e \"f\" 'g'",
        );
        assert_eq!(
            w.finish(),
            "<text class=\"rd-text\">a &amp; b &lt; c &gt; d&nbsp;e \"f\" 'g'</text>"
        );
    }

    #[test]
    fn paths_are_written_in_the_grammar() {
        let p = [Pt { x: 1600, y: 2000 }, Pt { x: 3000, y: 4000 }];
        assert_eq!(line(&p), "M16.00 20.00 L30.00 40.00");
        assert_eq!(closed(&p), "M16.00 20.00 L30.00 40.00 Z");
    }
}
