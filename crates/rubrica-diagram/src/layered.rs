//! The layered layout, shared by the diagram types drawn as graphs.

use crate::{Unsupported, Why};

/// The layout's input.
pub(crate) struct Input;

/// The layout's output.
pub(crate) struct Output;

/// Lays out `input`.
pub(crate) fn layout(_input: &Input) -> Result<Output, Unsupported> {
    Err(Unsupported {
        line: 0,
        why: Why::Empty,
    })
}
