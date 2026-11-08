//! Golden skeletons: an HTML page body reduced to what a reader sees and a
//! link can reach, so Wiki.js's stored render and Rubrica's can be compared
//! line by line (the parity crawl uses the same reducer).
//!
//! A skeleton is one line per element, indented by depth: the tag, `#id`,
//! ` href=…`, ` src=…`, ` alt=…`, ` title=…`, an `ol`'s ` start=…`, a cell's
//! ` align=…` (from `align`, or `text-align` in its `style`: Wiki.js writes
//! `style="text-align:right"`, the writer `align="right"`), an `input`'s
//! ` type=…` and ` checked` (a task checkbox), a `details`'s ` open`, and one
//! line per text run (`"…"`). A `<pre>`'s text is kept exactly, trailing
//! blanks too, on the `pre` line, with its fence's language (` lang=x`, from
//! the `language-x` class of the `<code>` in it; Wiki.js's `language-` with
//! no language is none), and nothing inside it is an element (Wiki.js
//! highlights in the browser; Rubrica's markup inside a `<pre>` is its own).
//! Classes and every other attribute are dropped: Wiki.js's class soup is
//! not the contract.
//!
//! ASCII whitespace (space, tab, line feed, form feed, carriage return) in
//! a text run collapses to one blank, as a browser shows it; NBSP, U+3000,
//! U+2028 and the rest of Unicode's spaces are text, which it shows. The
//! blank is kept at an edge that touches inline content (`a <b>b</b>` is not
//! `a<b>b</b>`), dropped at one that touches a block's start or end, a block
//! element or a `<br>` (where a browser drops it too, and where Wiki.js's
//! and the writer's newlines differ).
//!
//! Where the two differ on purpose, the difference is an [`Allowance`],
//! named, justified and applied explicitly; each one counts its uses, and
//! [`unused`] lists those that were never needed, so the corpus test fails
//! when one is no longer used and the list cannot rot.

use std::cell::RefCell;
use std::collections::BTreeMap;

use html5ever::tokenizer::{
    BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};

/// Which renderer wrote the HTML.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Wiki.js 2.5.314's stored render (`pages.render`).
    WikiJs,
    /// [`crate::render`]'s body HTML.
    Rubrica,
}

/// A named, justified difference the reducer takes away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Allowance {
    /// Its name, as counted in [`Skeleton::used`].
    pub name: &'static str,
    /// Which side it applies to.
    pub source: Source,
    /// Why it is allowed.
    pub why: &'static str,
}

/// Every allowance, in the order they are applied. Each is used by the
/// private conformance corpus.
pub const ALLOWANCES: &[Allowance] = &[
    Allowance {
        name: "wikijs-toc-anchor",
        source: Source::WikiJs,
        why: "Wiki.js prepends `<a class=\"toc-anchor\" href=\"#id\">¶</a>` to every heading; \
              Rubrica's page template, not its renderer, draws heading links",
    },
    Allowance {
        name: "wikijs-table-container",
        source: Source::WikiJs,
        why: "Wiki.js wraps every table in `<div class=\"table-container\">` for its CSS",
    },
    Allowance {
        name: "mermaid-container",
        source: Source::WikiJs,
        why: "Wiki.js's Mermaid 8 reads `<div class=\"mermaid\">`; Rubrica writes the \
              fence as `<pre><code class=\"language-mermaid\">`, no script draws it; the \
              source text is compared",
    },
    Allowance {
        name: "mermaid-container",
        source: Source::Rubrica,
        why: "the other side of the same allowance",
    },
    Allowance {
        name: "strikethrough-tag",
        source: Source::WikiJs,
        why: "markdown-it writes `~~x~~` as `<s>`, the writer as `<del>`; same meaning",
    },
    Allowance {
        name: "linkify-artifact",
        source: Source::WikiJs,
        why: "markdown-it's bare-domain linkify (`http://example.io`, `http://README.md`) \
              is not reproduced; exactly the hrefs the page context lists, per page, \
              are unwrapped to their text",
    },
    Allowance {
        name: "markdown-it-sub",
        source: Source::WikiJs,
        why: "markdown-it-sub reads `~90→~30` as `<sub>90→</sub>30` (the corpus's one, \
              services/calm-farm/ios-ux-ui-backlog); Rubrica keeps the tildes as text",
    },
    Allowance {
        name: "sec-anchor",
        source: Source::Rubrica,
        why: "Wiki.js's DOMPurify deletes the `@sec` comments; Rubrica turns each into \
              `<span class=\"sec\" id=\"sec-…\">` on its heading, an anchor Wiki.js never had",
    },
];

