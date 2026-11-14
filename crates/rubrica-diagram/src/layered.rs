//! The layered layout, shared by the diagram types drawn as graphs.
//!
//! The layout runs in a top-down frame and maps the result to the drawing's orientation at
//! the end. Its steps, each with a fixed order and a fixed tie-break, so the same input gives
//! the same drawing everywhere:
//!
//! 1. self-loops are set apart and drawn beside their node;
//! 2. cycles are broken by a depth-first search over nodes in index order, each node's
//!    out-edges in edge order: an edge to a node on the search's stack is reversed (its upper
//!    end is its `to`, and its arrowhead is drawn at its start);
//! 3. ranks are longest paths (Kahn's algorithm, the smallest ready node first), an edge with a
//!    label counting 2 and any other 1;
//! 4. an edge across more than one rank gets a dummy cell at each rank between its ends, in
//!    edge order, top to bottom; a labelled edge's first dummy holds its label. The cells are
//!    counted against [`MAX_LAYOUT_CELLS`] before any is made;
//! 5. each rank starts in cell order, then is grouped so every cluster's cells are contiguous;
//! 6. eight sweeps, down then up, order each rank by the barycentre of its neighbours' positions
//!    in the rank before (compared by cross-multiplication, ties by current position; a cell
//!    with no neighbour keeps its place), each followed by the grouping;
//! 7. x: each rank packed from 0, then four passes (down, up, down, up) of a left-to-right and
//!    a right-to-left greedy placement towards the neighbours' mean centre, averaged;
//! 8. y: rank bands, with room for cluster padding and titles;
//! 9. edges through their dummies, with ports spread along each node's side in neighbour order;
//! 10. cluster boxes around their members, innermost first.
//!
//! Every division is a `div_euclid`: frame coordinates are negative in some orientations.

use crate::geom::{
    Orient, Pt, Rect, ARROW_HALF, ARROW_LEN, CLUSTER_PAD, COORD_PASSES, EDGE_SEP, LOOP_HALF,
    LOOP_LABEL_GAP, LOOP_OUT, LOOP_STEP, MAX_COORD, MAX_LAYOUT_CELLS, NODE_SEP, PADDING, RANK_SEP,
    STADIUM_INSET, STUB, SWEEPS, TITLE_H,
};
use crate::{Unsupported, Why};
use std::collections::{BTreeMap, BTreeSet};

/// Where a node's edges attach along its top and bottom sides, in the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ports {
    /// Spread over the whole side.
    Spread,
    /// Spread over the side less [`STADIUM_INSET`] at each end.
    Inset,
    /// All at the side's centre.
    Centre,
}

/// A node to lay out.
pub(crate) struct NodeIn {
    /// Its width in the drawing.
    pub w: i64,
    /// Its height in the drawing.
    pub h: i64,
    /// Where its edges attach.
    pub ports: Ports,
    /// The clusters it is in, outermost first.
    pub path: Vec<usize>,
}

/// An edge to lay out.
pub(crate) struct EdgeIn {
    /// The node it starts at.
    pub from: usize,
    /// The node it ends at.
    pub to: usize,
    /// Its label's size in the drawing, if it has one.
    pub label: Option<(i64, i64)>,
    /// Whether an arrowhead is drawn at its end.
    pub arrow: bool,
}

/// A cluster to lay out. A cluster's parent comes before it (a smaller index).
pub(crate) struct ClusterIn {
    /// Its title's width.
    pub title_w: i64,
}

/// What to lay out.
pub(crate) struct Input {
    /// The flow direction.
    pub orient: Orient,
    /// The nodes.
    pub nodes: Vec<NodeIn>,
    /// The edges.
    pub edges: Vec<EdgeIn>,
    /// The clusters.
    pub clusters: Vec<ClusterIn>,
}

