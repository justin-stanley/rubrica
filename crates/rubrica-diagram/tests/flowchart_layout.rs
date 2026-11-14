//! The flowchart layout, read back from the SVG: the two-node drawing exactly, the four
//! orientations as one layout mapped, and the geometric promises over the corpus (no overlaps,
//! edges that touch their nodes, back edges, self-loops, clusters) and the cell cap.

use rubrica_diagram::flowchart::{parse, Flowchart};
use rubrica_diagram::{render, text_width, Unsupported, Why, CAP_HEIGHT};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const PADDING: i64 = 1600;
const CLUSTER_PAD: i64 = 1600;
const TITLE_H: i64 = 2000;
const ARROW_LEN: i64 = 900;
const LOOP_STEP: i64 = 800;

/// A box in centipixels: x, y, width, height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Bx {
    x: i64,
    y: i64,
    w: i64,
    h: i64,
}

impl Bx {
    fn right(&self) -> i64 {
        self.x + self.w
    }
    fn bottom(&self) -> i64 {
        self.y + self.h
    }
    /// Whether the interiors meet (touching edges do not count).
    fn overlaps(&self, o: &Bx) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }
    /// Whether `p` lies on the border.
    fn on_border(&self, p: Pt) -> bool {
        let in_x = self.x <= p.0 && p.0 <= self.right();
        let in_y = self.y <= p.1 && p.1 <= self.bottom();
        (in_x && (p.1 == self.y || p.1 == self.bottom()))
            || (in_y && (p.0 == self.x || p.0 == self.right()))
    }
    fn centre(&self) -> Pt {
        (self.x + self.w / 2, self.y + self.h / 2)
    }
}

type Pt = (i64, i64);

/// One edge group: the line's points and the arrowhead's, if drawn.
#[derive(Debug)]
struct EdgeG {
    points: Vec<Pt>,
    arrow: Option<Vec<Pt>>,
}

/// What a flowchart SVG draws, read back in document order.
#[derive(Debug, Default)]
struct Drawing {
    w: i64,
    h: i64,
    clusters: Vec<Bx>,
    edges: Vec<EdgeG>,
    nodes: Vec<Bx>,
    labels: Vec<Bx>,
}

/// `12.34` as centipixels.
fn cpx(s: &str) -> i64 {
    let (a, b) = s
        .split_once('.')
        .unwrap_or_else(|| panic!("not a number: {s}"));
    assert_eq!(b.len(), 2, "not two decimals: {s}");
    a.parse::<i64>().unwrap() * 100 + b.parse::<i64>().unwrap()
}

/// The numbers of a path's `d`, as points (`H` and `V` are not used by the flowchart).
fn path_points(d: &str) -> Vec<Pt> {
    let mut nums = Vec::new();
    for word in d.split(' ') {
        let w = word.trim_start_matches(['M', 'L']);
        if !w.is_empty() && w != "Z" {
            nums.push(cpx(w));
        }
    }
    nums.chunks(2).map(|c| (c[0], c[1])).collect()
}

