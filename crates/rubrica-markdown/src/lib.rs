// Copyright 2015 Google Inc. All rights reserved.
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.
// Vendored by Rubrica from pulldown-cmark 0.13.6, src/lib.rs (MIT; see UPSTREAM and NOTICE).

//! Rubrica's Markdown parser: a pull parser for [CommonMark](https://commonmark.org)
//! 0.31.2 plus the GitHub Flavored Markdown extensions, written for the
//! [Rubrica](https://github.com/justin-stanley/rubrica) wiki engine. Its [`Parser`] is an
//! iterator over [`Event`]s, and [`parse_with_offsets`] pairs each event with its byte
//! range in the source.
//!
//! Use this crate when you need the structure of a Markdown document (events and source
//! positions) and not HTML. To turn Markdown into sanitized HTML, use
//! [`rubrica-render`](https://docs.rs/rubrica-render), which builds on this crate.
//!
//! # Example
//!
//! ```rust
//! use rubrica_markdown::{parse, Event, Tag, GFM};
//!
//! let events: Vec<Event> = parse("a ~~b~~ *c*", GFM).collect();
//! assert!(events.contains(&Event::Start(Tag::Strikethrough)));
//! assert!(events.contains(&Event::Start(Tag::Emphasis)));
//! ```
//!
//! Source positions come from [`parse_with_offsets`]:
//!
//! ```rust
//! use rubrica_markdown::{parse_with_offsets, Event, GFM};
//!
//! let src = "# Title\n\ntext";
//! let (event, range) = parse_with_offsets(src, GFM)
//!     .find(|(e, _)| matches!(e, Event::Text(t) if &**t == "text"))
//!     .unwrap();
//! assert_eq!(&src[range], "text");
//! # let _ = event;
//! ```
//!
//! # Which function to call
//!
//! - [`parse`] is the plain event stream.
//! - [`parse_with_offsets`] adds each event's byte range in the source.
//! - [`events`] is the stream Rubrica renders: [`parse_with_offsets`] with consecutive text
//!   merged ([`textmerge`]) and GFM extended autolinks (`www.`, `http://`, `https://`,
//!   `ftp://`, email) found in it ([`autolink`]).
//!
//! [`GFM`] is Rubrica's option set: tables, strikethrough, task lists and footnotes.
//! Extended autolinks are Rubrica's own pass over the events, not an [`Options`] flag,
//! and upstream's `ENABLE_GFM` (blockquote alerts) is not part of it. Parsing returns no
//! errors: every `&str` has an event stream.
//!
//! # The other crates
//!
//! - [`rubrica-render`](https://docs.rs/rubrica-render) writes these events as sanitized HTML.
//! - [`rubrica-site`](https://docs.rs/rubrica-site) builds a wiki's pages, links and search
//!   index from rendered pages.
//! - [`rubrica`](https://docs.rs/rubrica) is the command-line tool.
//!
//! # Provenance
//!
//! The block and inline parsers derive from pulldown-cmark 0.13.6 by Raph Levien and
//! contributors, MIT (see `UPSTREAM` and `NOTICE` in this crate's repository); the
//! vendored files keep their upstream headers. Upstream changes are ported by hand after
//! reading the diff, never merged blind. The vendored internals keep their upstream
//! comments; the public items are documented here.
//!
//! # Minimum supported Rust version
//!
//! Rust 1.94. A raise is a minor-version change.
//!
//! # Licence
//!
//! AGPL-3.0-only AND MIT: the vendored files are MIT, the rest AGPL-3.0-only.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]
#![warn(missing_docs)]
// vendored; lints silenced, not fixed
#![allow(
    clippy::large_const_arrays,
    clippy::unnecessary_map_or,
    clippy::type_complexity,
    clippy::single_match,
    clippy::byte_char_slices,
    clippy::manual_is_multiple_of,
    clippy::get_first,
    clippy::manual_contains,
    clippy::match_ref_pats,
    clippy::useless_borrows_in_formatting,
    clippy::collapsible_match,
    clippy::extra_unused_lifetimes,
    clippy::repeat_once,
    clippy::assertions_on_constants
)]

mod entities;
mod firstpass;
mod linklabel;
mod parse;
mod puncttable;
mod scanners;
mod strings;
mod tree;

use std::fmt::Display;

