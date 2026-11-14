//! The flowchart reader.

use crate::{preamble, Kind, Unsupported, Why, MAX_DEPTH, MAX_LABEL_BYTES};
use std::collections::BTreeMap;

const MAX_NODES: usize = 300;
const MAX_EDGES: usize = 600;
const MAX_SUBGRAPHS: usize = 50;
/// The longest `found` text, in chars, before it is cut.
const FOUND_CHARS: usize = 40;

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

/// Reads a flowchart from `src`: the preamble first, then the statements of the body.
pub fn parse(src: &str) -> Result<Flowchart, Unsupported> {
    let p = preamble(src)?;
    let other = match p.kind {
        Kind::Flowchart => None,
        Kind::Sequence => Some("sequenceDiagram"),
        Kind::State => Some("stateDiagram-v2"),
    };
    if let Some(keyword) = other {
        return Err(at(
            p.header_line,
            Why::UnknownType {
                keyword: keyword.to_string(),
            },
        ));
    }
    let (direction, tail) = header(p.header_rest).map_err(|why| at(p.header_line, why))?;
    let mut st = State::default();
    st.line(tail, p.header_line)?;
    for (n, text) in &p.body {
        st.line(text, *n)?;
    }
    if let Some(&open) = st.open.last() {
        return Err(at(
            st.subgraphs[open].line,
            Why::UnclosedBlock {
                keyword: "subgraph".into(),
            },
        ));
    }
    if st.nodes.is_empty() {
        return Err(at(0, Why::Empty));
    }
    Ok(Flowchart {
        direction,
        title: p.title,
        source: src.trim_end_matches('\n').to_string(),
        nodes: st.nodes,
        edges: st.edges,
        subgraphs: st.subgraphs,
    })
}

fn at(line: usize, why: Why) -> Unsupported {
    Unsupported { line, why }
}

/// `found` text for a `Syntax` failure: `s` trimmed, cut after 40 chars with `…`.
fn found(s: &str) -> String {
    let t = s.trim();
    let mut chars = t.chars();
    let head: String = chars.by_ref().take(FOUND_CHARS).collect();
    if chars.next().is_some() {
        head + "…"
    } else {
        head
    }
}

/// A `Syntax` failure reading `s` from byte `pos`.
fn syntax(s: &str, pos: usize) -> Why {
    Why::Syntax {
        found: found(&s[pos..]),
    }
}

/// The direction in the header's text after the keyword, and the statements after its `;`.
fn header(rest: &str) -> Result<(Direction, &str), Why> {
    let rest = rest.trim();
    if let Some(tail) = rest.strip_prefix(';') {
        return Ok((Direction::TopDown, tail));
    }
    let word_len = rest
        .find(|c: char| c.is_whitespace() || c == ';')
        .unwrap_or(rest.len());
    let direction = match &rest[..word_len] {
        "" | "TD" | "TB" => Direction::TopDown,
        "BT" => Direction::BottomUp,
        "LR" => Direction::LeftRight,
        "RL" => Direction::RightLeft,
        _ => return Err(syntax(rest, 0)),
    };
    let after = rest[word_len..].trim_start();
    if after.is_empty() {
        Ok((direction, ""))
    } else if let Some(tail) = after.strip_prefix(';') {
        Ok((direction, tail))
    } else {
        Err(syntax(rest, 0))
    }
}

