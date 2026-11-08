//! The compatibility core of Rubrica: everything that must render exactly as
//! Wiki.js 2 did (heading ids, anchors, links, `@sec` sections), sanitized
//! ([`sanitize`]), and the skeleton reducer the golden and parity tests
//! compare it with Wiki.js by ([`skeleton`]). No git, HTTP or template
//! dependency: `&str` in, rendered output out.

mod adapt;
mod dom_clobber;
pub mod frontmatter;
pub mod heading;
pub mod lines;
pub mod markdown;
pub mod sanitize;
pub mod sec;
pub mod skeleton;
pub mod slug;
mod uslug_tables;
pub mod writer;

pub use frontmatter::{split_front_matter, FrontMatter};
pub use heading::Heading;
pub use lines::LineTable;
pub use markdown::{
    render, render_page, Link, LinkKind, RawHtml, RenderOptions, Rendered, TaskItem,
};
pub use markdown::{Highlight, Highlighter, Profile};
pub use sec::{RenderFinding, SecHidden, SecMarker};