fn bbox(points: &[Pt]) -> Bx {
    let x0 = points.iter().map(|p| p.0).min().unwrap();
    let x1 = points.iter().map(|p| p.0).max().unwrap();
    let y0 = points.iter().map(|p| p.1).min().unwrap();
    let y1 = points.iter().map(|p| p.1).max().unwrap();
    Bx {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> &'a str {
    attrs
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
        .unwrap_or_else(|| panic!("no {name} in {attrs:?}"))
}

fn rect(attrs: &[(String, String)]) -> Bx {
    Bx {
        x: cpx(attr(attrs, "x")),
        y: cpx(attr(attrs, "y")),
        w: cpx(attr(attrs, "width")),
        h: cpx(attr(attrs, "height")),
    }
}

/// Reads the drawing back from Rubrica's own SVG (no whitespace between tags, attribute
/// values in double quotes, text escaped).
fn read(svg: &str) -> Drawing {
    let mut d = Drawing::default();
    let mut section = String::new();
    let mut depth = 0usize;
    for piece in svg.split('<').skip(1) {
        let (tag, _text) = piece.split_once('>').expect("a tag ends with >");
        if let Some(_name) = tag.strip_prefix('/') {
            depth -= 1;
            continue;
        }
        let name = tag.split(' ').next().unwrap();
        let mut attrs = Vec::new();
        let mut rest = &tag[name.len()..];
        while let Some((k, v)) = rest.split_once("=\"") {
            let (val, after) = v.split_once('"').unwrap();
            attrs.push((k.trim().to_string(), val.to_string()));
            rest = after;
        }
        depth += 1;
        let class = attrs
            .iter()
            .find(|(k, _)| k == "class")
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        match (name, depth) {
            ("svg", _) => {
                d.w = cpx(attr(&attrs, "width"));
                d.h = cpx(attr(&attrs, "height"));
            }
            ("g", 2) => section = class,
            ("g", 3) if section == "rd-edges" => d.edges.push(EdgeG {
                points: Vec::new(),
                arrow: None,
            }),
            ("rect", _) if section == "rd-clusters" => d.clusters.push(rect(&attrs)),
            ("rect", _) if section == "rd-labels" => d.labels.push(rect(&attrs)),
            ("rect", _) if section == "rd-nodes" && class == "rd-shape" => {
                d.nodes.push(rect(&attrs))
            }
            ("path", _) if section == "rd-nodes" && class == "rd-shape" => {
                d.nodes.push(bbox(&path_points(attr(&attrs, "d"))))
            }
            ("path", _) if class == "rd-line" => {
                d.edges.last_mut().unwrap().points = path_points(attr(&attrs, "d"))
            }
            ("path", _) if class == "rd-arrow" => {
                d.edges.last_mut().unwrap().arrow = Some(path_points(attr(&attrs, "d")))
            }
            _ => {}
        }
    }
    d
}

fn dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/flowchart"
    ))
}

/// The corpus: (file name, source), sorted by name.
fn corpus() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = fs::read_dir(dir())
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

fn source(file: &str) -> String {
    fs::read_to_string(dir().join(file)).unwrap()
}

/// Two decimals, as the SVG writes a number.
fn fmt(n: i64) -> String {
    format!("{}.{:02}", n / 100, n % 100)
}

fn tw(s: &str) -> i64 {
    (text_width(s) as i64 * 1400 + 500).div_euclid(1000)
}

fn cap() -> i64 {
    (CAP_HEIGHT as i64 * 1400 + 500).div_euclid(1000)
}

#[test]
fn two_nodes_top_down_is_exactly_this_svg() {
    // Both nodes are NODE_MIN_W wide: their text is narrower than the minimum less padding.
    assert!(tw("A") + 2400 < 4000);
    assert!(tw("B") + 2400 < 4000);
    let text_y = |top: i64| fmt(top + 1650 + cap().div_euclid(2));
    let want = [
        "<svg role=\"img\" viewBox=\"0 0 72.00 138.00\" width=\"72.00\" height=\"138.00\" ",
        "class=\"rd rd-flowchart\" font-family=\"Inter, sans-serif\" font-size=\"14\">",
        "<title>graph TD</title><desc>graph TD\n  A --&gt; B</desc>",
        "<g class=\"rd-clusters\"></g>",
        "<g class=\"rd-edges\"><g class=\"rd-edge rd-edge-solid\">",
        "<path class=\"rd-line\" d=\"M36.00 49.00 L36.00 59.00 L36.00 70.00 L36.00 80.00\"></path>",
        "<path class=\"rd-arrow\" d=\"M36.00 89.00 L31.50 80.00 L40.50 80.00 Z\"></path>",
        "</g></g>",
        "<g class=\"rd-nodes\"><g class=\"rd-node\">",
        "<rect class=\"rd-shape\" x=\"16.00\" y=\"16.00\" width=\"40.00\" height=\"33.00\"></rect>",
        &format!(
            "<text class=\"rd-text\" x=\"36.00\" y=\"{}\" text-anchor=\"middle\">A</text>",
            text_y(1600)
        ),
        "</g><g class=\"rd-node\">",
        "<rect class=\"rd-shape\" x=\"16.00\" y=\"89.00\" width=\"40.00\" height=\"33.00\"></rect>",
        &format!(
            "<text class=\"rd-text\" x=\"36.00\" y=\"{}\" text-anchor=\"middle\">B</text>",
            text_y(8900)
        ),
        "</g></g>",
        "<g class=\"rd-labels\"></g>",
        "</svg>",
    ]
    .concat();
    assert_eq!(text_y(1600), fmt(3250 + cap().div_euclid(2)));
    assert_eq!(render(&source("01-two-nodes.mmd")).unwrap(), want);
}