pub use crate::parse::{
    BrokenLink, BrokenLinkCallback, DefaultBrokenLinkCallback, OffsetIter, Parser, RefDefs,
};
pub use crate::strings::{CowStr, InlineStr};

/// Rubrica's GFM option set: tables, strikethrough, task lists and footnotes.
/// Extended autolinks are Rubrica's own pass over the events (`autolink`),
/// not an upstream option. Upstream's `ENABLE_GFM` (blockquote
/// alerts) is not part of it.
pub const GFM: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_FOOTNOTES);

/// The event stream of `src` under `options`.
pub fn parse(src: &str, options: Options) -> Parser<'_> {
    Parser::new_ext(src, options)
}

/// The event stream with each event's byte range in `src`.
pub fn parse_with_offsets(src: &str, options: Options) -> OffsetIter<'_> {
    Parser::new_ext(src, options).into_offset_iter()
}

pub mod autolink;
pub mod textmerge;

/// The event stream Rubrica renders: `parse_with_offsets` with text runs
/// merged and extended autolinks found.
pub fn events(src: &str, options: Options) -> autolink::Autolinks<'_, OffsetIter<'_>> {
    autolink::Autolinks::new(parse_with_offsets(src, options), src)
}

/// Codeblock kind.
#[derive(Clone, Debug, PartialEq)]
pub enum CodeBlockKind<'a> {
    /// An indented code block.
    Indented,
    /// The value contained in the tag describes the language of the code, which may be empty.
    Fenced(CowStr<'a>),
}

impl<'a> CodeBlockKind<'a> {
    /// Whether this is an indented code block.
    pub fn is_indented(&self) -> bool {
        matches!(*self, CodeBlockKind::Indented)
    }

    /// Whether this is a fenced code block.
    pub fn is_fenced(&self) -> bool {
        matches!(*self, CodeBlockKind::Fenced(_))
    }

    /// This kind with its language tag owned (`'static`), so it can outlive the source.
    pub fn into_static(self) -> CodeBlockKind<'static> {
        match self {
            CodeBlockKind::Indented => CodeBlockKind::Indented,
            CodeBlockKind::Fenced(s) => CodeBlockKind::Fenced(s.into_static()),
        }
    }
}

/// BlockQuote kind (Note, Tip, Important, Warning, Caution).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BlockQuoteKind {
    /// A `[!NOTE]` alert.
    Note,
    /// A `[!TIP]` alert.
    Tip,
    /// An `[!IMPORTANT]` alert.
    Important,
    /// A `[!WARNING]` alert.
    Warning,
    /// A `[!CAUTION]` alert.
    Caution,
}

/// Metadata block kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MetadataBlockKind {
    /// A block delimited by `---` lines (YAML front matter).
    YamlStyle,
    /// A block delimited by `+++` lines (TOML front matter).
    PlusesStyle,
}

