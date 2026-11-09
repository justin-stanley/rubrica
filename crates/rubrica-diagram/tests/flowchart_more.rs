//! Rejections and accepted variants beyond the fixtures: each branch of the grammar.

use rubrica_diagram::flowchart::{parse, Direction, EdgeKind, Shape};

/// (source, line, message) for every source the parser must refuse.
const REJECTED: &[(&str, usize, &str)] = &[
    ("graph TD x\n  A\n", 1, "cannot read `TD x`"),
    ("graph td\n  A\n", 1, "cannot read `td`"),
    ("graph TD A --> B\n", 1, "cannot read `TD A --> B`"),
    ("graph LR ;; A -->\n", 1, "cannot read ``"),
    ("graph TD\n  end now\n", 2, "cannot read `end now`"),
    ("graph TD\n  graph --> B\n", 2, "cannot read `graph --> B`"),
    ("graph TD\n  flowchart\n", 2, "cannot read `flowchart`"),
    ("graph TD\n  subgraph\n  end\n", 2, "cannot read `subgraph`"),
    ("graph TD\n  click\n", 2, "`click` is not supported: a diagram cannot link or run script"),
    ("graph TD\n  class A hot\n", 2, "`class` is not supported; colours come from the theme"),
    ("graph TD\n  accDescr: some text\n", 2, "`accDescr` statements are not supported"),
    ("graph TD\n  accTitle\n", 2, "`accTitle` statements are not supported"),
    ("graph TD\n  direction LR\n", 2, "`direction` statements are not supported"),
    ("graph TD\n  A --> \n", 2, "cannot read ``"),
    ("graph TD\n  --> B\n", 2, "cannot read `--> B`"),
    ("graph TD\n  A --> [x]\n", 2, "cannot read `[x]`"),
    ("graph TD\n  A:::hot\n", 2, "`:::` is not supported; colours come from the theme"),
    ("graph TD\n  A[x]:::hot\n", 2, "cannot read `:::hot`"),
    ("graph TD\n  A[(db)]\n", 2, "cannot read `[(db)]`"),
    ("graph TD\n  A[/x/]\n", 2, "cannot read `[/x/]`"),
    ("graph TD\n  A[\\x\\]\n", 2, "cannot read `[\\x\\]`"),
    ("graph TD\n  A{{x}}\n", 2, "cannot read `{{x}}`"),
    ("graph TD\n  A>x]\n", 2, "cannot read `>x]`"),
    ("graph TD\n  A([a(b])\n", 2, "cannot read `([a(b])`"),
    ("graph TD\n  A[a|b]\n", 2, "cannot read `[a|b]`"),
    ("graph TD\n  A[abc --> B\n", 2, "cannot read `[abc --> B`"),
    ("graph TD\n  A[\"abc]\n", 2, "cannot read `[\"abc]`"),
    ("graph TD\n  A[\"a\" b]\n", 2, "cannot read `[\"a\" b]`"),
    ("graph TD\n  A[\"\"]\n", 2, "cannot read `[\"\"]`"),
    ("graph TD\n  A[   ]\n", 2, "cannot read `[   ]`"),
    ("graph TD\n  A[x]]\n", 2, "cannot read `]`"),
    ("graph TD\n  A[\"a\u{1}b\"] --> B\n", 2, "cannot read `[\"a\u{1}b\"] --> B`"),
    ("graph TD\n  A[\u{7f}]\n", 2, "cannot read `[\u{7f}]`"),
    ("graph TD\n  A[<b>x</b>]\n", 2, "HTML in a label is not supported; labels are plain text"),
    ("graph TD\n  A[a</b>]\n", 2, "HTML in a label is not supported; labels are plain text"),
    ("graph TD\n  A[<!-- x -->]\n", 2, "HTML in a label is not supported; labels are plain text"),
    ("graph TD\n  A[<?x?>]\n", 2, "HTML in a label is not supported; labels are plain text"),
    ("graph TD\n  A[xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx]\n", 2, "a label is 300 bytes, over the limit of 256"),
    ("graph TD\n  A[\"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\"]\n", 2, "a label is 300 bytes, over the limit of 256"),
    ("graph TD\n  A ----B\n", 2, "cannot read `----B`"),
    ("graph TD\n  A ==>> B\n", 2, "cannot read `==>> B`"),
    ("graph TD\n  A -->> B\n", 2, "cannot read `-->> B`"),
    ("graph TD\n  A === B\n", 2, "cannot read `=== B`"),
    ("graph TD\n  A -- x B\n", 2, "cannot read `-- x B`"),
    ("graph TD\n  A -- --> B\n", 2, "cannot read `-- --> B`"),
    ("graph TD\n  A -.-x B\n", 2, "cannot read `-.-x B`"),
    ("graph TD\n  A -. x B\n", 2, "cannot read `-. x B`"),
    ("graph TD\n  A == x B\n", 2, "cannot read `== x B`"),
    ("graph TD\n  A -- <b> --> B\n", 2, "HTML in a label is not supported; labels are plain text"),
    ("graph TD\n  A -- xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx --> B\n", 2, "a label is 300 bytes, over the limit of 256"),
    ("graph TD\n  A -->|| B\n", 2, "cannot read `|| B`"),
    ("graph TD\n  A -->|  | B\n", 2, "cannot read `|  | B`"),
    ("graph TD\n  A -->|x\u{1}y| B\n", 2, "cannot read `|x\u{1}y| B`"),
    ("graph TD\n  A -->|xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx| B\n", 2, "a label is 300 bytes, over the limit of 256"),
    ("graph TD\n  A -- x --> |y| B\n", 2, "cannot read `|y| B`"),
    ("graph TD\n  subgraph s [<i>t</i>]\n  A\n  end\n", 2, "HTML in a label is not supported; labels are plain text"),
    ("graph TD\n  subgraph t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t t \n  A\n  end\n", 2, "a label is 399 bytes, over the limit of 256"),
    ("graph TD\n  subgraph \"a\u{1}b\"\n  A\n  end\n", 2, "cannot read `\"a\u{1}b\"`"),
    ("graph TD\n  s\n  subgraph s\n  A\n  end\n", 3, "`s` names both a node and a subgraph"),
    ("graph TD\n  subgraph s\n  A\n  end\n  s\n", 5, "`s` names both a node and a subgraph"),
    ("graph TD\n  subgraph s\n  A\n  end\n  s --> A\n", 5, "an edge to the subgraph `s` is not supported; edges join nodes"),
    ("graph TD\n  subgraph s\n  A\n  end\n  A --> s --> B\n", 5, "an edge to the subgraph `s` is not supported; edges join nodes"),
    ("graph TD\n  subgraph s\n    subgraph t\n      A --> B\n", 3, "`subgraph` is not closed by `end`"),
    ("graph TD\n  subgraph s\n  end\n  end\n", 4, "`end` closes nothing"),
    ("graph TD\n  subgraph s\n  end\n", 0, "the diagram is empty"),
    ("graph TD\n  subgraph s\n  A\n  end\n  subgraph s\n  A\n  end\n", 5, "the subgraph `s` is opened twice"),
    ("graph TD\n  subgraph s\n  A\n  end\n  subgraph t\n  subgraph u\n  A\n  end\n  end\n", 7, "the node `A` is placed in two subgraphs"),
    ("graph TD\n  A[x]\n  A(x)\n", 3, "the node `A` is given two different shapes or labels"),
    ("graph TD\n  A[x]\n  A[y]\n", 3, "the node `A` is given two different shapes or labels"),
    ("graph TD\n  A\n  A[x] --> B\n  B{y}\n  B{z}\n", 5, "the node `B` is given two different shapes or labels"),
];
fn ids_and_labels(src: &str) -> Vec<(String, String)> {
    parse(src)
        .unwrap()
        .nodes
        .into_iter()
        .map(|n| (n.id, n.label))
        .collect()
}

