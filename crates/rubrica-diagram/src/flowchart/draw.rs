//! A flowchart drawn: node sizes, the layered layout, the SVG document.

use super::{Direction, EdgeKind, Flowchart, Shape};
use crate::geom::{
    cap, label_size, tw, Orient, Pt, Rect, FONT_SIZE, NODE_H, NODE_MIN_W, NODE_PAD_X, ROUND_R,
    SUB_INSET, TITLE_TOP,
};
use crate::layered::{layout, ClusterIn, EdgeIn, Input, NodeIn, Ports};
use crate::svg::{closed, line, num, Writer};
use crate::{Unsupported, FONT_FAMILY};

/// A node's size in the drawing (width, height) and where its edges attach.
fn node_shape(shape: Shape, label: &str, orient: Orient) -> (i64, i64, Ports) {
    let text = tw(label) + 2 * NODE_PAD_X;
    match shape {
        Shape::Rect | Shape::Round => (text.max(NODE_MIN_W), NODE_H, Ports::Spread),
        Shape::Stadium => {
            let ports = if orient.across() {
                Ports::Centre
            } else {
                Ports::Inset
            };
            ((text + NODE_H).max(NODE_MIN_W), NODE_H, ports)
        }
        Shape::Subroutine => (
            (text + 2 * SUB_INSET).max(NODE_MIN_W),
            NODE_H,
            Ports::Spread,
        ),
        Shape::Rhombus => (text + NODE_H, text + NODE_H, Ports::Centre),
    }
}

/// The layout's input for `f`.
fn input(f: &Flowchart) -> Input {
    let orient = match f.direction {
        Direction::TopDown => Orient::Td,
        Direction::BottomUp => Orient::Bt,
        Direction::LeftRight => Orient::Lr,
        Direction::RightLeft => Orient::Rl,
    };
    // Each subgraph's path: its ancestors, outermost first, then itself.
    let mut paths: Vec<Vec<usize>> = Vec::with_capacity(f.subgraphs.len());
    for (k, s) in f.subgraphs.iter().enumerate() {
        let mut p = s.parent.map_or_else(Vec::new, |q| paths[q].clone());
        p.push(k);
        paths.push(p);
    }
    Input {
        orient,
        nodes: f
            .nodes
            .iter()
            .map(|n| {
                let (w, h, ports) = node_shape(n.shape, &n.label, orient);
                NodeIn {
                    w,
                    h,
                    ports,
                    path: n.subgraph.map_or_else(Vec::new, |k| paths[k].clone()),
                }
            })
            .collect(),
        edges: f
            .edges
            .iter()
            .map(|e| EdgeIn {
                from: e.from,
                to: e.to,
                label: e.label.as_deref().map(label_size),
                arrow: e.kind != EdgeKind::Open,
            })
            .collect(),
        clusters: f
            .subgraphs
            .iter()
            .map(|s| ClusterIn {
                title_w: tw(&s.title),
            })
            .collect(),
    }
}

/// `x`, `y`, `width`, `height` of `b`, written.
fn rect_attrs(b: &Rect) -> [String; 4] {
    [num(b.x), num(b.y), num(b.w), num(b.h)]
}

/// A `rect` of class `class` over `b`, with corner radius `r` when given.
fn rect(w: &mut Writer, class: &str, b: &Rect, r: Option<i64>) {
    let [x, y, width, height] = rect_attrs(b);
    let mut attrs = vec![
        ("class", class),
        ("x", &x),
        ("y", &y),
        ("width", &width),
        ("height", &height),
    ];
    let r = r.map(num);
    if let Some(r) = &r {
        attrs.push(("rx", r));
        attrs.push(("ry", r));
    }
    w.empty("rect", &attrs);
}

/// A `path` of class `class`.
fn path(w: &mut Writer, class: &str, d: &str) {
    w.empty("path", &[("class", class), ("d", d)]);
}

/// Text centred on `at`, its capitals centred on the line.
fn centred_text(w: &mut Writer, class: &str, at: Pt, text: &str) {
    let x = num(at.x);
    let y = num(at.y + cap().div_euclid(2));
    w.text(
        "text",
        &[
            ("class", class),
            ("x", &x),
            ("y", &y),
            ("text-anchor", "middle"),
        ],
        text,
    );
}

/// A node's shape.
fn shape(w: &mut Writer, shape: Shape, b: &Rect) {
    match shape {
        Shape::Rect => rect(w, "rd-shape", b, None),
        Shape::Round => rect(w, "rd-shape", b, Some(ROUND_R)),
        Shape::Stadium => rect(w, "rd-shape", b, Some(b.h.div_euclid(2))),
        Shape::Subroutine => {
            rect(w, "rd-shape", b, None);
            for x in [b.x + SUB_INSET, b.right() - SUB_INSET] {
                let d = format!("M{} {} V{}", num(x), num(b.y), num(b.bottom()));
                path(w, "rd-shape-line", &d);
            }
        }
        Shape::Rhombus => {
            let c = b.centre();
            let p = |x: i64, y: i64| Pt { x, y };
            let corners = [
                p(c.x, b.y),
                p(b.right(), c.y),
                p(c.x, b.bottom()),
                p(b.x, c.y),
            ];
            path(w, "rd-shape", &closed(&corners));
        }
    }
}