/// What a page's reduction needs to know beyond its HTML.
#[derive(Debug, Clone, Default)]
pub struct PageContext {
    /// The page's linkify artefacts (hrefs, one entry per occurrence), from
    /// the private conformance corpus's artefact list.
    pub linkify: Vec<String>,
}

/// A reduced page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skeleton {
    /// The lines.
    pub lines: Vec<String>,
    /// How many times each allowance was applied.
    pub used: BTreeMap<&'static str, usize>,
    /// Linkify artefacts of the [`PageContext`] the page did not have.
    pub unused_linkify: Vec<String>,
}

impl Skeleton {
    /// The lines, one per line.
    pub fn text(&self) -> String {
        let mut s = self.lines.join("\n");
        s.push('\n');
        s
    }
}

/// The allowance names in [`ALLOWANCES`] that `used` never counts.
pub fn unused(used: &BTreeMap<&'static str, usize>) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = ALLOWANCES.iter().map(|a| a.name).collect();
    names.dedup();
    names
        .into_iter()
        .filter(|n| used.get(n).copied().unwrap_or(0) == 0)
        .collect()
}

#[derive(Debug, Clone)]
enum Node {
    Element {
        name: String,
        attrs: Vec<(String, String)>,
        children: Vec<Node>,
    },
    Text(String),
}

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];

#[derive(Default)]
struct TreeSink {
    // The open elements; the root is the bottom one.
    stack: RefCell<Vec<Node>>,
}

impl TreeSink {
    fn close_top(stack: &mut Vec<Node>) {
        let node = stack.pop().expect("the root is never closed");
        if let Some(Node::Element { children, .. }) = stack.last_mut() {
            children.push(node);
        }
    }
}

