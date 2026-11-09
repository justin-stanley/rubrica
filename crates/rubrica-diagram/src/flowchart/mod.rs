//! The flowchart grammar.

mod parse;

pub use parse::{parse, Direction, Edge, EdgeKind, Flowchart, Node, Shape, Subgraph};