/// Tags for elements that can contain other elements.
#[derive(Clone, Debug, PartialEq)]
pub enum Tag<'a> {
    /// A paragraph of text and other inline elements.
    Paragraph,

    /// A heading, with optional identifier, classes and custom attributes.
    /// The identifier is prefixed with `#` and the last one in the attributes
    /// list is chosen, classes are prefixed with `.` and custom attributes
    /// have no prefix and can optionally have a value (`myattr` or `myattr=myvalue`).
    ///
    /// `id`, `classes` and `attrs` are only parsed and populated with [`Options::ENABLE_HEADING_ATTRIBUTES`], `None` or empty otherwise.
    Heading {
        /// The heading's level, `H1` to `H6`.
        level: HeadingLevel,
        /// The identifier from `{ #id }`, without the `#`.
        id: Option<CowStr<'a>>,
        /// The classes from `{ .class }`, without the `.`.
        classes: Vec<CowStr<'a>>,
        /// The first item of the tuple is the attr and second one the value.
        attrs: Vec<(CowStr<'a>, Option<CowStr<'a>>)>,
    },

    /// A block quote.
    ///
    /// The `BlockQuoteKind` is only parsed & populated with [`Options::ENABLE_GFM`], `None` otherwise.
    ///
    /// ```markdown
    /// > regular quote
    ///
    /// > [!NOTE]
    /// > note quote
    /// ```
    BlockQuote(Option<BlockQuoteKind>),
    /// A code block.
    CodeBlock(CodeBlockKind<'a>),

    /// An HTML block.
    ///
    /// A line that begins with some predefined tags (HTML block tags) (see [CommonMark Spec](https://spec.commonmark.org/0.31.2/#html-blocks) for more details) or any tag that is followed only by whitespace.
    ///
    /// Most HTML blocks end on an empty line, though some e.g. `<pre>` like `<script>` or `<!-- Comments -->` don't.
    /// ```markdown
    /// <body> Is HTML block even though here is non-whitespace.
    /// Block ends on an empty line.
    ///
    /// <some-random-tag>
    /// This is HTML block.
    ///
    /// <pre> Doesn't end on empty lines.
    ///
    /// This is still the same block.</pre>
    /// ```
    HtmlBlock,

    /// A list. If the list is ordered the field indicates the number of the first item.
    /// Contains only list items.
    List(Option<u64>), // TODO: add delim and tight for ast (not needed for html)
    /// A list item.
    Item,
    /// A footnote definition. The value contained is the footnote's label by which it can
    /// be referred to.
    ///
    /// Only parsed and emitted with [`Options::ENABLE_FOOTNOTES`] or [`Options::ENABLE_OLD_FOOTNOTES`].
    FootnoteDefinition(CowStr<'a>),

    /// Only parsed and emitted with [`Options::ENABLE_DEFINITION_LIST`].
    DefinitionList,
    /// Only parsed and emitted with [`Options::ENABLE_DEFINITION_LIST`].
    DefinitionListTitle,
    /// Only parsed and emitted with [`Options::ENABLE_DEFINITION_LIST`].
    DefinitionListDefinition,

    /// A table. Contains a vector describing the text-alignment for each of its columns.
    /// Only parsed and emitted with [`Options::ENABLE_TABLES`].
    Table(Vec<Alignment>),
    /// A table header. Contains only `TableCell`s. Note that the table body starts immediately
    /// after the closure of the `TableHead` tag. There is no `TableBody` tag.
    /// Only parsed and emitted with [`Options::ENABLE_TABLES`].
    TableHead,
    /// A table row. Is used both for header rows as body rows. Contains only `TableCell`s.
    /// Only parsed and emitted with [`Options::ENABLE_TABLES`].
    TableRow,
    /// Only parsed and emitted with [`Options::ENABLE_TABLES`].
    TableCell,

    // span-level tags
    /// [Emphasis](https://spec.commonmark.org/0.31.2/#emphasis-and-strong-emphasis).
    /// ```markdown
    /// half*emph* _strong_ _multi _level__
    /// ```
    Emphasis,
    /// [Strong emphasis](https://spec.commonmark.org/0.31.2/#emphasis-and-strong-emphasis).
    /// ```markdown
    /// half**strong** __strong__ __multi __level____
    /// ```
    Strong,
    /// Only parsed and emitted with [`Options::ENABLE_STRIKETHROUGH`].
    ///
    /// ```markdown
    /// ~strike through~
    /// ```
    Strikethrough,
    /// Only parsed and emitted with [`Options::ENABLE_SUPERSCRIPT`].
    ///
    /// ```markdown
    /// ^superscript^
    /// ```
    Superscript,
    /// Only parsed and emitted with [`Options::ENABLE_SUBSCRIPT`], if disabled `~something~` is parsed as [`Strikethrough`](Self::Strikethrough).
    /// ```markdown
    /// ~subscript~ ~~if also enabled this is strikethrough~~
    /// ```
    Subscript,

    /// A link.
    Link {
        /// How the link was written: inline, reference, autolink and so on.
        link_type: LinkType,
        /// The destination URL, as written (entities and escapes resolved).
        dest_url: CowStr<'a>,
        /// The link title, empty when there is none.
        title: CowStr<'a>,
        /// Identifier of reference links, e.g. `world` in the link `[hello][world]`.
        id: CowStr<'a>,
    },

    /// An image. The first field is the link type, the second the destination URL and the third is a title,
    /// the fourth is the link identifier.
    Image {
        /// How the image was written: inline, reference, autolink and so on.
        link_type: LinkType,
        /// The image source URL, as written (entities and escapes resolved).
        dest_url: CowStr<'a>,
        /// The image title, empty when there is none.
        title: CowStr<'a>,
        /// Identifier of reference links, e.g. `world` in the link `[hello][world]`.
        id: CowStr<'a>,
    },

    /// A metadata block.
    /// Only parsed and emitted with [`Options::ENABLE_YAML_STYLE_METADATA_BLOCKS`]
    /// or [`Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS`].
    MetadataBlock(MetadataBlockKind),
}

impl<'a> Tag<'a> {
    /// The [`TagEnd`] that closes this tag.
    pub fn to_end(&self) -> TagEnd {
        match self {
            Tag::Paragraph => TagEnd::Paragraph,
            Tag::Heading { level, .. } => TagEnd::Heading(*level),
            Tag::BlockQuote(kind) => TagEnd::BlockQuote(*kind),
            Tag::CodeBlock(_) => TagEnd::CodeBlock,
            Tag::HtmlBlock => TagEnd::HtmlBlock,
            Tag::List(number) => TagEnd::List(number.is_some()),
            Tag::Item => TagEnd::Item,
            Tag::FootnoteDefinition(_) => TagEnd::FootnoteDefinition,
            Tag::Table(_) => TagEnd::Table,
            Tag::TableHead => TagEnd::TableHead,
            Tag::TableRow => TagEnd::TableRow,
            Tag::TableCell => TagEnd::TableCell,
            Tag::Subscript => TagEnd::Subscript,
            Tag::Superscript => TagEnd::Superscript,
            Tag::Emphasis => TagEnd::Emphasis,
            Tag::Strong => TagEnd::Strong,
            Tag::Strikethrough => TagEnd::Strikethrough,
            Tag::Link { .. } => TagEnd::Link,
            Tag::Image { .. } => TagEnd::Image,
            Tag::MetadataBlock(kind) => TagEnd::MetadataBlock(*kind),
            Tag::DefinitionList => TagEnd::DefinitionList,
            Tag::DefinitionListTitle => TagEnd::DefinitionListTitle,
            Tag::DefinitionListDefinition => TagEnd::DefinitionListDefinition,
        }
    }

    /// This tag with every string it holds owned (`'static`), so it can outlive the source.
    pub fn into_static(self) -> Tag<'static> {
        match self {
            Tag::Paragraph => Tag::Paragraph,
            Tag::Heading {
                level,
                id,
                classes,
                attrs,
            } => Tag::Heading {
                level,
                id: id.map(|s| s.into_static()),
                classes: classes.into_iter().map(|s| s.into_static()).collect(),
                attrs: attrs
                    .into_iter()
                    .map(|(k, v)| (k.into_static(), v.map(|s| s.into_static())))
                    .collect(),
            },
            Tag::BlockQuote(k) => Tag::BlockQuote(k),
            Tag::CodeBlock(kb) => Tag::CodeBlock(kb.into_static()),
            Tag::HtmlBlock => Tag::HtmlBlock,
            Tag::List(v) => Tag::List(v),
            Tag::Item => Tag::Item,
            Tag::FootnoteDefinition(a) => Tag::FootnoteDefinition(a.into_static()),
            Tag::Table(v) => Tag::Table(v),
            Tag::TableHead => Tag::TableHead,
            Tag::TableRow => Tag::TableRow,
            Tag::TableCell => Tag::TableCell,
            Tag::Emphasis => Tag::Emphasis,
            Tag::Strong => Tag::Strong,
            Tag::Strikethrough => Tag::Strikethrough,
            Tag::Superscript => Tag::Superscript,
            Tag::Subscript => Tag::Subscript,
            Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            } => Tag::Link {
                link_type,
                dest_url: dest_url.into_static(),
                title: title.into_static(),
                id: id.into_static(),
            },
            Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            } => Tag::Image {
                link_type,
                dest_url: dest_url.into_static(),
                title: title.into_static(),
                id: id.into_static(),
            },
            Tag::MetadataBlock(v) => Tag::MetadataBlock(v),
            Tag::DefinitionList => Tag::DefinitionList,
            Tag::DefinitionListTitle => Tag::DefinitionListTitle,
            Tag::DefinitionListDefinition => Tag::DefinitionListDefinition,
        }
    }
}

