//! Diagrams in a Mermaid subset, drawn to SVG in Rust. Text is measured with a table generated
//! from the font the SVG names (`widths.rs`), so what is measured is what is drawn and nothing
//! is read from the host.

pub mod flowchart;
mod preamble;
mod why;
mod widths;

pub use preamble::{preamble, Kind, Preamble};
pub use why::{Unsupported, Why};

/// The longest diagram source drawn, in bytes.
pub const MAX_SOURCE_BYTES: usize = 32_768;
/// The longest label (node text, edge label, subgraph title), in bytes.
pub const MAX_LABEL_BYTES: usize = 256;
/// The deepest nesting of subgraphs (and, later, of composite states and sequence blocks).
pub const MAX_DEPTH: usize = 8;

pub use widths::{
    ASCENT, AVERAGE, CAP_HEIGHT, DESCENT, FONT_FAMILY, FONT_TTF_SHA256, FONT_VERSION,
    FONT_WOFF2_SHA256, UNITS_PER_EM,
};

/// The advance of `c` in units at 1,000 per em: the table's value, or [`AVERAGE`] for a code
/// point outside its ranges or not in the font.
pub fn advance(c: char) -> u16 {
    let c = c as u32;
    for (first, last, advances) in widths::RANGES {
        if (first..=last).contains(&c) {
            let v = advances
                .get((c - first) as usize)
                .copied()
                .unwrap_or(widths::MISSING);
            return if v == widths::MISSING { AVERAGE } else { v };
        }
    }
    AVERAGE
}

/// The sum of the advances of `s`'s chars, in units at 1,000 per em. No kerning, no shaping.
pub fn text_width(s: &str) -> u32 {
    s.chars()
        .fold(0u32, |sum, c| sum.saturating_add(u32::from(advance(c))))
}