/// Splits a line at the `;` outside a `"…"` quote and outside a shape's brackets.
fn split_statements(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut depth = 0usize;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '[' | '(' | '{' if !quoted => depth += 1,
            ']' | ')' | '}' if !quoted => depth = depth.saturating_sub(1),
            ';' if !quoted && depth == 0 => {
                out.push(&line[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&line[start..]);
    out
}

/// The label rules, shared by node text, edge labels and subgraph titles: `t` is trimmed and
/// not empty. `from` is where a control character is reported (the label's opener).
fn label(t: &str, s: &str, from: usize) -> Result<String, Why> {
    if t.len() > MAX_LABEL_BYTES {
        return Err(Why::LabelTooLong {
            bytes: t.len(),
            cap: MAX_LABEL_BYTES,
        });
    }
    let b = t.as_bytes();
    if b.windows(2)
        .any(|w| w[0] == b'<' && (w[1].is_ascii_alphabetic() || matches!(w[1], b'/' | b'!' | b'?')))
    {
        return Err(Why::HtmlLabel);
    }
    if t.chars()
        .any(|c| (c < '\u{20}' && c != '\t') || c == '\u{7f}')
    {
        return Err(syntax(s, from));
    }
    Ok(t.replace('\t', " "))
}

fn skip_spaces(s: &str, mut i: usize) -> usize {
    while matches!(s.as_bytes().get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    i
}

/// Reads a shape's text from the opener at `open` (of `open_len` bytes) to `closer`. Returns
/// the label and the position after the closer.
fn shape_text(s: &str, open: usize, open_len: usize, closer: &str) -> Result<(String, usize), Why> {
    let bad = || syntax(s, open);
    let i = skip_spaces(s, open + open_len);
    let (raw, end) = if s[i..].starts_with('"') {
        let q = i + 1;
        let len = s[q..].find('"').ok_or_else(bad)?;
        let j = skip_spaces(s, q + len + 1);
        if !s[j..].starts_with(closer) {
            return Err(bad());
        }
        (&s[q..q + len], j + closer.len())
    } else {
        let mut j = None;
        for (off, c) in s[i..].char_indices() {
            if s[i + off..].starts_with(closer) {
                j = Some(i + off);
                break;
            }
            if matches!(c, '[' | ']' | '(' | ')' | '{' | '}' | '"' | '|') {
                return Err(bad());
            }
        }
        let j = j.ok_or_else(bad)?;
        (&s[i..j], j + closer.len())
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(bad());
    }
    Ok((label(raw, s, open)?, end))
}

/// A node mention: an id and the shape and label it defines, if any.
struct NodeRef<'a> {
    id: &'a str,
    def: Option<(Shape, String)>,
}

fn is_id_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Reads `id [shape]` at `*pos`; `first` marks the chain's first node.
fn node_ref<'a>(s: &'a str, pos: &mut usize, first: bool) -> Result<NodeRef<'a>, Why> {
    let start = *pos;
    let len = s.as_bytes()[start..]
        .iter()
        .take_while(|b| is_id_byte(**b))
        .count();
    if len == 0 {
        return Err(syntax(s, start));
    }
    let id = &s[start..start + len];
    if first && matches!(id, "graph" | "flowchart" | "subgraph") {
        return Err(syntax(s, 0));
    }
    let p = start + len;
    *pos = p;
    let rest = &s[p..];
    if rest.starts_with(":::") {
        return Err(Why::Styling {
            keyword: ":::".into(),
        });
    }
    if rest.starts_with("@{") {
        return Err(syntax(s, p));
    }
    let (shape, open_len, closer) = if rest.starts_with("([") {
        (Shape::Stadium, 2, "])")
    } else if rest.starts_with("[[") {
        (Shape::Subroutine, 2, "]]")
    } else if ["((", "[(", "[/", "[\\", "{{", ">"]
        .iter()
        .any(|o| rest.starts_with(o))
    {
        return Err(syntax(s, p));
    } else if rest.starts_with('[') {
        (Shape::Rect, 1, "]")
    } else if rest.starts_with('(') {
        (Shape::Round, 1, ")")
    } else if rest.starts_with('{') {
        (Shape::Rhombus, 1, "}")
    } else {
        return Ok(NodeRef { id, def: None });
    };
    let (text, end) = shape_text(s, p, open_len, closer)?;
    *pos = end;
    Ok(NodeRef {
        id,
        def: Some((shape, text)),
    })
}

/// A link between two node mentions.
struct Link {
    kind: EdgeKind,
    label: Option<String>,
}

/// The inline form `lead` text `terminator`: the label's end, the terminator, its length.
fn inline(
    r: &str,
    lead: usize,
    terminators: &[(&str, EdgeKind)],
) -> Option<(usize, EdgeKind, usize)> {
    for (off, c) in r[lead..].char_indices() {
        if c.is_whitespace() {
            let after = &r[lead + off + c.len_utf8()..];
            for &(t, kind) in terminators {
                if after.starts_with(t) {
                    return Some((lead + off, kind, c.len_utf8() + t.len()));
                }
            }
        }
    }
    None
}