/// An edge laid out.
#[derive(Debug)]
pub(crate) struct EdgeOut {
    /// The line, from its first point to its last.
    pub points: Vec<Pt>,
    /// The arrowhead: its tip, then the two corners of its base.
    pub arrow: Option<[Pt; 3]>,
    /// The label's box.
    pub label: Option<Rect>,
}

/// The layout, normalised: every coordinate is at least [`PADDING`], and `w` and `h` hold the
/// drawing with its padding.
#[derive(Debug)]
pub(crate) struct Output {
    /// Per node, its box.
    pub nodes: Vec<Rect>,
    /// Per edge, its line, arrowhead and label.
    pub edges: Vec<EdgeOut>,
    /// Per cluster, its box; `None` for a cluster with no member node.
    pub clusters: Vec<Option<Rect>>,
    /// The drawing's width.
    pub w: i64,
    /// The drawing's height.
    pub h: i64,
}

/// A cell of a rank: a node, or a dummy of an edge.
struct Cell {
    rank: usize,
    /// The frame width (0 for a plain dummy).
    w: i64,
    /// The frame height (0 for a plain dummy).
    h: i64,
    /// A dummy without a label.
    plain: bool,
    /// The cluster path is `nodes[path_node].path[..path_len]`.
    path_node: usize,
    path_len: usize,
}

/// The direction an arrowhead points, in the frame.
#[derive(Clone, Copy)]
enum Head {
    Down,
    Up,
    Left,
}

fn too_many() -> Unsupported {
    Unsupported {
        line: 0,
        why: Why::TooMany {
            what: "layout cells",
            cap: MAX_LAYOUT_CELLS,
        },
    }
}

/// The length of the common prefix of `a` and `b`.
fn common(a: &[usize], b: &[usize]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

/// The ranks a sweep or a coordinate pass visits: `1..=max` down, `max − 1` to `0` up.
fn pass_ranks(down: bool, max: usize) -> Vec<usize> {
    if down {
        (1..=max).collect()
    } else {
        (0..max).rev().collect()
    }
}

/// The arrowhead with its tip at `t`, pointing `head`.
fn arrowhead(t: Pt, head: Head) -> [Pt; 3] {
    let at = |dx: i64, dy: i64| Pt {
        x: t.x + dx,
        y: t.y + dy,
    };
    match head {
        Head::Down => [t, at(-ARROW_HALF, -ARROW_LEN), at(ARROW_HALF, -ARROW_LEN)],
        Head::Up => [t, at(ARROW_HALF, ARROW_LEN), at(-ARROW_HALF, ARROW_LEN)],
        Head::Left => [t, at(ARROW_LEN, -ARROW_HALF), at(ARROW_LEN, ARROW_HALF)],
    }
}

/// The graph after steps 1 to 4: the layered edges with their direction, the ranks and the
/// cells.
struct Graph<'a> {
    input: &'a Input,
    /// The edges between two different nodes, in edge order.
    layered: Vec<usize>,
    /// Per edge, whether it is reversed (drawn from `to` down to `from`).
    reversed: Vec<bool>,
    cells: Vec<Cell>,
    /// Per edge, its first dummy cell and how many it has.
    dummies: Vec<(usize, usize)>,
    /// Per cell, the cells joined to it by a segment in the rank above (with multiplicity).
    above: Vec<Vec<usize>>,
    /// Per cell, the cells joined to it by a segment in the rank below.
    below: Vec<Vec<usize>>,
    max_rank: usize,
}