/// Draws a parsed flowchart as one `<svg>` element.
pub fn draw(f: &Flowchart) -> Result<String, Unsupported> {
    let out = layout(&input(f))?;
    let mut w = Writer::default();
    let (width, height) = (num(out.w), num(out.h));
    let view_box = format!("0 0 {width} {height}");
    let family = format!("{FONT_FAMILY}, sans-serif");
    let size = FONT_SIZE.to_string();
    w.open(
        "svg",
        &[
            ("role", "img"),
            ("viewBox", &view_box),
            ("width", &width),
            ("height", &height),
            ("class", "rd rd-flowchart"),
            ("font-family", &family),
            ("font-size", &size),
        ],
    );
    w.text("title", &[], &f.title);
    w.text("desc", &[], &f.source);

    w.open("g", &[("class", "rd-clusters")]);
    for (s, b) in f.subgraphs.iter().zip(&out.clusters) {
        let Some(b) = b else { continue };
        w.open("g", &[("class", "rd-cluster")]);
        rect(&mut w, "rd-cluster-box", b, None);
        let x = num(b.centre().x);
        let y = num(b.y + TITLE_TOP + cap());
        w.text(
            "text",
            &[
                ("class", "rd-cluster-title"),
                ("x", &x),
                ("y", &y),
                ("text-anchor", "middle"),
            ],
            &s.title,
        );
        w.close("g");
    }
    w.close("g");

    w.open("g", &[("class", "rd-edges")]);
    for (e, laid) in f.edges.iter().zip(&out.edges) {
        let class = match e.kind {
            EdgeKind::Arrow | EdgeKind::Open => "rd-edge rd-edge-solid",
            EdgeKind::Dotted => "rd-edge rd-edge-dotted",
            EdgeKind::Thick => "rd-edge rd-edge-thick",
        };
        w.open("g", &[("class", class)]);
        path(&mut w, "rd-line", &line(&laid.points));
        if let Some(a) = &laid.arrow {
            path(&mut w, "rd-arrow", &closed(a));
        }
        w.close("g");
    }
    w.close("g");

    w.open("g", &[("class", "rd-nodes")]);
    for (n, b) in f.nodes.iter().zip(&out.nodes) {
        w.open("g", &[("class", "rd-node")]);
        shape(&mut w, n.shape, b);
        centred_text(&mut w, "rd-text", b.centre(), &n.label);
        w.close("g");
    }
    w.close("g");

    w.open("g", &[("class", "rd-labels")]);
    for (e, laid) in f.edges.iter().zip(&out.edges) {
        let (Some(text), Some(b)) = (&e.label, &laid.label) else {
            continue;
        };
        w.open("g", &[("class", "rd-edge-label")]);
        rect(&mut w, "rd-label-bg", b, None);
        centred_text(&mut w, "rd-text", b.centre(), text);
        w.close("g");
    }
    w.close("g");
    w.close("svg");
    Ok(w.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flowchart::parse;
    use crate::{render, Why};

    fn laid(src: &str) -> crate::layered::Output {
        layout(&input(&parse(src).unwrap())).unwrap()
    }

    #[test]
    fn other_diagram_kinds_are_not_drawn_yet() {
        for (src, keyword) in [
            ("sequenceDiagram\n  A->>B: hi", "sequenceDiagram"),
            ("%% note\nstateDiagram-v2\n  [*] --> A", "stateDiagram-v2"),
        ] {
            let want = Unsupported {
                line: if keyword == "sequenceDiagram" { 1 } else { 2 },
                why: Why::UnknownType {
                    keyword: keyword.to_string(),
                },
            };
            assert_eq!(render(src), Err(want));
        }
    }

    #[test]
    fn a_stadium_across_takes_its_edges_at_its_centre() {
        let across = laid("graph LR\n  A([one]) --> B([two])\n  A --> B");
        let starts: Vec<Pt> = across.edges.iter().map(|e| e.points[0]).collect();
        assert_eq!(starts[0], starts[1]);
        assert_eq!(starts[0].y, across.nodes[0].centre().y);
        let down = laid("graph TD\n  A([one]) --> B([two])\n  A --> B");
        let (a, b) = (down.edges[0].points[0].x, down.edges[1].points[0].x);
        let node = down.nodes[0];
        assert!(
            node.x + 1650 < a && a < b && b < node.right() - 1650,
            "{a} {b} {node:?}"
        );
    }

    #[test]
    fn a_subgraph_without_nodes_is_not_drawn() {
        let svg = render("graph TD\n  subgraph s [Empty]\n  end\n  A").unwrap();
        assert!(svg.contains("<g class=\"rd-clusters\"></g>"), "{svg}");
        assert_eq!(laid("graph TD\n  subgraph s\n  end\n  A").clusters, [None]);
    }

    #[test]
    fn an_open_self_loop_has_no_arrowhead() {
        let out = laid("graph TD\n  A --- A");
        let e = &out.edges[0];
        assert!(e.arrow.is_none());
        let right = out.nodes[0].right();
        assert_eq!(e.points[0].x, right);
        assert_eq!(e.points[3].x, right);
    }

    #[test]
    fn a_wide_subgraph_title_widens_its_box() {
        let title = "a long title for one small node";
        let out = laid(&format!("graph TD\n  subgraph s [{title}]\n    A\n  end"));
        let b = out.clusters[0].unwrap();
        assert_eq!(b.w, tw(title) + 2 * crate::geom::CLUSTER_PAD);
        assert_eq!(b.x, out.nodes[0].x - crate::geom::CLUSTER_PAD);
    }
}
