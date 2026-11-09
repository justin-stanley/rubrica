//! The flowchart reader.

use crate::{Unsupported, Why};

/// A flowchart read from its source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flowchart {
    /// The flow direction.
    pub direction: Direction,
    /// The front matter's title, else the header line.
    pub title: String,
    /// The source with trailing newlines trimmed.
    pub source: String,
    /// Nodes in first-mention order.
    pub nodes: Vec<Node>,
    /// Edges in reading order.
    pub edges: Vec<Edge>,
    /// Subgraphs in opening order.
    pub subgraphs: Vec<Subgraph>,
}

/// The flow direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// `TD` and `TB`.
    TopDown,
    /// `BT`.
    BottomUp,
    /// `LR`.
    LeftRight,
    /// `RL`.
    RightLeft,
}

/// A node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// The id.
    pub id: String,
    /// The label; the id when no mention gives one.
    pub label: String,
    /// The shape; `Rect` when no mention gives one.
    pub shape: Shape,
    /// The innermost subgraph the node belongs to.
    pub subgraph: Option<usize>,
    /// The line of the first mention.
    pub line: usize,
}

/// A node's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `[text]`.
    Rect,
    /// `(text)`.
    Round,
    /// `{text}`.
    Rhombus,
    /// `([text])`.
    Stadium,
    /// `[[text]]`.
    Subroutine,
}

/// An edge between two nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    /// The index of the node it starts at.
    pub from: usize,
    /// The index of the node it ends at.
    pub to: usize,
    /// The line style.
    pub kind: EdgeKind,
    /// The label.
    pub label: Option<String>,
    /// The line.
    pub line: usize,
}

/// An edge's line style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// `-->`.
    Arrow,
    /// `---`.
    Open,
    /// `-.->`.
    Dotted,
    /// `==>`.
    Thick,
}

/// A subgraph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subgraph {
    /// The id.
    pub id: String,
    /// The title.
    pub title: String,
    /// The enclosing subgraph.
    pub parent: Option<usize>,
    /// The line it opens on.
    pub line: usize,
}

/// Reads a flowchart from `src`.
pub fn parse(src: &str) -> Result<Flowchart, Unsupported> {
    let _ = src;
    Err(Unsupported {
        line: 0,
        why: Why::Empty,
    })
}