#[test]
fn the_four_orientations_are_one_layout_mapped() {
    let draw = |dir: &str| read(&render(&format!("graph {dir}\n  A --> B --> C\n")).unwrap());
    let td = draw("TD");
    let bt = draw("BT");
    let lr = draw("LR");
    let rl = draw("RL");
    // TD: one column, top to bottom.
    assert!(td.nodes[0].bottom() < td.nodes[1].y && td.nodes[1].bottom() < td.nodes[2].y);
    assert_eq!(td.nodes[0].centre().0, td.nodes[2].centre().0);
    // BT is TD mirrored top to bottom: the same x, C on top.
    assert_eq!((bt.w, bt.h), (td.w, td.h));
    for (t, b) in td.nodes.iter().zip(&bt.nodes) {
        assert_eq!((b.x, b.w, b.h), (t.x, t.w, t.h));
        assert_eq!(b.y, td.h - t.y - t.h);
    }
    assert!(bt.nodes[2].bottom() < bt.nodes[1].y && bt.nodes[1].bottom() < bt.nodes[0].y);
    // LR: one row, centres ascending in x, one centre y.
    let c: Vec<Pt> = lr.nodes.iter().map(Bx::centre).collect();
    assert!(c[0].0 < c[1].0 && c[1].0 < c[2].0, "{c:?}");
    assert!(c[0].1 == c[1].1 && c[1].1 == c[2].1, "{c:?}");
    assert!(lr.nodes[0].right() < lr.nodes[1].x && lr.nodes[1].right() < lr.nodes[2].x);
    // RL is LR mirrored left to right.
    assert_eq!((rl.w, rl.h), (lr.w, lr.h));
    for (l, r) in lr.nodes.iter().zip(&rl.nodes) {
        assert_eq!((r.y, r.w, r.h), (l.y, l.w, l.h));
        assert_eq!(r.x, lr.w - l.x - l.w);
    }
    let c: Vec<Pt> = rl.nodes.iter().map(Bx::centre).collect();
    assert!(c[0].0 > c[1].0 && c[1].0 > c[2].0, "{c:?}");
    // The edges are mapped with the nodes.
    for (t, b) in td.edges.iter().zip(&bt.edges) {
        let mirrored: Vec<Pt> = t.points.iter().map(|p| (p.0, td.h - p.1)).collect();
        assert_eq!(b.points, mirrored);
    }
    for (l, r) in lr.edges.iter().zip(&rl.edges) {
        let mirrored: Vec<Pt> = l.points.iter().map(|p| (lr.w - p.0, p.1)).collect();
        assert_eq!(r.points, mirrored);
    }
}

/// Whether `p` is on `bx`'s border, or is the base of an arrowhead whose tip is on it.
fn touches(p: Pt, bx: &Bx, arrow: &Option<Vec<Pt>>) -> bool {
    bx.on_border(p)
        || arrow.as_ref().is_some_and(|a| {
            let tip = a[0];
            bx.on_border(tip)
                && ((p.0 == tip.0 && (p.1 - tip.1).abs() == ARROW_LEN)
                    || (p.1 == tip.1 && (p.0 - tip.0).abs() == ARROW_LEN))
        })
}

