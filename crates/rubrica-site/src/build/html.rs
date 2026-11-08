//! Escaping for every context the build writes untrusted text into: HTML
//! text and attribute values (what askama's HTML escaper does, for the
//! markup the build writes itself), URL paths, JSON that could end up in a
//! `<script>`, and XML.

/// `s` for HTML text or a double- or single-quoted attribute value: `&`,
/// `<`, `>`, `"` and `'` as character references. Every other character is
/// itself (a control character in text is a parse error a browser shows as
/// itself or U+FFFD, never markup).
pub(crate) fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// A page URL (`/a/b c`) as an href: [`crate::links::written_href`]'s
/// percent-encoding (as the writer writes a link's href: every byte but
/// unreserved ASCII and a few delimiters as `%XX`), so a browser reads back
/// exactly the path. A page path has no `?`, `#` or `%`, so nothing in it
/// is a delimiter.
pub(crate) fn url(path: &str) -> String {
    crate::links::written_href(path)
}

/// JSON text with `<`, `>`, `&`, U+2028 and U+2029 written as `\u` escapes:
/// the same value, and safe inside a `<script>` element (no `</script>`,
/// no `<!--`) and as JavaScript source. Those characters occur in JSON text
/// only inside strings, where the escape means the character itself. The
/// build streams it ([`ScriptSafe`]); this is the reference its tests hold
/// it to.
#[cfg(test)]
pub(crate) fn script_safe_json(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        match c {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out
}

/// A writer of JSON text that writes it into `out` as [`script_safe_json`]
/// would (the index is streamed, not built and then copied).
/// A write may end inside U+2028 or U+2029 (`E2 80 A8`, `E2 80 A9`), so
/// the start of one (`E2`, `E2 80`) is held until the next write shows
/// what it is, or [`std::io::Write::flush`].
pub(crate) struct ScriptSafe<W: std::io::Write> {
    /// Where the escaped text goes.
    pub out: W,
    held: Vec<u8>,
    scratch: Vec<u8>,
}

impl<W: std::io::Write> ScriptSafe<W> {
    /// A writer into `out`.
    pub fn new(out: W) -> Self {
        Self {
            out,
            held: Vec::new(),
            scratch: Vec::new(),
        }
    }
}

impl<W: std::io::Write> std::io::Write for ScriptSafe<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let out = &mut self.scratch;
        out.clear();
        for &b in buf {
            match (self.held.as_slice(), b) {
                ([0xE2], 0x80) => {
                    self.held.push(b);
                    continue;
                }
                ([0xE2, 0x80], 0xA8) => {
                    self.held.clear();
                    out.extend_from_slice(b"\\u2028");
                    continue;
                }
                ([0xE2, 0x80], 0xA9) => {
                    self.held.clear();
                    out.extend_from_slice(b"\\u2029");
                    continue;
                }
                // Not one of the two after all: as it is, then `b` afresh.
                ([_, ..], _) => {
                    out.append(&mut self.held);
                }
                ([], _) => {}
            }
            match b {
                b'<' => out.extend_from_slice(b"\\u003c"),
                b'>' => out.extend_from_slice(b"\\u003e"),
                b'&' => out.extend_from_slice(b"\\u0026"),
                0xE2 => self.held.push(b),
                _ => out.push(b),
            }
        }
        self.out.write_all(out)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.out.write_all(&self.held)?;
        self.held.clear();
        self.out.flush()
    }
}

/// `s` for XML 1.0 text or a double-quoted attribute: [`escape`]'s five,
/// and every character XML 1.0 refuses (`Char`: C0 controls but tab, line
/// feed and carriage return; U+FFFE and U+FFFF) as U+FFFD, since XML has no
/// way to write them.
pub(crate) fn xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => out.push('\u{FFFD}'),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_writes_the_five_and_nothing_else() {
        for c in (0..=0x10FFFFu32).filter_map(char::from_u32) {
            let e = escape(&c.to_string());
            match c {
                '&' | '<' | '>' | '"' | '\'' => assert!(e.starts_with('&') && e.ends_with(';')),
                _ => assert_eq!(e, c.to_string()),
            }
        }
        assert_eq!(
            escape("<a href=\"x\">'&'</a>"),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;"
        );
    }

    #[test]
    fn script_safe_json_reads_back_the_same_value() {
        let s = "</script><!-- & \u{2028}\u{2029} \"q\"";
        let json = serde_json::to_string(&s).unwrap();
        let safe = script_safe_json(&json);
        for bad in ['<', '>', '&', '\u{2028}', '\u{2029}'] {
            assert!(!safe.contains(bad));
        }
        let back: String = serde_json::from_str(&safe).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn script_safe_writes_what_script_safe_json_does_however_it_is_split() {
        use std::io::Write as _;
        let s = "a</script><!-- & \u{2028}\u{2029} \"q\" é \u{2027}\u{202A} \u{1F600} >";
        let json = serde_json::to_string(&s).unwrap();
        let want = script_safe_json(&json);
        // serde_json's own writes.
        let mut w = ScriptSafe::new(Vec::new());
        serde_json::to_writer(&mut w, &s).unwrap();
        w.flush().unwrap();
        assert_eq!(String::from_utf8(w.out).unwrap(), want);
        // Every split of the text into two writes, and one byte a write.
        let bytes = json.as_bytes();
        for at in 0..=bytes.len() {
            let mut w = ScriptSafe::new(Vec::new());
            w.write_all(&bytes[..at]).unwrap();
            w.write_all(&bytes[at..]).unwrap();
            w.flush().unwrap();
            assert_eq!(String::from_utf8(w.out).unwrap(), want, "split at {at}");
        }
        let mut w = ScriptSafe::new(Vec::new());
        for b in bytes {
            w.write_all(std::slice::from_ref(b)).unwrap();
        }
        w.flush().unwrap();
        assert_eq!(String::from_utf8(w.out).unwrap(), want);
    }

    #[test]
    fn xml_holds_only_characters_xml_allows() {
        for c in (0..=0x10FFFFu32).filter_map(char::from_u32) {
            let e = xml(&c.to_string());
            for d in e.chars() {
                let n = d as u32;
                assert!(
                    matches!(n, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF),
                    "{c:?} gave {d:?}"
                );
            }
            assert!(!e.contains('<') && !e.contains('>') && !e.contains('"'));
        }
    }

    #[test]
    fn a_url_is_percent_encoded_as_the_writer_writes_an_href() {
        assert_eq!(url("/a b/é"), "/a%20b/%C3%A9");
        assert_eq!(url("/"), "/");
    }
}
