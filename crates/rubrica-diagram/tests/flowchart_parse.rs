//! The flowchart grammar: what parses, what each construct becomes, and every rejection with
//! its line and message.

use rubrica_diagram::flowchart::{
    parse, Direction, Edge, EdgeKind, Flowchart, Node, Shape, Subgraph,
};
use std::fs;
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures")).join(name)
}

/// The `.mmd` files of a fixture directory, sorted by name: (file name, text).
fn files(name: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = fs::read_dir(dir(name))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "mmd"))
        .map(|p| {
            (
                p.file_name().unwrap().to_str().unwrap().to_string(),
                fs::read_to_string(&p).unwrap(),
            )
        })
        .collect();
    out.sort();
    out
}

fn corpus(file: &str) -> Flowchart {
    let text = fs::read_to_string(dir("flowchart").join(file)).unwrap();
    parse(&text).unwrap()
}

fn node(id: &str, label: &str, shape: Shape, subgraph: Option<usize>, line: usize) -> Node {
    Node {
        id: id.into(),
        label: label.into(),
        shape,
        subgraph,
        line,
    }
}

fn ids(f: &Flowchart) -> Vec<&str> {
    f.nodes.iter().map(|n| n.id.as_str()).collect()
}

#[test]
fn a_two_node_graph_parses() {
    let want = Flowchart {
        direction: Direction::TopDown,
        title: "graph TD".into(),
        source: "graph TD\n  A --> B".into(),
        nodes: vec![
            node("A", "A", Shape::Rect, None, 2),
            node("B", "B", Shape::Rect, None, 2),
        ],
        edges: vec![Edge {
            from: 0,
            to: 1,
            kind: EdgeKind::Arrow,
            label: None,
            line: 2,
        }],
        subgraphs: vec![],
    };
    assert_eq!(corpus("01-two-nodes.mmd"), want);
}

#[test]
fn every_direction_keyword_parses() {
    let cases = [
        ("graph TD", Direction::TopDown),
        ("graph TB", Direction::TopDown),
        ("graph BT", Direction::BottomUp),
        ("graph LR", Direction::LeftRight),
        ("graph RL", Direction::RightLeft),
        ("graph", Direction::TopDown),
        ("flowchart LR", Direction::LeftRight),
    ];
    for (header, want) in cases {
        let f = parse(&format!("{header}\n  A --> B\n")).unwrap();
        assert_eq!(f.direction, want, "{header}");
    }
}

#[test]
fn the_five_shapes_parse_with_their_labels() {
    let f = corpus("03-shapes.mmd");
    let got: Vec<(&str, Shape, &str)> = f
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.shape, n.label.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("r", Shape::Rect, "Rectangle"),
            ("o", Shape::Round, "Rounded"),
            ("d", Shape::Rhombus, "Decision"),
            ("s", Shape::Stadium, "Stadium"),
            ("u", Shape::Subroutine, "Subroutine"),
        ]
    );
}

#[test]
fn the_four_edge_kinds_parse() {
    let f = corpus("04-edge-kinds.mmd");
    let kinds: Vec<EdgeKind> = f.edges.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        vec![
            EdgeKind::Arrow,
            EdgeKind::Open,
            EdgeKind::Dotted,
            EdgeKind::Thick
        ]
    );
}

#[test]
fn pipe_and_inline_labels_parse() {
    let pipes = corpus("05-pipe-labels.mmd");
    let got: Vec<Option<&str>> = pipes.edges.iter().map(|e| e.label.as_deref()).collect();
    assert_eq!(
        got,
        vec![Some("yes"), Some("no"), Some("later"), Some("now")]
    );
    let inline = corpus("06-inline-labels.mmd");
    let got: Vec<(Option<&str>, EdgeKind)> = inline
        .edges
        .iter()
        .map(|e| (e.label.as_deref(), e.kind))
        .collect();
    assert_eq!(
        got,
        vec![
            (Some("text"), EdgeKind::Arrow),
            (Some("maybe"), EdgeKind::Dotted),
            (Some("sure"), EdgeKind::Thick),
            (Some("plain"), EdgeKind::Open),
        ]
    );
}