#[test]
fn boxes_never_overlap_and_edges_touch_their_ends() {
    for (file, src) in corpus() {
        let f: Flowchart = parse(&src).unwrap();
        let d = read(&render(&src).unwrap());
        assert_eq!(d.nodes.len(), f.nodes.len(), "{file}");
        assert_eq!(d.edges.len(), f.edges.len(), "{file}");
        for (i, a) in d.nodes.iter().enumerate() {
            for b in &d.nodes[i + 1..] {
                assert!(!a.overlaps(b), "{file}: nodes {a:?} and {b:?} overlap");
            }
            for l in &d.labels {
                assert!(!a.overlaps(l), "{file}: node {a:?} and label {l:?} overlap");
            }
        }
        for (i, (e, g)) in f.edges.iter().zip(&d.edges).enumerate() {
            let (from, to) = (&d.nodes[e.from], &d.nodes[e.to]);
            let first = g.points[0];
            let last = *g.points.last().unwrap();
            let forward = touches(first, from, &g.arrow) && touches(last, to, &g.arrow);
            let backward = touches(first, to, &g.arrow) && touches(last, from, &g.arrow);
            assert!(
                forward || backward,
                "{file}: edge {i} {:?} does not join {from:?} and {to:?}",
                g.points
            );
            // No point is repeated (the stubs and the dummies' points are joined by straight
            // lines, which may be diagonal).
            for w in g.points.windows(2) {
                assert_ne!(w[0], w[1], "{file}: edge {i} repeats a point");
            }
        }
        // Everything is inside the drawing, with the padding.
        let all = d.nodes.iter().chain(&d.labels).chain(&d.clusters);
        for b in all {
            assert!(b.x >= PADDING && b.y >= PADDING, "{file}: {b:?}");
            assert!(b.right() <= d.w - PADDING && b.bottom() <= d.h - PADDING);
        }
    }
}

#[test]
fn a_back_edge_points_at_its_original_target() {
    let src = source("09-cycle.mmd");
    let f = parse(&src).unwrap();
    let (back, e) = f
        .edges
        .iter()
        .enumerate()
        .find(|(_, e)| f.nodes[e.from].id == "C" && f.nodes[e.to].id == "A")
        .unwrap();
    let d = read(&render(&src).unwrap());
    let a = d.nodes[e.to];
    let c = d.nodes[e.from];
    assert!(c.y > a.bottom(), "C is drawn below A: {c:?} {a:?}");
    let tip = d.edges[back].arrow.as_ref().expect("an arrowhead")[0];
    // The tip is on A's bottom side, and the head points up into A.
    assert_eq!(tip.1, a.bottom(), "{tip:?} {a:?}");
    assert!(a.x <= tip.0 && tip.0 <= a.right(), "{tip:?} {a:?}");
    let base = &d.edges[back].arrow.as_ref().unwrap()[1..];
    assert!(base.iter().all(|p| p.1 == a.bottom() + ARROW_LEN));
    // No arrowhead touches C.
    assert!(!c.on_border(tip));
}