/// The end of a `Tag`.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum TagEnd {
    /// The end of a paragraph.
    Paragraph,
    /// The end of a heading of the given level.
    Heading(HeadingLevel),

    /// The end of a block quote, with its alert kind if it had one.
    BlockQuote(Option<BlockQuoteKind>),
    /// The end of a code block.
    CodeBlock,

    /// The end of a raw HTML block.
    HtmlBlock,

    /// A list, `true` for ordered lists.
    List(bool),
    /// The end of a list item.
    Item,
    /// The end of a footnote definition.
    FootnoteDefinition,

    /// The end of a definition list.
    DefinitionList,
    /// The end of a definition list title.
    DefinitionListTitle,
    /// The end of a definition list definition.
    DefinitionListDefinition,

    /// The end of a table.
    Table,
    /// The end of a table's header row.
    TableHead,
    /// The end of a table row.
    TableRow,
    /// The end of a table cell.
    TableCell,

    /// The end of emphasis (`*a*`).
    Emphasis,
    /// The end of strong emphasis (`**a**`).
    Strong,
    /// The end of strikethrough (`~~a~~`).
    Strikethrough,
    /// The end of superscript.
    Superscript,
    /// The end of subscript.
    Subscript,

    /// The end of a link.
    Link,
    /// The end of an image.
    Image,

    /// The end of a metadata block.
    MetadataBlock(MetadataBlockKind),
}

