//! The part every diagram type shares: the size cap, directives, front matter and the header.

use crate::{Unsupported, Why, MAX_SOURCE_BYTES};

/// The diagram types the crate knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `graph` and `flowchart`.
    Flowchart,
    /// `sequenceDiagram`.
    Sequence,
    /// `stateDiagram-v2`.
    State,
}

/// A diagram's source split into its header and body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preamble<'a> {
    /// The diagram type.
    pub kind: Kind,
    /// The front matter's title, else the header line trimmed (`graph TD`).
    pub title: String,
    /// The header line's number and its text after the keyword (untrimmed).
    pub header_line: usize,
    /// The header line's text after the keyword.
    pub header_rest: &'a str,
    /// Every line after the header: (line number, text without its `\n`).
    pub body: Vec<(usize, &'a str)>,
}

/// Reads the preamble of `src`: the size cap, `%%{…}%%` directives, front matter and the
/// header line, in that order. The first rule that fails is the answer.
pub fn preamble(src: &str) -> Result<Preamble<'_>, Unsupported> {
    let fail = |line: usize, why: Why| Unsupported { line, why };
    if src.len() > MAX_SOURCE_BYTES {
        return Err(fail(
            0,
            Why::SourceTooLarge {
                bytes: src.len(),
                cap: MAX_SOURCE_BYTES,
            },
        ));
    }
    let mut lines: Vec<(usize, &str)> = src
        .split('\n')
        .enumerate()
        .map(|(i, l)| (i + 1, l.strip_suffix('\r').unwrap_or(l)))
        .collect();
    // The piece after a final newline (or the whole of an empty source) is not a line.
    if lines.last().is_some_and(|(_, l)| l.is_empty()) && (src.is_empty() || src.ends_with('\n')) {
        lines.pop();
    }
    if let Some(&(n, _)) = lines.iter().find(|(_, l)| l.contains("%%{")) {
        return Err(fail(n, Why::Init));
    }

    let mut at = 0;
    let mut title = None;
    if lines.first().is_some_and(|(_, l)| l.trim() == "---") {
        let Some(close) = lines
            .iter()
            .skip(1)
            .position(|(_, l)| l.trim() == "---")
            .map(|p| p + 1)
        else {
            return Err(fail(1, Why::FrontMatter));
        };
        for &(n, l) in &lines[1..close] {
            if l.trim().is_empty() {
                continue;
            }
            let rest = l.trim_start().strip_prefix("title:").map(str::trim);
            match (rest, &title) {
                (Some(t), None) if !t.is_empty() => title = Some(t.to_string()),
                _ => return Err(fail(n, Why::FrontMatter)),
            }
        }
        if title.is_none() {
            return Err(fail(1, Why::FrontMatter));
        }
        at = close + 1;
    }

    let Some(h) = lines[at..].iter().position(|(_, l)| {
        let t = l.trim_start();
        !t.is_empty() && !t.starts_with("%%")
    }) else {
        return Err(fail(0, Why::Empty));
    };
    let h = at + h;
    let (header_line, header) = lines[h];
    let header = header.trim_start();
    let word_len = header
        .find(|c: char| c.is_whitespace() || c == ';')
        .unwrap_or(header.len());
    let word = &header[..word_len];
    let kind = match word {
        "graph" | "flowchart" => Kind::Flowchart,
        "sequenceDiagram" => Kind::Sequence,
        "stateDiagram-v2" => Kind::State,
        _ => {
            return Err(fail(
                header_line,
                Why::UnknownType {
                    keyword: word.to_string(),
                },
            ))
        }
    };
    Ok(Preamble {
        kind,
        title: title.unwrap_or_else(|| header.trim().to_string()),
        header_line,
        header_rest: &header[word_len..],
        body: lines[h + 1..].to_vec(),
    })
}