#[test]
fn self_loops_sit_beside_their_node_and_stack() {
    let src = source("10-self-loop.mmd");
    let f = parse(&src).unwrap();
    let d = read(&render(&src).unwrap());
    let a = d.nodes[0];
    let loops: Vec<usize> = (0..f.edges.len())
        .filter(|&i| f.edges[i].from == 0 && f.edges[i].to == 0)
        .collect();
    assert_eq!(loops, [0, 1]);
    let mut outs = Vec::new();
    for &i in &loops {
        let p = &d.edges[i].points;
        assert_eq!(
            p[0].0,
            a.right(),
            "loop {i} starts on A's right side: {p:?}"
        );
        assert!(a.y < p[0].1 && p[0].1 < a.bottom());
        assert!(
            p.iter().all(|q| q.0 >= a.right()),
            "loop {i} is right of A: {p:?}"
        );
        let tip = d.edges[i].arrow.as_ref().unwrap()[0];
        assert_eq!(tip.0, a.right());
        outs.push(p[1].0 - p[0].0);
    }
    assert_eq!(outs[1] - outs[0], LOOP_STEP);
    // The labelled loop is the second edge and the first label.
    assert!(f.edges[1].label.is_some() && f.edges[0].label.is_none());
    let label = d.labels[0];
    let loop_right = d.edges[1].points.iter().map(|p| p.0).max().unwrap();
    assert!(label.x > loop_right, "{label:?} right of {loop_right}");
    for n in &d.nodes {
        assert!(!label.overlaps(n), "{label:?} overlaps {n:?}");
    }
    for (i, e) in d.edges.iter().enumerate() {
        for w in e.points.windows(2) {
            let seg = bbox(w);
            let crosses = seg.x < label.right()
                && label.x < seg.right().max(seg.x + 1)
                && seg.y < label.bottom()
                && label.y < seg.bottom().max(seg.y + 1);
            assert!(!crosses, "edge {i} {w:?} crosses {label:?}");
        }
    }
}

/// The subgraphs `k` is in or under: `k` and its ancestors.
fn chain_of(f: &Flowchart, node: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut cur = f.nodes[node].subgraph;
    while let Some(k) = cur {
        out.push(k);
        cur = f.subgraphs[k].parent;
    }
    out
}

fn contains_padded(outer: &Bx, inner: &Bx) -> bool {
    inner.x >= outer.x + CLUSTER_PAD
        && inner.right() <= outer.right() - CLUSTER_PAD
        && inner.y >= outer.y + CLUSTER_PAD + TITLE_H
        && inner.bottom() <= outer.bottom() - CLUSTER_PAD
}

#[test]
fn clusters_contain_their_members_and_nest() {
    for file in ["15-subgraphs.mmd", "16-nested-subgraphs.mmd"] {
        let src = source(file);
        let f = parse(&src).unwrap();
        let d = read(&render(&src).unwrap());
        assert_eq!(d.clusters.len(), f.subgraphs.len(), "{file}");
        for (k, cl) in d.clusters.iter().enumerate() {
            for (i, n) in d.nodes.iter().enumerate() {
                if chain_of(&f, i).contains(&k) {
                    assert!(
                        contains_padded(cl, n),
                        "{file}: cluster {k} {cl:?}, node {n:?}"
                    );
                } else {
                    assert!(!cl.overlaps(n), "{file}: cluster {k} {cl:?} holds {n:?}");
                }
            }
            if let Some(p) = f.subgraphs[k].parent {
                let outer = d.clusters[p];
                assert!(
                    contains_padded(&outer, cl),
                    "{file}: {cl:?} not inside {outer:?}"
                );
            }
        }
    }
}

/// The chain `n0 --> n1 --> …` plus `n0 --> n{i}` for every `i` from 2: quadratic in dummy
/// cells.
fn long_edges(n: usize) -> String {
    let mut s = String::from("graph TD\n");
    for i in 0..n - 1 {
        s.push_str(&format!("  n{i} --> n{}\n", i + 1));
    }
    for i in 2..n {
        s.push_str(&format!("  n0 --> n{i}\n"));
    }
    s
}

#[test]
fn the_cell_cap_refuses_a_quadratic_diagram_fast() {
    let src = long_edges(300);
    assert_eq!(parse(&src).unwrap().edges.len(), 597);
    let start = Instant::now();
    let got = render(&src);
    let took = start.elapsed();
    assert_eq!(
        got,
        Err(Unsupported {
            line: 0,
            why: Why::TooMany {
                what: "layout cells",
                cap: 20000
            }
        })
    );
    eprintln!("long-edges n=300 refused in {took:?}");
    if cfg!(not(debug_assertions)) {
        assert!(took < Duration::from_millis(100), "took {took:?}");
    }
}