#[test]
fn chains_and_semicolons_make_one_edge_per_link() {
    let chain = corpus("02-chain-lr.mmd");
    let got: Vec<(usize, usize, usize)> =
        chain.edges.iter().map(|e| (e.from, e.to, e.line)).collect();
    assert_eq!(got, vec![(0, 1, 2), (1, 2, 2), (2, 3, 2)]);
    let semi = corpus("19-semicolons-comments.mmd");
    assert_eq!(ids(&semi), vec!["A", "B", "C", "D"]);
    let got: Vec<(usize, usize, usize)> =
        semi.edges.iter().map(|e| (e.from, e.to, e.line)).collect();
    assert_eq!(got, vec![(0, 1, 3), (1, 2, 3), (2, 3, 4)]);
}

#[test]
fn subgraphs_nest_and_own_their_nodes() {
    let f = corpus("15-subgraphs.mmd");
    let sg = |id: &str, title: &str, parent, line| Subgraph {
        id: id.into(),
        title: title.into(),
        parent,
        line,
    };
    assert_eq!(
        f.subgraphs,
        vec![sg("one", "First group", None, 2), sg("two", "two", None, 5)]
    );
    let owner: Vec<(&str, Option<usize>)> = f
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.subgraph))
        .collect();
    assert_eq!(
        owner,
        vec![
            ("a1", Some(0)),
            ("a2", Some(0)),
            ("b1", Some(1)),
            ("b2", Some(1)),
            ("c", None)
        ]
    );

    let f = corpus("16-nested-subgraphs.mmd");
    assert_eq!(
        f.subgraphs,
        vec![
            sg("outer", "Outer", None, 2),
            sg("inner", "Inner", Some(0), 3)
        ]
    );
    let owner: Vec<(&str, Option<usize>)> = f
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.subgraph))
        .collect();
    assert_eq!(
        owner,
        vec![
            ("x", Some(1)),
            ("y", Some(1)),
            ("z", Some(0)),
            ("w", None),
            ("v", None)
        ]
    );

    // A node mentioned outside and then inside a subgraph belongs to it.
    let f = parse("graph TD\n  A --> B\n  subgraph s\n    B\n  end\n").unwrap();
    assert_eq!(f.nodes[0].subgraph, None);
    assert_eq!(f.nodes[1].subgraph, Some(0));
}

#[test]
fn quoted_labels_keep_brackets_and_semicolons() {
    let f = corpus("18-quoted-labels.mmd");
    let labels: Vec<&str> = f.nodes.iter().map(|n| n.label.as_str()).collect();
    assert_eq!(labels, vec!["x < y & z", "5 > 3", "a;b (c) [d]"]);
    assert_eq!(f.edges.len(), 2);
}

#[test]
fn front_matter_sets_the_title() {
    assert_eq!(corpus("20-front-matter-title.mmd").title, "Release flow");
    assert_eq!(corpus("01-two-nodes.mmd").title, "graph TD");
}

#[test]
fn the_whole_corpus_parses() {
    let all = files("flowchart");
    assert_eq!(all.len(), 20, "the corpus has 20 files");
    let errors: Vec<String> = all
        .iter()
        .filter_map(|(name, text)| parse(text).err().map(|u| format!("{name}: {u:?}")))
        .collect();
    assert!(
        errors.is_empty(),
        "the corpus: {} files, {} errors (first: {})",
        all.len(),
        errors.len(),
        errors[0]
    );
}

