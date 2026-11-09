//! The part every diagram type shares: the size cap, directives, front matter and the header.

use crate::{Unsupported, Why};

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

/// Reads the preamble of `src`.
pub fn preamble(src: &str) -> Result<Preamble<'_>, Unsupported> {
    let _ = src;
    Err(Unsupported {
        line: 0,
        why: Why::Empty,
    })
}