/// Reads a link at `*pos`, and its pipe label.
fn link(s: &str, pos: &mut usize) -> Result<Link, Why> {
    let start = *pos;
    let r = &s[start..];
    let plain = if r.starts_with("-->") {
        Some((EdgeKind::Arrow, 3, "->"))
    } else if r.starts_with("---") {
        Some((EdgeKind::Open, 3, "->"))
    } else if r.starts_with("-.->") {
        Some((EdgeKind::Dotted, 4, ""))
    } else if r.starts_with("==>") {
        Some((EdgeKind::Thick, 3, "=>"))
    } else {
        None
    };
    if let Some((kind, len, not_next)) = plain {
        // The next char must not extend the arrow: `--->` and `==>>` are not links.
        if r[len..]
            .chars()
            .next()
            .is_some_and(|c| not_next.contains(c))
        {
            return Err(syntax(s, start));
        }
        let mut i = skip_spaces(s, start + len);
        let mut text = None;
        if s[i..].starts_with('|') {
            let bar = i;
            let close = s[bar + 1..].find('|').ok_or_else(|| syntax(s, bar))?;
            let raw = s[bar + 1..bar + 1 + close].trim();
            if raw.is_empty() {
                return Err(syntax(s, bar));
            }
            text = Some(label(raw, s, bar)?);
            i = bar + close + 2;
        }
        *pos = i;
        return Ok(Link { kind, label: text });
    }
    let found = if r.starts_with("--") && r[2..].starts_with(char::is_whitespace) {
        inline(r, 2, &[("-->", EdgeKind::Arrow), ("---", EdgeKind::Open)])
    } else if r.starts_with("-.") && r[2..].starts_with(char::is_whitespace) {
        inline(r, 2, &[(".->", EdgeKind::Dotted)])
    } else if r.starts_with("==") && r[2..].starts_with(char::is_whitespace) {
        inline(r, 2, &[("==>", EdgeKind::Thick)])
    } else {
        None
    };
    let Some((end, kind, term)) = found else {
        return Err(syntax(s, start));
    };
    let raw = r[2..end].trim();
    if raw.is_empty() {
        return Err(syntax(s, start));
    }
    let text = label(raw, s, start)?;
    *pos = start + end + term;
    Ok(Link {
        kind,
        label: Some(text),
    })
}

/// Reads a whole chain: `node-ref (ws* link ws* node-ref)*`.
fn chain(s: &str) -> Result<(Vec<NodeRef<'_>>, Vec<Link>), Why> {
    let mut pos = 0;
    let mut refs = vec![node_ref(s, &mut pos, true)?];
    let mut links = Vec::new();
    loop {
        pos = skip_ws(s, pos);
        if pos == s.len() {
            return Ok((refs, links));
        }
        links.push(link(s, &mut pos)?);
        pos = skip_ws(s, pos);
        refs.push(node_ref(s, &mut pos, false)?);
    }
}

fn skip_ws(s: &str, pos: usize) -> usize {
    pos + (s.len() - pos) - s[pos..].trim_start().len()
}

/// The reader's state: everything read so far.
#[derive(Default)]
struct State {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    subgraphs: Vec<Subgraph>,
    /// Whether the node at the same index has been given a shape and label.
    defined: Vec<bool>,
    node_ix: BTreeMap<String, usize>,
    sub_ix: BTreeMap<String, usize>,
    /// The subgraphs open now, outermost first.
    open: Vec<usize>,
}

impl State {
    /// Reads one source line (or the header's tail).
    fn line(&mut self, text: &str, n: usize) -> Result<(), Unsupported> {
        if text.trim_start().starts_with("%%") {
            return Ok(());
        }
        for stmt in split_statements(text) {
            let stmt = stmt.trim();
            if !stmt.is_empty() {
                self.statement(stmt, n).map_err(|why| at(n, why))?;
            }
        }
        Ok(())
    }

    fn statement(&mut self, stmt: &str, n: usize) -> Result<(), Why> {
        let word = stmt.split(char::is_whitespace).next().unwrap_or("");
        let acc = stmt
            .split(|c: char| c.is_whitespace() || c == ':')
            .next()
            .unwrap_or("");
        if matches!(acc, "accTitle" | "accDescr") {
            return Err(Why::Statement {
                keyword: acc.into(),
            });
        }
        match word {
            "click" => Err(Why::Click),
            "style" | "classDef" | "class" | "linkStyle" => Err(Why::Styling {
                keyword: word.into(),
            }),
            "direction" => Err(Why::Statement {
                keyword: word.into(),
            }),
            "subgraph" => self.open_subgraph(stmt, n),
            "end" if stmt == "end" => self.open.pop().map(|_| ()).ok_or(Why::StrayEnd),
            "end" => Err(syntax(stmt, 0)),
            _ => self.chain(stmt, n),
        }
    }

    fn chain(&mut self, stmt: &str, n: usize) -> Result<(), Why> {
        let (refs, links) = chain(stmt)?;
        let mut ix = Vec::with_capacity(refs.len());
        for r in &refs {
            ix.push(self.mention(r, !links.is_empty(), n)?);
        }
        for (i, l) in links.into_iter().enumerate() {
            if self.edges.len() == MAX_EDGES {
                return Err(Why::TooMany {
                    what: "edges",
                    cap: MAX_EDGES,
                });
            }
            self.edges.push(Edge {
                from: ix[i],
                to: ix[i + 1],
                kind: l.kind,
                label: l.label,
                line: n,
            });
        }
        Ok(())
    }