impl TokenSink for TreeSink {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut stack = self.stack.borrow_mut();
        match token {
            Token::TagToken(tag) => {
                let name = tag.name.to_string();
                match tag.kind {
                    TagKind::StartTag => {
                        let node = Node::Element {
                            attrs: tag
                                .attrs
                                .iter()
                                .map(|a| (a.name.local.to_string(), a.value.to_string()))
                                .collect(),
                            name: name.clone(),
                            children: Vec::new(),
                        };
                        stack.push(node);
                        if VOID.contains(&name.as_str()) || tag.self_closing {
                            Self::close_top(&mut stack);
                        }
                    }
                    TagKind::EndTag => {
                        // Close up to the matching element, if one is open.
                        let open = stack.iter().skip(1).rposition(
                            |n| matches!(n, Node::Element { name: m, .. } if *m == name),
                        );
                        if let Some(at) = open {
                            while stack.len() > at + 1 {
                                Self::close_top(&mut stack);
                            }
                        }
                    }
                }
            }
            Token::CharacterTokens(t) => {
                if let Some(Node::Element { children, .. }) = stack.last_mut() {
                    match children.last_mut() {
                        Some(Node::Text(s)) => s.push_str(&t),
                        _ => children.push(Node::Text(t.to_string())),
                    }
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

fn parse(html: &str) -> Vec<Node> {
    let sink = TreeSink::default();
    sink.stack.borrow_mut().push(Node::Element {
        name: String::new(),
        attrs: Vec::new(),
        children: Vec::new(),
    });
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(html.into());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let mut stack = tokenizer.sink.stack.into_inner();
    while stack.len() > 1 {
        TreeSink::close_top(&mut stack);
    }
    match stack.pop() {
        Some(Node::Element { children, .. }) => children,
        _ => Vec::new(),
    }
}

fn attr<'n>(attrs: &'n [(String, String)], name: &str) -> Option<&'n str> {
    attrs
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn has_class(attrs: &[(String, String)], class: &str) -> bool {
    attr(attrs, "class").is_some_and(|c| c.split_ascii_whitespace().any(|x| x == class))
}

struct Reducer {
    source: Source,
    linkify: Vec<String>,
    used: BTreeMap<&'static str, usize>,
}

impl Reducer {
    fn allow(&mut self, name: &'static str) -> bool {
        let known = ALLOWANCES
            .iter()
            .any(|a| a.name == name && a.source == self.source);
        if known {
            *self.used.entry(name).or_default() += 1;
        }
        known
    }

    /// The allowances on a list of siblings, depth first.
    fn apply(&mut self, nodes: Vec<Node>) -> Vec<Node> {
        let mut out = Vec::with_capacity(nodes.len());
        for node in nodes {
            let Node::Element {
                mut name,
                attrs,
                children,
            } = node
            else {
                out.push(node);
                continue;
            };
            let class = |c: &str| has_class(&attrs, c);
            if name == "a" && class("toc-anchor") && self.allow("wikijs-toc-anchor") {
                continue;
            }
            if name == "span" && class("sec") && self.allow("sec-anchor") {
                continue;
            }
            if name == "a" {
                let href = attr(&attrs, "href").unwrap_or("");
                if let Some(at) = self.linkify.iter().position(|h| h == href) {
                    if self.allow("linkify-artifact") {
                        self.linkify.remove(at);
                        out.extend(self.apply(children));
                        continue;
                    }
                }
            }
            if name == "div" && class("table-container") && self.allow("wikijs-table-container") {
                out.extend(self.apply(children));
                continue;
            }
            if name == "sub" && self.allow("markdown-it-sub") {
                out.push(Node::Text("~".into()));
                out.extend(self.apply(children));
                out.push(Node::Text("~".into()));
                continue;
            }
            // Wiki.js draws a diagram from `<div class="mermaid">`; Rubrica writes the
            // fence as code, `<pre><code class="language-mermaid">` (no script draws it).
            // Both reduce to one `mermaid` element holding the source.
            if self.source == Source::WikiJs
                && name == "div"
                && class("mermaid")
                && self.allow("mermaid-container")
            {
                name = "mermaid".into();
            }
            let mut children = children;
            if self.source == Source::Rubrica && name == "pre" {
                let diagram = matches!(
                    children.as_slice(),
                    [Node::Element { name: inner, attrs: inner_attrs, .. }]
                        if inner == "code" && has_class(inner_attrs, "language-mermaid")
                );
                if diagram && self.allow("mermaid-container") {
                    if let Some(Node::Element { children: code, .. }) = children.pop() {
                        children = code;
                    }
                    name = "mermaid".into();
                }
            }
            if name == "s" && self.allow("strikethrough-tag") {
                name = "del".into();
            }
            let children = self.apply(children);
            out.push(Node::Element {
                name,
                attrs,
                children,
            });
        }
        out
    }
}

fn text_of(nodes: &[Node], out: &mut String) {
    for n in nodes {
        match n {
            Node::Text(t) => out.push_str(t),
            Node::Element { children, .. } => text_of(children, out),
        }
    }
}

/// Elements a text run beside them flows into: whitespace between the two
/// shows as a blank. (`br` is not one: a blank before or after a line
/// break does not show.)
const INLINE: &[&str] = &[
    "a", "abbr", "b", "cite", "code", "del", "dfn", "em", "i", "img", "input", "kbd", "mark", "q",
    "s", "samp", "small", "span", "strong", "sub", "sup", "u", "var",
];

fn is_inline(node: Option<&Node>) -> bool {
    matches!(node, Some(Node::Element { name, .. }) if INLINE.contains(&name.as_str()))
}

/// A `<pre>`'s fence language: `x` of its `<code>`'s first `language-x`
/// class, none for `language-` alone (Wiki.js's fence with no language).
fn fence_language(children: &[Node]) -> Option<&str> {
    children.iter().find_map(|n| match n {
        Node::Element { name, attrs, .. } if name == "code" => attr(attrs, "class")?
            .split_ascii_whitespace()
            .find_map(|c| c.strip_prefix("language-"))
            .filter(|l| !l.is_empty()),
        _ => None,
    })
}

/// A cell's alignment, from `align` or the `text-align` of its `style`.
fn alignment(attrs: &[(String, String)]) -> Option<String> {
    if let Some(a) = attr(attrs, "align") {
        return Some(a.trim().to_ascii_lowercase());
    }
    attr(attrs, "style")?.split(';').find_map(|decl| {
        let (k, v) = decl.split_once(':')?;
        (k.trim().eq_ignore_ascii_case("text-align")).then(|| v.trim().to_ascii_lowercase())
    })
}

/// The lines of `nodes`, the children of an element that is inline
/// (`inline_parent`) or not.
fn lines_of(nodes: &[Node], depth: usize, inline_parent: bool, out: &mut Vec<String>) {
    let indent = "  ".repeat(depth);
    let mut text = String::new();
    // `before`/`after`: the edges touch inline content.
    let flush = |text: &mut String, before: bool, after: bool, out: &mut Vec<String>| {
        let mut t = text.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
        let blank = |c: char| c.is_ascii_whitespace();
        if !t.is_empty() {
            if before && text.starts_with(blank) {
                t.insert(0, ' ');
            }
            if after && text.ends_with(blank) {
                t.push(' ');
            }
        } else if before && after && !text.is_empty() {
            t.push(' ');
        }
        if !t.is_empty() {
            out.push(format!("{indent}{t:?}"));
        }
        text.clear();
    };
    // Where the current run started: the node before it, if any.
    let mut run_start: Option<usize> = None;
    for (i, n) in nodes.iter().enumerate() {
        match n {
            Node::Text(t) => {
                if run_start.is_none() {
                    run_start = Some(i);
                }
                text.push_str(t);
            }
            Node::Element {
                name,
                attrs,
                children,
            } => {
                if let Some(start) = run_start.take() {
                    let before = match start.checked_sub(1) {
                        Some(p) => is_inline(nodes.get(p)),
                        None => inline_parent,
                    };
                    flush(&mut text, before, is_inline(Some(n)), out);
                }
                let mut line = format!("{indent}{name}");
                if let Some(id) = attr(attrs, "id") {
                    line.push('#');
                    line.push_str(id);
                }
                for key in ["href", "src"] {
                    if let Some(v) = attr(attrs, key) {
                        line.push_str(&format!(" {key}={v}"));
                    }
                }
                // Free text: quoted.
                for key in ["alt", "title"] {
                    if let Some(v) = attr(attrs, key) {
                        line.push_str(&format!(" {key}={v:?}"));
                    }
                }
                if name == "ol" {
                    if let Some(v) = attr(attrs, "start") {
                        line.push_str(&format!(" start={}", v.trim()));
                    }
                }
                if name == "th" || name == "td" {
                    if let Some(a) = alignment(attrs) {
                        line.push_str(&format!(" align={a}"));
                    }
                }
                if name == "input" {
                    if let Some(v) = attr(attrs, "type") {
                        line.push_str(&format!(" type={}", v.trim().to_ascii_lowercase()));
                    }
                    if attr(attrs, "checked").is_some() {
                        line.push_str(" checked");
                    }
                }
                if name == "details" && attr(attrs, "open").is_some() {
                    line.push_str(" open");
                }
                if name == "pre" {
                    if let Some(lang) = fence_language(children) {
                        line.push_str(&format!(" lang={lang}"));
                    }
                }
                if name == "pre" || name == "mermaid" {
                    let mut t = String::new();
                    text_of(children, &mut t);
                    // Mermaid's source is the fence's, the same on both
                    // sides but for Wiki.js's trailing newline.
                    let t = if name == "mermaid" { t.trim_end() } else { &t };
                    line.push_str(&format!(": {t:?}"));
                    out.push(line);
                    continue;
                }
                out.push(line);
                let inline = INLINE.contains(&name.as_str());
                lines_of(children, depth + 1, inline, out);
            }
        }
    }
    if let Some(start) = run_start {
        let before = match start.checked_sub(1) {
            Some(p) => is_inline(nodes.get(p)),
            None => inline_parent,
        };
        flush(&mut text, before, inline_parent, out);
    }
}

/// Reduce a page body.
pub fn skeleton(html: &str, source: Source, ctx: &PageContext) -> Skeleton {
    let mut reducer = Reducer {
        source,
        linkify: ctx.linkify.clone(),
        used: BTreeMap::new(),
    };
    let nodes = reducer.apply(parse(html));
    let mut lines = Vec::new();
    lines_of(&nodes, 0, false, &mut lines);
    Skeleton {
        lines,
        used: reducer.used,
        unused_linkify: reducer.linkify,
    }
}