#[test]
fn every_rejection_has_its_line_and_message() {
    let tsv = fs::read_to_string(dir("rejected").join("expected.tsv")).unwrap();
    let mut listed = Vec::new();
    let mut bad = Vec::new();
    for row in tsv.lines() {
        let mut cols = row.splitn(3, '\t');
        let (file, line, message) = (
            cols.next().unwrap(),
            cols.next().unwrap().parse::<usize>().unwrap(),
            cols.next().unwrap(),
        );
        listed.push(file.to_string());
        let text = fs::read_to_string(dir("rejected").join(file)).unwrap();
        match parse(&text) {
            Err(u) if u.line == line && u.why.to_string() == message => {}
            Err(u) => bad.push(format!(
                "{file}: line {} \"{}\", want line {line} \"{message}\"",
                u.line, u.why
            )),
            Ok(_) => bad.push(format!("{file}: parsed, want line {line} \"{message}\"")),
        }
    }
    let on_disk: Vec<String> = files("rejected").into_iter().map(|(n, _)| n).collect();
    assert_eq!(listed, on_disk, "expected.tsv lists every file, sorted");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// The first mismatch of `src` against an expected rejection, as text.
fn expect_err(name: &str, src: &str, line: usize, message: &str, bad: &mut Vec<String>) {
    match parse(src) {
        Err(u) if u.line == line && u.why.to_string() == message => {}
        Err(u) => bad.push(format!(
            "{name}: line {} \"{}\", want line {line} \"{message}\"",
            u.line, u.why
        )),
        Ok(_) => bad.push(format!("{name}: parsed, want line {line} \"{message}\"")),
    }
}

/// A source of exactly `total` bytes: the header, comment lines, and `tail`.
fn padded(total: usize, tail: &str) -> String {
    let mut s = String::from("graph TD\n");
    let budget = total - tail.len();
    while s.len() + 5 <= budget {
        s.push_str("%% x\n");
    }
    if s.len() < budget {
        // The last comment takes the remainder, ending in a newline.
        let pad = budget - s.len();
        s.push_str("%%");
        s.push_str(&"x".repeat(pad - 3));
        s.push('\n');
    }
    s.push_str(tail);
    assert_eq!(s.len(), total);
    s
}

#[test]
fn the_caps_are_enforced_at_their_line() {
    let mut bad = Vec::new();

    let over = padded(32_769, "");
    expect_err(
        "source cap",
        &over,
        0,
        "the diagram source is 32769 bytes, over the limit of 32768",
        &mut bad,
    );
    let at = padded(32_768, "  A\n");
    match parse(&at) {
        Ok(f) => {
            if f.nodes.len() != 1 {
                bad.push("at the source cap: not one node".into());
            }
        }
        Err(u) => bad.push(format!("at the source cap: {u:?}")),
    }

    let mut nodes = String::from("graph TD\n");
    for i in 0..301 {
        nodes.push_str(&format!("  n{i}\n"));
    }
    expect_err(
        "node cap",
        &nodes,
        302,
        "the diagram has more than 300 nodes",
        &mut bad,
    );

    let edges = format!("graph TD\n{}", "  a --> b\n".repeat(601));
    expect_err(
        "edge cap",
        &edges,
        602,
        "the diagram has more than 600 edges",
        &mut bad,
    );

    let mut depth = String::from("graph TD\n");
    for k in 1..=9 {
        depth.push_str(&format!("subgraph s{k}\n"));
    }
    depth.push_str("A\n");
    depth.push_str(&"end\n".repeat(9));
    expect_err(
        "depth cap",
        &depth,
        10,
        "the diagram has more than 8 levels of nesting",
        &mut bad,
    );

    let mut subs = String::from("graph TD\n");
    for k in 1..=51 {
        subs.push_str(&format!("subgraph s{k}\n  x{k}\nend\n"));
    }
    expect_err(
        "subgraph cap",
        &subs,
        152,
        "the diagram has more than 50 subgraphs",
        &mut bad,
    );

    let long = format!("graph TD\n  A[{}]\n", "x".repeat(257));
    expect_err(
        "label cap",
        &long,
        2,
        "a label is 257 bytes, over the limit of 256",
        &mut bad,
    );
    match parse(&format!("graph TD\n  A[{}]\n", "x".repeat(256))) {
        Ok(f) if f.nodes[0].label.len() == 256 => {}
        other => bad.push(format!("at the label cap: {other:?}")),
    }

    expect_err(
        "control character",
        "graph TD\n  A[a\u{1}b] --> B\n",
        2,
        "cannot read `[a\u{1}b] --> B`",
        &mut bad,
    );

    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn no_prefix_or_deletion_panics() {
    let mut texts: Vec<(String, String)> = files("flowchart");
    texts.extend(files("rejected"));
    for (name, text) in &texts {
        let run = |cut: &str, s: &str| {
            let owned = s.to_string();
            if std::panic::catch_unwind(move || {
                let _ = parse(&owned);
            })
            .is_err()
            {
                panic!("{name}: the parser panicked on {cut}");
            }
        };
        for end in 0..=text.len() {
            if text.is_char_boundary(end) {
                run(&format!("the prefix of {end} bytes"), &text[..end]);
            }
        }
        for at in 0..text.len() {
            let mut bytes = text.as_bytes().to_vec();
            bytes.remove(at);
            if let Ok(s) = String::from_utf8(bytes) {
                run(&format!("the deletion of byte {at}"), &s);
            }
        }
    }
}