    /// Records a mention of a node and returns its index.
    fn mention(&mut self, r: &NodeRef<'_>, in_edge: bool, n: usize) -> Result<usize, Why> {
        if self.sub_ix.contains_key(r.id) {
            let id = r.id.to_string();
            return Err(if in_edge {
                Why::EdgeToSubgraph { id }
            } else {
                Why::IdConflict { id }
            });
        }
        let i = match self.node_ix.get(r.id) {
            Some(&i) => i,
            None => {
                if self.nodes.len() == MAX_NODES {
                    return Err(Why::TooMany {
                        what: "nodes",
                        cap: MAX_NODES,
                    });
                }
                self.nodes.push(Node {
                    id: r.id.to_string(),
                    label: r.id.to_string(),
                    shape: Shape::Rect,
                    subgraph: None,
                    line: n,
                });
                self.defined.push(false);
                self.node_ix.insert(r.id.to_string(), self.nodes.len() - 1);
                self.nodes.len() - 1
            }
        };
        if let Some((shape, text)) = &r.def {
            let node = &mut self.nodes[i];
            if self.defined[i] && (node.shape != *shape || node.label != *text) {
                return Err(Why::Redefined {
                    id: r.id.to_string(),
                });
            }
            self.defined[i] = true;
            node.shape = *shape;
            node.label = text.clone();
        }
        if let Some(&scope) = self.open.last() {
            match self.nodes[i].subgraph {
                None => self.nodes[i].subgraph = Some(scope),
                Some(cur) if cur == scope || self.contains(scope, cur) => {}
                Some(cur) if self.contains(cur, scope) => self.nodes[i].subgraph = Some(scope),
                Some(_) => {
                    return Err(Why::TwoSubgraphs {
                        id: r.id.to_string(),
                    })
                }
            }
        }
        Ok(i)
    }

    /// Whether subgraph `outer` contains subgraph `inner` (is one of its ancestors).
    fn contains(&self, outer: usize, inner: usize) -> bool {
        let mut cur = self.subgraphs[inner].parent;
        while let Some(p) = cur {
            if p == outer {
                return true;
            }
            cur = self.subgraphs[p].parent;
        }
        false
    }

    fn open_subgraph(&mut self, stmt: &str, n: usize) -> Result<(), Why> {
        let r = stmt["subgraph".len()..].trim();
        if r.is_empty() {
            return Err(syntax(stmt, 0));
        }
        let index = self.subgraphs.len();
        let (id, title) = subgraph_header(r, index)?;
        if self.sub_ix.contains_key(&id) {
            return Err(Why::SubgraphTwice { id });
        }
        if self.node_ix.contains_key(&id) {
            return Err(Why::IdConflict { id });
        }
        if self.open.len() == MAX_DEPTH {
            return Err(Why::TooMany {
                what: "levels of nesting",
                cap: MAX_DEPTH,
            });
        }
        if index == MAX_SUBGRAPHS {
            return Err(Why::TooMany {
                what: "subgraphs",
                cap: MAX_SUBGRAPHS,
            });
        }
        self.sub_ix.insert(id.clone(), index);
        self.subgraphs.push(Subgraph {
            id,
            title,
            parent: self.open.last().copied(),
            line: n,
        });
        self.open.push(index);
        Ok(())
    }
}

/// The id and title of a subgraph from the text after the keyword.
fn subgraph_header(r: &str, index: usize) -> Result<(String, String), Why> {
    let anonymous = format!("subgraph {index}");
    let id_len = r.bytes().take_while(|b| is_id_byte(*b)).count();
    if id_len > 0 {
        let id = &r[..id_len];
        if id_len == r.len() {
            return Ok((id.to_string(), label(r, r, 0)?));
        }
        let open = skip_spaces(r, id_len);
        if r[open..].starts_with('[') {
            // `id [text]`, the whole of R; any other shape of R falls through to a plain title.
            match shape_text(r, open, 1, "]") {
                Ok((title, end)) if end == r.len() => return Ok((id.to_string(), title)),
                Err(why @ (Why::HtmlLabel | Why::LabelTooLong { .. })) => return Err(why),
                _ => {}
            }
        }
    }
    if r.len() >= 2 && r.starts_with('"') && r.ends_with('"') && !r[1..r.len() - 1].contains('"') {
        let text = r[1..r.len() - 1].trim();
        let title = if text.is_empty() {
            String::new()
        } else {
            label(text, r, 0)?
        };
        return Ok((anonymous, title));
    }
    Ok((anonymous, label(r, r, 0)?))
}
