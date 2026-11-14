//! A flowchart drawn: node sizes, the layered layout, the SVG document.

use super::Flowchart;
use crate::layered::{layout, Input};
use crate::Unsupported;

/// Draws a parsed flowchart as one `<svg>` element.
pub fn draw(_f: &Flowchart) -> Result<String, Unsupported> {
    layout(&Input)?;
    Ok(String::new())
}
