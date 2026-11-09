//! Why a diagram is not drawn.

/// Why a diagram is not drawn. `line` is 1-based in the diagram's source (the fence's first
/// content line is 1); 0 means the diagram as a whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported {
    /// The 1-based line in the diagram's source; 0 for the diagram as a whole.
    pub line: usize,
    /// The reason.
    pub why: Why,
}

/// The reasons a diagram is refused. The `Display` text of each is fixed: it is shown to the
/// author as the reason the diagram stays code.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Why {
    /// No diagram text at all.
    Empty,
    /// A header keyword outside the supported diagram types.
    UnknownType {
        /// The header's first word.
        keyword: String,
    },
    /// A `%%{…}%%` directive.
    Init,
    /// Front matter that is not a single `title:` line.
    FrontMatter,
    /// A `click` statement.
    Click,
    /// HTML inside a label.
    HtmlLabel,
    /// Styling statements and class shorthand.
    Styling {
        /// The statement's keyword.
        keyword: String,
    },
    /// A statement kind outside the subset.
    Statement {
        /// The statement's keyword.
        keyword: String,
    },
    /// Text that cannot be read as the grammar.
    Syntax {
        /// The unreadable text, shortened.
        found: String,
    },
    /// A block that `end` never closes.
    UnclosedBlock {
        /// The block's keyword.
        keyword: String,
    },
    /// An `end` with nothing open.
    StrayEnd,
    /// An edge that ends at a subgraph.
    EdgeToSubgraph {
        /// The subgraph's id.
        id: String,
    },
    /// One id used for a node and a subgraph.
    IdConflict {
        /// The id.
        id: String,
    },
    /// A node given two different shapes or labels.
    Redefined {
        /// The node's id.
        id: String,
    },
    /// A node placed in two subgraphs that do not nest.
    TwoSubgraphs {
        /// The node's id.
        id: String,
    },
    /// A subgraph id opened twice.
    SubgraphTwice {
        /// The subgraph's id.
        id: String,
    },
    /// A source over [`crate::MAX_SOURCE_BYTES`].
    SourceTooLarge {
        /// The source's length.
        bytes: usize,
        /// The limit.
        cap: usize,
    },
    /// More of something than the limit.
    TooMany {
        /// What there is too much of.
        what: &'static str,
        /// The limit.
        cap: usize,
    },
    /// A label over [`crate::MAX_LABEL_BYTES`].
    LabelTooLong {
        /// The label's length.
        bytes: usize,
        /// The limit.
        cap: usize,
    },
    /// A drawing wider or taller than 999,999 pixels.
    DrawingTooLarge,
}

impl std::fmt::Display for Why {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("stub")
    }
}