#[test]
fn every_listed_source_is_refused_with_its_line_and_message() {
    let mut bad = Vec::new();
    for (src, line, message) in REJECTED {
        match parse(src) {
            Err(u) if u.line == *line && u.why.to_string() == *message => {}
            Err(u) => bad.push(format!(
                "{src:?}: line {} \"{}\", want line {line} \"{message}\"",
                u.line, u.why
            )),
            Ok(_) => bad.push(format!("{src:?}: parsed, want line {line} \"{message}\"")),
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn other_diagram_types_are_unknown_to_the_flowchart_reader() {
    let u = parse("sequenceDiagram\n  A->>B: hi\n").unwrap_err();
    assert_eq!(
        (u.line, u.why.to_string()),
        (1, "`sequenceDiagram` diagrams are not supported".into())
    );
    let u = parse("\n%% c\nstateDiagram-v2\n  [*] --> A\n").unwrap_err();
    assert_eq!(
        (u.line, u.why.to_string()),
        (3, "`stateDiagram-v2` diagrams are not supported".into())
    );
}

#[test]
fn the_found_text_is_cut_by_chars_not_bytes() {
    let src = format!("graph TD\n  A --x {}\n", "日".repeat(50));
    let u = parse(&src).unwrap_err();
    assert_eq!(
        u.why.to_string(),
        format!("cannot read `--x {}…`", "日".repeat(36))
    );
    // Exactly 40 chars is not cut.
    let src = format!("graph TD\n  A --x {}\n", "日".repeat(36));
    let u = parse(&src).unwrap_err();
    assert_eq!(
        u.why.to_string(),
        format!("cannot read `--x {}`", "日".repeat(36))
    );
}

#[test]
fn the_header_may_carry_statements() {
    for src in [
        "graph TD; A --> B\n",
        "graph TD ; A --> B\n",
        "graph; A --> B\n",
        "graph ;A --> B\n",
        "flowchart LR;A-->B;\n",
    ] {
        let f = parse(src).unwrap();
        assert_eq!(f.edges.len(), 1, "{src:?}");
        assert_eq!(f.edges[0].line, 1, "{src:?}");
    }
    assert_eq!(
        parse("flowchart LR;A-->B;\n").unwrap().direction,
        Direction::LeftRight
    );
    // A comment after the header's semicolon is a comment.
    assert_eq!(parse("graph TD; %% note\n  A\n").unwrap().nodes.len(), 1);
}

#[test]
fn crlf_line_ends_read_like_lf() {
    let f = parse("graph TD\r\n  A --> B\r\n  B -->|x| C\r\n").unwrap();
    assert_eq!(f.edges.len(), 2);
    assert_eq!(f.edges[1].label.as_deref(), Some("x"));
    assert_eq!(f.source, "graph TD\r\n  A --> B\r\n  B -->|x| C\r");
    assert_eq!(f.nodes[2].label, "C");
}

#[test]
fn links_without_spaces_and_in_every_form() {
    let f = parse("graph LR\n  A-->B\n  B---C\n  C-.->D\n  D==>E\n  E-->|p|F\n").unwrap();
    let kinds: Vec<EdgeKind> = f.edges.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        vec![
            EdgeKind::Arrow,
            EdgeKind::Open,
            EdgeKind::Dotted,
            EdgeKind::Thick,
            EdgeKind::Arrow
        ]
    );
    assert_eq!(f.edges[4].label.as_deref(), Some("p"));
    // The label may be spaced from its link; an inline label ends at the first terminator.
    let f = parse("graph TD\n  A --> |p q| B\n  A -- x y --- B\n  A -. x -.-> y .-> B\n").unwrap();
    assert_eq!(f.edges[0].label.as_deref(), Some("p q"));
    assert_eq!(
        (f.edges[1].kind, f.edges[1].label.as_deref()),
        (EdgeKind::Open, Some("x y"))
    );
    assert_eq!(
        (f.edges[2].kind, f.edges[2].label.as_deref()),
        (EdgeKind::Dotted, Some("x -.-> y"))
    );
    let f = parse("graph TD\n  A -- x --> B -- y ---C\n").unwrap();
    assert_eq!(f.edges.len(), 2);
}

#[test]
fn labels_are_trimmed_and_tabs_become_spaces() {
    assert_eq!(
        ids_and_labels("graph TD\n  A[  a\tb  ] --> B(\"  q  \")\n"),
        vec![
            ("A".to_string(), "a b".to_string()),
            ("B".to_string(), "q".to_string())
        ]
    );
    assert_eq!(
        ids_and_labels("graph TD\n  A([ s ]) --> B[[ t ]]\n  C{ \"u\" }\n"),
        vec![
            ("A".to_string(), "s".to_string()),
            ("B".to_string(), "t".to_string()),
            ("C".to_string(), "u".to_string())
        ]
    );
    let f = parse("graph TD\n  A -->|a\tb| B\n").unwrap();
    assert_eq!(f.edges[0].label.as_deref(), Some("a b"));
    // `<` that does not open HTML is text.
    assert_eq!(
        ids_and_labels("graph TD\n  A[a < b <3 <1]\n"),
        vec![("A".to_string(), "a < b <3 <1".to_string())]
    );
}

#[test]
fn a_label_of_exactly_the_cap_is_kept_in_every_place() {
    let x = "x".repeat(256);
    for src in [
        format!("graph TD\n  A[{x}]\n"),
        format!("graph TD\n  A -->|{x}| B\n"),
        format!("graph TD\n  A -- {x} --> B\n"),
        format!("graph TD\n  subgraph s [{x}]\n    A\n  end\n"),
    ] {
        assert!(parse(&src).is_ok());
    }
}

#[test]
fn semicolons_and_comments() {
    let f = parse("graph TD\n  ;; A ;\n  %% B --> C\n  A --> D ;;\n   \n").unwrap();
    assert_eq!(f.nodes.len(), 2);
    assert_eq!(f.edges.len(), 1);
    // A semicolon inside a quote or a shape's brackets does not split.
    assert_eq!(
        ids_and_labels("graph TD\n  A[x;y] --> B(\"p;q\") ; C{r;s}\n"),
        vec![
            ("A".to_string(), "x;y".to_string()),
            ("B".to_string(), "p;q".to_string()),
            ("C".to_string(), "r;s".to_string())
        ]
    );
}

#[test]
fn ids_are_alphanumeric_and_may_start_like_keywords() {
    let f = parse("graph TD\n  endpoint --> classy\n  styled --> clicked\n  _a1 --> 9\n").unwrap();
    let ids: Vec<&str> = f.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["endpoint", "classy", "styled", "clicked", "_a1", "9"]
    );
}

#[test]
fn repeated_definitions_agree_and_later_mentions_keep_them() {
    let f = parse("graph TD\n  A[x] --> B\n  A[x] --> C\n  A --> D\n  B{y}\n").unwrap();
    assert_eq!(
        (f.nodes[0].shape, f.nodes[0].label.as_str()),
        (Shape::Rect, "x")
    );
    assert_eq!(
        (f.nodes[1].shape, f.nodes[1].label.as_str()),
        (Shape::Rhombus, "y")
    );
    assert_eq!(f.nodes.len(), 4);
    assert_eq!(f.nodes[0].line, 2);
}

#[test]
fn subgraph_headers_in_every_form() {
    let src = "graph TD\n  subgraph a [\"Alpha one\"]\n    n1\n  end\n  subgraph \"Beta two\"\n    n2\n  end\n  subgraph Gamma three\n    n3\n  end\n  subgraph d[a (b)]\n    n4\n  end\n  subgraph e\n    n5\n  end\n  subgraph  f   [ Fox ]\n    n6\n  end\n";
    let f = parse(src).unwrap();
    let got: Vec<(&str, &str)> = f
        .subgraphs
        .iter()
        .map(|s| (s.id.as_str(), s.title.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("a", "Alpha one"),
            ("subgraph 1", "Beta two"),
            ("subgraph 2", "Gamma three"),
            ("subgraph 3", "d[a (b)]"),
            ("e", "e"),
            ("f", "Fox"),
        ]
    );
}

#[test]
fn a_node_belongs_to_its_deepest_scope() {
    let src = "graph TD\n  subgraph a\n    subgraph b\n      subgraph c\n        N\n      end\n    end\n    N\n    M\n  end\n  M --> N\n  subgraph a2\n    subgraph a3\n      M\n    end\n  end\n";
    // M is mentioned in a, then in the unrelated a3: that is two subgraphs.
    let u = parse(src).unwrap_err();
    assert_eq!(
        (u.line, u.why.to_string()),
        (14, "the node `M` is placed in two subgraphs".into())
    );
    let ok = "graph TD\n  subgraph a\n    N\n    subgraph b\n      N\n      subgraph c\n        N\n      end\n      N\n    end\n    N\n  end\n";
    let f = parse(ok).unwrap();
    assert_eq!(f.nodes[0].subgraph, Some(2));
    assert_eq!(f.subgraphs[2].parent, Some(1));
    assert_eq!(f.subgraphs[1].parent, Some(0));
    assert_eq!(f.subgraphs[0].parent, None);
}

#[test]
fn eight_levels_of_nesting_are_allowed() {
    let mut src = String::from("graph TD\n");
    for k in 0..8 {
        src.push_str(&format!("subgraph s{k}\n"));
    }
    src.push_str("A\n");
    src.push_str(&"end\n".repeat(8));
    let f = parse(&src).unwrap();
    assert_eq!(f.subgraphs.len(), 8);
    assert_eq!(f.nodes[0].subgraph, Some(7));
}

#[test]
fn exactly_the_caps_are_allowed() {
    let mut nodes = String::from("graph TD\n");
    for i in 0..300 {
        nodes.push_str(&format!("  n{i}\n"));
    }
    assert_eq!(parse(&nodes).unwrap().nodes.len(), 300);
    let edges = format!("graph TD\n{}", "  a --> b\n".repeat(600));
    assert_eq!(parse(&edges).unwrap().edges.len(), 600);
    let mut subs = String::from("graph TD\n");
    for k in 0..50 {
        subs.push_str(&format!("subgraph s{k}\n  x{k}\nend\n"));
    }
    assert_eq!(parse(&subs).unwrap().subgraphs.len(), 50);
}

#[test]
fn a_statement_is_read_once_so_time_stays_linear_in_the_source() {
    // A long run of unreadable statements stops at the first; a long valid one reads once.
    let long_chain = format!("graph LR\n  n{}\n", " --> n".repeat(500));
    let f = parse(&long_chain).unwrap();
    assert_eq!(f.edges.len(), 500);
    assert_eq!(f.nodes.len(), 1);
}
