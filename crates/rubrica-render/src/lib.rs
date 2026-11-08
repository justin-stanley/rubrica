//! Markdown in, sanitized HTML out, for the
//! [Rubrica](https://github.com/justin-stanley/rubrica) wiki engine.
//!
//! [`render`] turns a Markdown body into a [`Rendered`]: the HTML and everything a wiki
//! needs from the same parse (headings with their ids, links, task items, the plain text
//! for search, and [`RenderFinding`]s for what is wrong with the page). [`render_page`]
//! does the same for a whole page file with YAML front matter. The input is `&str` and the
//! output is a value: this crate does no I/O, and has no git, HTTP or template dependency.
//!
//! # Example
//!
//! ```rust
//! use rubrica_render::{render, RenderOptions};
//!
//! let rendered = render("# Hello\n\nSome *Markdown*.\n", &RenderOptions::native());
//! assert!(rendered.html.contains("<em>Markdown</em>"));
//! assert_eq!(rendered.headings[0].text, "Hello");
//! ```
//!
//! # Render profiles
//!
//! A [`Profile`] picks the rules. [`RenderOptions::native`] (the default) is Rubrica's own:
//! CommonMark's HTML and GFM task lists. [`RenderOptions::wikijs_compat`] reproduces the
//! output of Wiki.js 2 (its line breaks, heading ids, task items and a few details) for
//! wikis that migrated from it. Raw HTML is always sanitized ([`sanitize`]), or escaped
//! with [`RawHtml::Escape`].
//!
//! # Also in this crate
//!
//! - [`frontmatter`]: split and read a page's YAML front matter.
//! - [`slug`]: the heading-id slugger.
//! - [`sec`]: `@sec` section markers.
//! - [`writer`]: the HTML writer over [`rubrica_markdown`] events, for callers that bring
//!   their own events.
//! - [`skeleton`]: reduces HTML to a comparable outline; used by tests that compare
//!   renders.
//!
//! # The other crates
//!
//! - [`rubrica-markdown`](https://docs.rs/rubrica-markdown) is the parser this crate builds on.
//! - [`rubrica-site`](https://docs.rs/rubrica-site) uses this crate to build a wiki's pages,
//!   links and search index.
//! - [`rubrica`](https://docs.rs/rubrica) is the command-line tool.
//!
//! # Minimum supported Rust version
//!
//! Rust 1.94. A raise is a minor-version change.
//!
//! # Licence
//!
//! AGPL-3.0-only.

#![warn(missing_docs)]

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