/// Make sure `TagEnd` is no more than two bytes in size.
/// This is why it's used instead of just using `Tag`.
#[cfg(target_pointer_width = "64")]
const _STATIC_ASSERT_TAG_END_SIZE: [(); 2] = [(); std::mem::size_of::<TagEnd>()];

impl<'a> From<Tag<'a>> for TagEnd {
    fn from(value: Tag) -> Self {
        value.to_end()
    }
}

/// The level of a heading, from `H1` (the top level) to `H6`.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum HeadingLevel {
    /// Level 1, `<h1>`.
    H1 = 1,
    /// Level 2, `<h2>`.
    H2,
    /// Level 3, `<h3>`.
    H3,
    /// Level 4, `<h4>`.
    H4,
    /// Level 5, `<h5>`.
    H5,
    /// Level 6, `<h6>`.
    H6,
}

impl Display for HeadingLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::H1 => write!(f, "h1"),
            Self::H2 => write!(f, "h2"),
            Self::H3 => write!(f, "h3"),
            Self::H4 => write!(f, "h4"),
            Self::H5 => write!(f, "h5"),
            Self::H6 => write!(f, "h6"),
        }
    }
}

/// Returned when trying to convert a `usize` into a `Heading` but it fails
/// because the usize isn't a valid heading level
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct InvalidHeadingLevel(usize);

impl TryFrom<usize> for HeadingLevel {
    type Error = InvalidHeadingLevel;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::H1),
            2 => Ok(Self::H2),
            3 => Ok(Self::H3),
            4 => Ok(Self::H4),
            5 => Ok(Self::H5),
            6 => Ok(Self::H6),
            _ => Err(InvalidHeadingLevel(value)),
        }
    }
}

/// Type specifier for inline links. See [the Tag::Link](enum.Tag.html#variant.Link) for more information.
#[derive(Clone, Debug, PartialEq, Copy)]
pub enum LinkType {
    /// Inline link like `[foo](bar)`
    Inline,
    /// Reference link like `[foo][bar]`
    Reference,
    /// Reference without destination in the document, but resolved by the broken_link_callback
    ReferenceUnknown,
    /// Collapsed link like `[foo][]`
    Collapsed,
    /// Collapsed link without destination in the document, but resolved by the broken_link_callback
    CollapsedUnknown,
    /// Shortcut link like `[foo]`
    Shortcut,
    /// Shortcut without destination in the document, but resolved by the broken_link_callback
    ShortcutUnknown,
    /// Autolink like `<http://foo.bar/baz>`
    Autolink,
    /// Email address in autolink like `<john@example.org>`
    Email,
    /// Wikilink link like `[[foo]]` or `[[foo|bar]]`
    WikiLink {
        /// `true` if the wikilink was piped.
        ///
        /// * `true` - `[[foo|bar]]`
        /// * `false` - `[[foo]]`
        has_pothole: bool,
    },
}

impl LinkType {
    /// Map the link type to an equivalent _Unknown link type.
    fn to_unknown(self) -> Self {
        match self {
            LinkType::Reference => LinkType::ReferenceUnknown,
            LinkType::Collapsed => LinkType::CollapsedUnknown,
            LinkType::Shortcut => LinkType::ShortcutUnknown,
            _ => unreachable!(),
        }
    }
}