impl Graph<'_> {
    fn upper(&self, e: usize) -> usize {
        let edge = &self.input.edges[e];
        if self.reversed[e] {
            edge.to
        } else {
            edge.from
        }
    }

    fn lower(&self, e: usize) -> usize {
        let edge = &self.input.edges[e];
        if self.reversed[e] {
            edge.from
        } else {
            edge.to
        }
    }

    fn path(&self, c: usize) -> &[usize] {
        let cell = &self.cells[c];
        &self.input.nodes[cell.path_node].path[..cell.path_len]
    }

    /// The chain of edge `e`: its upper node, its dummies top to bottom, its lower node.
    fn chain(&self, e: usize) -> Vec<usize> {
        let (first, count) = self.dummies[e];
        let mut c = vec![self.upper(e)];
        c.extend(first..first + count);
        c.push(self.lower(e));
        c
    }

    /// `cells` grouped from depth `d`: a cell outside any cluster at depth `d` is a group of
    /// its own, the cells of one cluster form one group (at the cluster's first occurrence),
    /// groups in creation order, each cluster's group grouped again at `d + 1`.
    fn group(&self, cells: &[usize], d: usize, out: &mut Vec<usize>) {
        let mut groups: Vec<(bool, Vec<usize>)> = Vec::new();
        let mut at: BTreeMap<usize, usize> = BTreeMap::new();
        for &c in cells {
            match self.path(c).get(d) {
                None => groups.push((false, vec![c])),
                Some(k) => match at.get(k) {
                    Some(&g) => groups[g].1.push(c),
                    None => {
                        at.insert(*k, groups.len());
                        groups.push((true, vec![c]));
                    }
                },
            }
        }
        for (cluster, members) in groups {
            if cluster {
                self.group(&members, d + 1, out);
            } else {
                out.extend(members);
            }
        }
    }
}

/// Steps 1 to 4: self-loops apart, cycles broken, ranks, dummy cells.
fn build(input: &Input) -> Result<Graph<'_>, Unsupported> {
    let n = input.nodes.len();
    let m = input.edges.len();
    let layered: Vec<usize> = (0..m)
        .filter(|&e| input.edges[e].from != input.edges[e].to)
        .collect();

    // 2. Cycle removal: an iterative depth-first search.
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &e in &layered {
        out[input.edges[e].from].push(e);
    }
    const WHITE: u8 = 0;
    const GREY: u8 = 1;
    const BLACK: u8 = 2;
    let mut colour = vec![WHITE; n];
    let mut reversed = vec![false; m];
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for s in 0..n {
        if colour[s] != WHITE {
            continue;
        }
        colour[s] = GREY;
        stack.push((s, 0));
        while let Some(top) = stack.last_mut() {
            let (v, i) = *top;
            if let Some(&e) = out[v].get(i) {
                top.1 += 1;
                let w = input.edges[e].to;
                if colour[w] == WHITE {
                    colour[w] = GREY;
                    stack.push((w, 0));
                } else if colour[w] == GREY {
                    reversed[e] = true;
                }
            } else {
                colour[v] = BLACK;
                stack.pop();
            }
        }
    }
    let mut g = Graph {
        input,
        layered,
        reversed,
        cells: Vec::new(),
        dummies: vec![(0, 0); m],
        above: Vec::new(),
        below: Vec::new(),
        max_rank: 0,
    };

    // 3. Ranking: Kahn's algorithm over the reversed graph, the smallest ready node first.
    let mut down: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indegree = vec![0usize; n];
    for &e in &g.layered {
        down[g.upper(e)].push(e);
        indegree[g.lower(e)] += 1;
    }
    let mut ready: BTreeSet<usize> = (0..n).filter(|&v| indegree[v] == 0).collect();
    let mut rank = vec![0usize; n];
    while let Some(v) = ready.pop_first() {
        for &e in &down[v] {
            let w = g.lower(e);
            let minlen = if input.edges[e].label.is_some() { 2 } else { 1 };
            rank[w] = rank[w].max(rank[v] + minlen);
            indegree[w] -= 1;
            if indegree[w] == 0 {
                ready.insert(w);
            }
        }
    }

    // 4. Cells: counted first, then made.
    let spans: usize = g
        .layered
        .iter()
        .map(|&e| rank[g.lower(e)] - rank[g.upper(e)] - 1)
        .sum();
    if n + spans > MAX_LAYOUT_CELLS {
        return Err(too_many());
    }
    let orient = input.orient;
    let mut cells: Vec<Cell> = Vec::with_capacity(n + spans);
    for (v, node) in input.nodes.iter().enumerate() {
        let (w, h) = orient.size(node.w, node.h);
        cells.push(Cell {
            rank: rank[v],
            w,
            h,
            plain: false,
            path_node: v,
            path_len: node.path.len(),
        });
    }
    for &e in &g.layered {
        let (u, l) = (g.upper(e), g.lower(e));
        let shared = common(&input.nodes[u].path, &input.nodes[l].path);
        g.dummies[e] = (cells.len(), rank[l] - rank[u] - 1);
        for r in rank[u] + 1..rank[l] {
            let label = input.edges[e].label.filter(|_| r == rank[u] + 1);
            let (w, h) = label.map_or((0, 0), |(w, h)| orient.size(w, h));
            cells.push(Cell {
                rank: r,
                w,
                h,
                plain: label.is_none(),
                path_node: u,
                path_len: shared,
            });
        }
    }
    g.max_rank = cells.iter().map(|c| c.rank).max().unwrap_or(0);
    g.cells = cells;
    g.above = vec![Vec::new(); g.cells.len()];
    g.below = vec![Vec::new(); g.cells.len()];
    for i in 0..g.layered.len() {
        let chain = g.chain(g.layered[i]);
        for pair in chain.windows(2) {
            g.below[pair[0]].push(pair[1]);
            g.above[pair[1]].push(pair[0]);
        }
    }
    Ok(g)
}