/// Markdown events that are generated in a preorder traversal of the document
/// tree, with additional `End` events whenever all of an inner node's children
/// have been visited.
#[derive(Clone, Debug, PartialEq)]
pub enum Event<'a> {
    /// Start of a tagged element. Events that are yielded after this event
    /// and before its corresponding `End` event are inside this element.
    /// Start and end events are guaranteed to be balanced.
    Start(Tag<'a>),
    /// End of a tagged element.
    End(TagEnd),
    /// A text node.
    ///
    /// All text, outside and inside [`Tag`]s.
    Text(CowStr<'a>),
    /// An [inline code node](https://spec.commonmark.org/0.31.2/#code-spans).
    ///
    /// ```markdown
    /// `code`
    /// ```
    Code(CowStr<'a>),
    /// An inline math environment node.
    /// Requires [`Options::ENABLE_MATH`].
    ///
    /// ```markdown
    /// $math$
    /// ```
    InlineMath(CowStr<'a>),
    /// A display math environment node.
    /// Requires [`Options::ENABLE_MATH`].
    ///
    /// ```markdown
    /// $$math$$
    /// ```
    DisplayMath(CowStr<'a>),
    /// An HTML node.
    ///
    /// A line of HTML inside [`Tag::HtmlBlock`] includes the line break.
    Html(CowStr<'a>),
    /// An [inline HTML node](https://spec.commonmark.org/0.31.2/#raw-html).
    ///
    /// Contains only the tag itself, e.g. `<open-tag>`, `</close-tag>` or `<!-- comment -->`.
    ///
    /// **Note**: Under some conditions HTML can also be parsed as an HTML Block, see [`Tag::HtmlBlock`] for details.
    InlineHtml(CowStr<'a>),
    /// A reference to a footnote with given label, which may or may not be defined
    /// by an event with a [`Tag::FootnoteDefinition`] tag. Definitions and references to them may
    /// occur in any order. Only parsed and emitted with [`Options::ENABLE_FOOTNOTES`] or [`Options::ENABLE_OLD_FOOTNOTES`].
    ///
    /// ```markdown
    /// [^1]
    /// ```
    FootnoteReference(CowStr<'a>),
    /// A [soft line break](https://spec.commonmark.org/0.31.2/#soft-line-breaks).
    ///
    /// Any line break that isn't a [`HardBreak`](Self::HardBreak), or the end of e.g. a paragraph.
    SoftBreak,
    /// A [hard line break](https://spec.commonmark.org/0.31.2/#hard-line-breaks).
    ///
    /// A line ending that is either preceded by at least two spaces or `\`.
    ///
    /// ```markdown
    /// hard··
    /// line\
    /// breaks
    /// ```
    /// *`·` is a space*
    HardBreak,
    /// A horizontal ruler.
    ///
    /// ```markdown
    /// ***
    /// ···---
    /// _·_··_····_··
    /// ```
    /// *`·` is any whitespace*
    Rule,
    /// A task list marker, rendered as a checkbox in HTML. Contains a true when it is checked.
    /// Only parsed and emitted with [`Options::ENABLE_TASKLISTS`].
    /// ```markdown
    /// - [ ] unchecked
    /// - [x] checked
    /// ```
    TaskListMarker(bool),
}

impl<'a> Event<'a> {
    /// This event with every string it holds owned (`'static`), so it can outlive the source.
    pub fn into_static(self) -> Event<'static> {
        match self {
            Event::Start(t) => Event::Start(t.into_static()),
            Event::End(e) => Event::End(e),
            Event::Text(s) => Event::Text(s.into_static()),
            Event::Code(s) => Event::Code(s.into_static()),
            Event::InlineMath(s) => Event::InlineMath(s.into_static()),
            Event::DisplayMath(s) => Event::DisplayMath(s.into_static()),
            Event::Html(s) => Event::Html(s.into_static()),
            Event::InlineHtml(s) => Event::InlineHtml(s.into_static()),
            Event::FootnoteReference(s) => Event::FootnoteReference(s.into_static()),
            Event::SoftBreak => Event::SoftBreak,
            Event::HardBreak => Event::HardBreak,
            Event::Rule => Event::Rule,
            Event::TaskListMarker(b) => Event::TaskListMarker(b),
        }
    }
}

/// Table column text alignment.
#[derive(Copy, Clone, Debug, PartialEq)]

pub enum Alignment {
    /// Default text alignment.
    None,
    /// Left-aligned.
    Left,
    /// Centered.
    Center,
    /// Right-aligned.
    Right,
}

bitflags::bitflags! {
    /// Option struct containing flags for enabling extra features
    /// that are not part of the CommonMark spec.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Options: u32 {
        /// GFM tables (`| a | b |` rows under a delimiter row).
        const ENABLE_TABLES = 1 << 1;
        /// GitHub-compatible footnote syntax.
        ///
        /// Footnotes are referenced with the syntax `[^IDENT]`,
        /// and defined with an identifier followed by a colon at top level.
        ///
        /// ---
        ///
        /// ```markdown
        /// Footnote referenced [^1].
        ///
        /// [^1]: footnote defined
        /// ```
        ///
        /// Footnote referenced [^1].
        ///
        /// [^1]: footnote defined
        const ENABLE_FOOTNOTES = 1 << 2;
        /// Strikethrough with `~~`.
        const ENABLE_STRIKETHROUGH = 1 << 3;
        /// Task list items (`- [ ]` and `- [x]`).
        const ENABLE_TASKLISTS = 1 << 4;
        /// Enables replacement of ASCII punctuation characters with
        /// Unicode ligatures and smart quotes.
        ///
        /// This includes replacing `--` with `—`, `---` with `—`, `...` with `…`,
        /// `"quote"` with `“quote”`, and `'quote'` with `‘quote’`.
        ///
        /// The replacement takes place during the parsing of the document.
        const ENABLE_SMART_PUNCTUATION = 1 << 5;
        /// Extension to allow headings to have ID and classes.
        ///
        /// `# text { #id .class1 .class2 myattr other_attr=myvalue }`
        /// is interpreted as a level 1 heading
        /// with the content `text`, ID `id`, classes `class1` and `class2` and
        /// custom attributes `myattr` (without value) and
        /// `other_attr` with value `myvalue`.
        /// Note that ID, classes, and custom attributes should be space-separated.
        const ENABLE_HEADING_ATTRIBUTES = 1 << 6;
        /// Metadata blocks in YAML style, i.e.:
        /// - starting with a `---` line
        /// - ending with a `---` or `...` line
        const ENABLE_YAML_STYLE_METADATA_BLOCKS = 1 << 7;
        /// Metadata blocks delimited by:
        /// - `+++` line at start
        /// - `+++` line at end
        const ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS = 1 << 8;
        /// Older footnote syntax. This flag implies `ENABLE_FOOTNOTES`, changing it to use an
        /// older syntax instead of the new, default, GitHub-compatible syntax.
        ///
        /// New syntax is different from the old syntax regarding
        /// indentation, nesting, and footnote references with no definition:
        ///
        /// ```markdown
        /// [^1]: In new syntax, this is two footnote definitions.
        /// [^2]: In old syntax, this is a single footnote definition with two lines.
        ///
        /// [^3]:
        ///
        ///     In new syntax, this is a footnote with two paragraphs.
        ///
        ///     In old syntax, this is a footnote followed by a code block.
        ///
        /// In new syntax, this undefined footnote definition renders as
        /// literal text [^4]. In old syntax, it creates a dangling link.
        /// ```
        const ENABLE_OLD_FOOTNOTES = (1 << 9) | (1 << 2);
        /// With this feature enabled, two events `Event::InlineMath` and `Event::DisplayMath`
        /// are emitted that conventionally contain TeX formulas.
        const ENABLE_MATH = 1 << 10;
        /// Misc GitHub Flavored Markdown features not supported in CommonMark.
        /// The following features are currently behind this tag:
        /// - Blockquote tags ([!NOTE], [!TIP], [!IMPORTANT], [!WARNING], [!CAUTION]).
        const ENABLE_GFM = 1 << 11;
        /// Commonmark-HS-Extensions compatible definition lists.
        ///
        /// ```markdown
        /// title 1
        ///   : definition 1
        /// title 2
        ///   : definition 2
        /// ```
        const ENABLE_DEFINITION_LIST = 1 << 12;
        /// Superscript with `^`.
        const ENABLE_SUPERSCRIPT = 1 << 13;
        /// Subscript with `~`.
        const ENABLE_SUBSCRIPT = 1 << 14;
        /// Obsidian-style Wikilinks.
        const ENABLE_WIKILINKS = 1 << 15;
    }
}

impl Options {
    pub(crate) fn has_gfm_footnotes(&self) -> bool {
        self.contains(Options::ENABLE_FOOTNOTES) && !self.contains(Options::ENABLE_OLD_FOOTNOTES)
    }
}