/// Steps 5 and 6: each rank's order, and each cell's position in it.
fn order(g: &Graph<'_>) -> (Vec<Vec<usize>>, Vec<usize>) {
    let mut ranks: Vec<Vec<usize>> = vec![Vec::new(); g.max_rank + 1];
    for (c, cell) in g.cells.iter().enumerate() {
        ranks[cell.rank].push(c);
    }
    let mut pos = vec![0usize; g.cells.len()];
    for rank in &mut ranks {
        let mut grouped = Vec::with_capacity(rank.len());
        g.group(rank, 0, &mut grouped);
        for (p, &c) in grouped.iter().enumerate() {
            pos[c] = p;
        }
        *rank = grouped;
    }
    for i in 0..SWEEPS {
        let down = i % 2 == 0;
        let fixed = if down { &g.above } else { &g.below };
        for r in pass_ranks(down, g.max_rank) {
            let current = &ranks[r];
            let mut slots: Vec<usize> = Vec::new();
            // (Σ neighbour positions, neighbours, current position, cell)
            let mut keyed: Vec<(i128, i128, usize, usize)> = Vec::new();
            for (p, &c) in current.iter().enumerate() {
                let nb = &fixed[c];
                if nb.is_empty() {
                    continue;
                }
                let sum: i128 = nb.iter().map(|&x| pos[x] as i128).sum();
                slots.push(p);
                keyed.push((sum, nb.len() as i128, p, c));
            }
            keyed.sort_by(|a, b| (a.0 * b.1).cmp(&(b.0 * a.1)).then(a.2.cmp(&b.2)));
            let mut next = current.clone();
            for (&slot, k) in slots.iter().zip(&keyed) {
                next[slot] = k.3;
            }
            let mut grouped = Vec::with_capacity(next.len());
            g.group(&next, 0, &mut grouped);
            for (p, &c) in grouped.iter().enumerate() {
                pos[c] = p;
            }
            ranks[r] = grouped;
        }
    }
    (ranks, pos)
}

/// Lays out `input`: boxes, edge lines, arrowheads, labels and cluster boxes, normalised.
pub(crate) fn layout(input: &Input) -> Result<Output, Unsupported> {
    let g = build(input)?;
    let (ranks, pos) = order(&g);
    let n = input.nodes.len();
    let orient = input.orient;
    let cells = &g.cells;

    // Self-loops, per node in edge order, and the room they need beside it.
    let mut loops: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (e, edge) in input.edges.iter().enumerate() {
        if edge.from == edge.to {
            loops[edge.from].push(e);
        }
    }
    let loop_label = |e: usize| input.edges[e].label.map(|(w, h)| orient.size(w, h));
    let reach = |j: usize| LOOP_OUT + LOOP_STEP * j as i64;
    let mut extra = vec![0i64; cells.len()];
    for (v, ls) in loops.iter().enumerate() {
        if let Some(most) = ls
            .iter()
            .enumerate()
            .map(|(j, &e)| reach(j) + LOOP_LABEL_GAP + loop_label(e).map_or(0, |s| s.0))
            .max()
        {
            extra[v] = most + LOOP_LABEL_GAP;
        }
    }

    // 7. Coordinates.
    let width = |c: usize| cells[c].w + extra[c];
    let gap = |a: usize, b: usize| {
        let base = if cells[a].plain || cells[b].plain {
            EDGE_SEP
        } else {
            NODE_SEP
        };
        let (pa, pb) = (g.path(a), g.path(b));
        let shared = common(pa, pb);
        let exits = (pa.len() - shared) as i64;
        let enters = (pb.len() - shared) as i64;
        let title = if orient.across() { TITLE_H * enters } else { 0 };
        base + CLUSTER_PAD * (exits + enters) + title
    };
    let mut left = vec![0i64; cells.len()];
    for rank in &ranks {
        for i in 1..rank.len() {
            let (a, b) = (rank[i - 1], rank[i]);
            left[b] = left[a] + width(a) + gap(a, b);
        }
    }
    for pass in 0..COORD_PASSES {
        let down = pass % 2 == 0;
        let fixed = if down { &g.above } else { &g.below };
        for r in pass_ranks(down, g.max_rank) {
            let rank = &ranks[r];
            let k = rank.len();
            let desired: Vec<i64> = rank
                .iter()
                .map(|&c| {
                    let nb = &fixed[c];
                    if nb.is_empty() {
                        left[c]
                    } else {
                        let sum: i64 = nb.iter().map(|&x| left[x] + cells[x].w.div_euclid(2)).sum();
                        sum.div_euclid(nb.len() as i64) - cells[c].w.div_euclid(2)
                    }
                })
                .collect();
            let mut l = desired.clone();
            for i in 1..k {
                let (a, b) = (rank[i - 1], rank[i]);
                l[i] = desired[i].max(l[i - 1] + width(a) + gap(a, b));
            }
            let mut rr = desired.clone();
            for i in (0..k.saturating_sub(1)).rev() {
                let (a, b) = (rank[i], rank[i + 1]);
                rr[i] = desired[i].min(rr[i + 1] - gap(a, b) - width(a));
            }
            for i in 0..k {
                left[rank[i]] = (l[i] + rr[i]).div_euclid(2);
            }
        }
    }
    let centre = |c: usize| left[c] + cells[c].w.div_euclid(2);

    // 8. Bands.
    let band_h: Vec<i64> = ranks
        .iter()
        .map(|rank| rank.iter().map(|&c| cells[c].h).max().unwrap_or(0))
        .collect();
    let mut span: Vec<(usize, usize)> = vec![(usize::MAX, 0); input.clusters.len()];
    for (c, cell) in cells.iter().enumerate() {
        for &k in g.path(c) {
            span[k] = (span[k].0.min(cell.rank), span[k].1.max(cell.rank));
        }
    }
    let count_at = |r: usize, end: bool| -> i64 {
        ranks[r]
            .iter()
            .map(|&c| {
                g.path(c)
                    .iter()
                    .filter(|&&k| (if end { span[k].1 } else { span[k].0 }) == r)
                    .count() as i64
            })
            .max()
            .unwrap_or(0)
    };
    let td = i64::from(orient == Orient::Td);
    let bt = i64::from(orient == Orient::Bt);
    let mut y_top = vec![0i64; ranks.len()];
    let starts0 = count_at(0, false);
    y_top[0] = CLUSTER_PAD * starts0 + TITLE_H * td * starts0;
    for r in 1..ranks.len() {
        let (ends, starts) = (count_at(r - 1, true), count_at(r, false));
        y_top[r] = y_top[r - 1]
            + band_h[r - 1]
            + RANK_SEP
            + CLUSTER_PAD * (ends + starts)
            + TITLE_H * (td * starts + bt * ends);
    }
    let top = |c: usize| {
        let r = cells[c].rank;
        y_top[r] + (band_h[r] - cells[c].h).div_euclid(2)
    };

    // 9. Edges, in the frame. Ports first: per node, its segments down and up in the order of
    // the cell at the other end, then by edge.
    let mut outs: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    let mut ins: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    for &e in &g.layered {
        let chain = g.chain(e);
        outs[g.upper(e)].push((pos[chain[1]], e));
        ins[g.lower(e)].push((pos[chain[chain.len() - 2]], e));
    }
    let port = |v: usize, k: usize, count: usize| {
        let (a, b) = match input.nodes[v].ports {
            Ports::Spread => (left[v], left[v] + cells[v].w),
            Ports::Inset => (
                left[v] + STADIUM_INSET,
                left[v] + cells[v].w - STADIUM_INSET,
            ),
            Ports::Centre => (centre(v), centre(v)),
        };
        a + ((b - a) * (k as i64 + 1)).div_euclid(count as i64 + 1)
    };
    let mut out_x = vec![0i64; input.edges.len()];
    let mut in_x = vec![0i64; input.edges.len()];
    for v in 0..n {
        outs[v].sort();
        ins[v].sort();
        for (k, &(_, e)) in outs[v].iter().enumerate() {
            out_x[e] = port(v, k, outs[v].len());
        }
        for (k, &(_, e)) in ins[v].iter().enumerate() {
            in_x[e] = port(v, k, ins[v].len());
        }
    }
    let node_box = |v: usize| Rect {
        x: left[v],
        y: top(v),
        w: cells[v].w,
        h: cells[v].h,
    };
    let mut edges: Vec<EdgeOut> = Vec::with_capacity(input.edges.len());
    for (e, edge) in input.edges.iter().enumerate() {
        let p = |x: i64, y: i64| Pt { x, y };
        if edge.from == edge.to {
            let v = edge.from;
            let j = loops[v].iter().position(|&x| x == e).unwrap_or(0);
            let b = node_box(v);
            let (r, cy) = (b.right(), b.centre().y);
            let out = reach(j);
            let end_x = if edge.arrow { r + ARROW_LEN } else { r };
            edges.push(EdgeOut {
                points: vec![
                    p(r, cy - LOOP_HALF),
                    p(r + out, cy - LOOP_HALF),
                    p(r + out, cy + LOOP_HALF),
                    p(end_x, cy + LOOP_HALF),
                ],
                arrow: edge
                    .arrow
                    .then(|| arrowhead(p(r, cy + LOOP_HALF), Head::Left)),
                label: loop_label(e).map(|(w, h)| Rect {
                    x: r + out + LOOP_LABEL_GAP,
                    y: cy - h.div_euclid(2),
                    w,
                    h,
                }),
            });
            continue;
        }
        let (u, l) = (g.upper(e), g.lower(e));
        let at_start = edge.arrow && g.reversed[e];
        let at_end = edge.arrow && !g.reversed[e];
        let bottom = top(u) + cells[u].h;
        let s = if at_start { ARROW_LEN } else { 0 };
        let t = if at_end { ARROW_LEN } else { 0 };
        let mut points = vec![p(out_x[e], bottom + s), p(out_x[e], bottom + s + STUB)];
        let (first, count) = g.dummies[e];
        for (d, cell) in cells.iter().enumerate().skip(first).take(count) {
            let r = cell.rank;
            points.push(p(centre(d), y_top[r]));
            points.push(p(centre(d), y_top[r] + band_h[r]));
        }
        points.push(p(in_x[e], top(l) - t - STUB));
        points.push(p(in_x[e], top(l) - t));
        points.dedup();
        let arrow = if at_end {
            Some(arrowhead(p(in_x[e], top(l)), Head::Down))
        } else if at_start {
            Some(arrowhead(p(out_x[e], bottom), Head::Up))
        } else {
            None
        };
        let label = edge.label.map(|_| {
            let r = cells[first].rank;
            let (w, h) = (cells[first].w, cells[first].h);
            Rect {
                x: centre(first) - w.div_euclid(2),
                y: y_top[r] + band_h[r].div_euclid(2) - h.div_euclid(2),
                w,
                h,
            }
        });
        edges.push(EdgeOut {
            points,
            arrow,
            label,
        });
    }

    // Into the drawing's orientation.
    let nodes: Vec<Rect> = (0..n).map(|v| orient.rect(node_box(v))).collect();
    for edge in &mut edges {
        for q in &mut edge.points {
            *q = orient.point(*q);
        }
        if let Some(a) = &mut edge.arrow {
            for q in a.iter_mut() {
                *q = orient.point(*q);
            }
        }
        edge.label = edge.label.map(|b| orient.rect(b));
    }

    // 10. Clusters, innermost first: members' boxes, then the padding and the title band.
    let k = input.clusters.len();
    let mut parent: Vec<Option<usize>> = vec![None; k];
    for node in &input.nodes {
        for pair in node.path.windows(2) {
            parent[pair[1]] = Some(pair[0]);
        }
    }
    let mut inside: Vec<Option<Rect>> = vec![None; k];
    let add = |slot: &mut Option<Rect>, b: Rect| {
        *slot = Some(slot.map_or(b, |s| s.union(&b)));
    };
    for (v, node) in input.nodes.iter().enumerate() {
        if let Some(&c) = node.path.last() {
            add(&mut inside[c], nodes[v]);
        }
    }
    let mut clusters: Vec<Option<Rect>> = vec![None; k];
    for c in (0..k).rev() {
        let Some(b) = inside[c] else { continue };
        let w = (b.w + 2 * CLUSTER_PAD).max(input.clusters[c].title_w + 2 * CLUSTER_PAD);
        let grown = Rect {
            x: b.x - CLUSTER_PAD,
            y: b.y - CLUSTER_PAD - TITLE_H,
            w,
            h: b.h + 2 * CLUSTER_PAD + TITLE_H,
        };
        clusters[c] = Some(grown);
        if let Some(p) = parent[c] {
            add(&mut inside[p], grown);
        }
    }

    // Normalised: the smallest x and y become PADDING.
    let mut extent: Option<Rect> = None;
    let boxes = nodes
        .iter()
        .chain(edges.iter().filter_map(|e| e.label.as_ref()))
        .chain(clusters.iter().flatten());
    for b in boxes {
        add(&mut extent, *b);
    }
    for e in &edges {
        for q in e.points.iter().chain(e.arrow.iter().flatten()) {
            add(
                &mut extent,
                Rect {
                    x: q.x,
                    y: q.y,
                    w: 0,
                    h: 0,
                },
            );
        }
    }
    let extent = extent.unwrap_or(Rect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    });
    let (dx, dy) = (PADDING - extent.x, PADDING - extent.y);
    let (w, h) = (extent.w + 2 * PADDING, extent.h + 2 * PADDING);
    if w > MAX_COORD || h > MAX_COORD {
        return Err(Unsupported {
            line: 0,
            why: Why::DrawingTooLarge,
        });
    }
    let shift = |b: Rect| Rect {
        x: b.x + dx,
        y: b.y + dy,
        ..b
    };
    let shift_pt = |q: Pt| Pt {
        x: q.x + dx,
        y: q.y + dy,
    };
    for e in &mut edges {
        e.points.iter_mut().for_each(|q| *q = shift_pt(*q));
        if let Some(a) = &mut e.arrow {
            a.iter_mut().for_each(|q| *q = shift_pt(*q));
        }
        e.label = e.label.map(shift);
    }
    Ok(Output {
        nodes: nodes.into_iter().map(shift).collect(),
        edges,
        clusters: clusters.into_iter().map(|c| c.map(shift)).collect(),
        w,
        h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(w: i64) -> NodeIn {
        NodeIn {
            w,
            h: 3300,
            ports: Ports::Spread,
            path: Vec::new(),
        }
    }

    #[test]
    fn a_drawing_over_the_largest_number_is_refused() {
        let wide = |w: i64| Input {
            orient: Orient::Td,
            nodes: vec![node(w)],
            edges: Vec::new(),
            clusters: Vec::new(),
        };
        let fits = layout(&wide(MAX_COORD - 2 * PADDING)).unwrap();
        assert_eq!((fits.w, fits.h), (MAX_COORD, 3300 + 2 * PADDING));
        let err = layout(&wide(MAX_COORD - 2 * PADDING + 1)).unwrap_err();
        assert_eq!(err.why, Why::DrawingTooLarge);
        assert_eq!(err.line, 0);
    }

    #[test]
    fn grouping_keeps_each_cluster_contiguous_in_first_occurrence_order() {
        // One rank, no edges: cells in index order are 0 (in 0), 1 (outside), 2 (in 0 and its
        // child 1), 3 (in 0), 4 (in 2), 5 (in 0 and 1). Cluster 0 first occurs at cell 0, so
        // its cells come first, its child's together at the child's first occurrence (cell 2);
        // then cell 1, then cluster 2.
        let at = |path: Vec<usize>| NodeIn { path, ..node(4000) };
        let input = Input {
            orient: Orient::Td,
            nodes: vec![
                at(vec![0]),
                at(vec![]),
                at(vec![0, 1]),
                at(vec![0]),
                at(vec![2]),
                at(vec![0, 1]),
            ],
            edges: Vec::new(),
            clusters: (0..3).map(|_| ClusterIn { title_w: 0 }).collect(),
        };
        let g = build(&input).unwrap();
        let (ranks, pos) = order(&g);
        assert_eq!(ranks, [vec![0, 2, 5, 3, 1, 4]]);
        assert_eq!(pos, [0, 4, 1, 3, 5, 2]);
    }

    #[test]
    fn barycentres_compare_exactly_and_tie_by_position() {
        // Rank 0 holds 0, 1, 2; rank 1 holds 3 (under 2), 4 (under 0), 5 (under 0 and 1) and
        // 6 (under 0). The keys are 2, 0, 1/2 and 0: 4 and 6 tie and keep their order, and 5
        // (1/2) goes after them, where a truncated mean (0) would put it between them.
        let edge = |from, to| EdgeIn {
            from,
            to,
            label: None,
            arrow: true,
        };
        let input = Input {
            orient: Orient::Td,
            nodes: (0..7).map(|_| node(4000)).collect(),
            edges: vec![edge(2, 3), edge(0, 4), edge(0, 5), edge(1, 5), edge(0, 6)],
            clusters: Vec::new(),
        };
        let g = build(&input).unwrap();
        let (ranks, _) = order(&g);
        assert_eq!(ranks, [vec![0, 1, 2], vec![4, 6, 5, 3]]);
    }

    #[test]
    fn the_cell_count_is_checked_before_the_cells_exist() {
        // A chain of three, and an edge from the first to the last spanning one dummy: three
        // nodes and one dummy are four cells.
        let edge = |from, to| EdgeIn {
            from,
            to,
            label: None,
            arrow: true,
        };
        let input = Input {
            orient: Orient::Td,
            nodes: (0..3).map(|_| node(4000)).collect(),
            edges: vec![edge(0, 1), edge(1, 2), edge(0, 2)],
            clusters: Vec::new(),
        };
        let g = build(&input).unwrap();
        assert_eq!(g.cells.len(), 4);
        assert_eq!(g.dummies[2], (3, 1));
    }
}
