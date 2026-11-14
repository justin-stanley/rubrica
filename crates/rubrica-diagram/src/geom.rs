//! Geometry: integer centipixels (1 px = 100), the layout's constants, and the mapping from the
//! layout's top-down frame to the drawing.
//!
//! Every quantity is an `i64` in centipixels. The only operations are `+ - *`, `div_euclid`
//! and `max`/`min`, so a drawing is the same on every platform.

use crate::{text_width, CAP_HEIGHT};

/// The `font-size` attribute, in pixels.
pub(crate) const FONT_SIZE: i64 = 14;
/// Around the drawing.
pub(crate) const PADDING: i64 = 1600;
/// One line of text.
pub(crate) const TEXT_H: i64 = 1700;
/// Inside a node, left and right.
pub(crate) const NODE_PAD_X: i64 = 1200;
/// Inside a node, top and bottom.
pub(crate) const NODE_PAD_Y: i64 = 800;
/// A node's height: one line of text and its padding.
pub(crate) const NODE_H: i64 = TEXT_H + 2 * NODE_PAD_Y;
/// The narrowest node.
pub(crate) const NODE_MIN_W: i64 = 4000;
/// A round node's corner radius.
pub(crate) const ROUND_R: i64 = 500;
/// A subroutine's inner lines, from its sides.
pub(crate) const SUB_INSET: i64 = 600;
/// An edge label's box, around its text, left and right.
pub(crate) const LABEL_PAD_X: i64 = 400;
/// An edge label's box, around its text, top and bottom.
pub(crate) const LABEL_PAD_Y: i64 = 200;
/// Between two rank bands.
pub(crate) const RANK_SEP: i64 = 4000;
/// Between two node or label cells of a rank.
pub(crate) const NODE_SEP: i64 = 3000;
/// Between two cells of a rank when either is a plain dummy.
pub(crate) const EDGE_SEP: i64 = 1500;
/// The straight piece where an edge leaves or enters a node.
pub(crate) const STUB: i64 = 1000;
/// An arrowhead's length.
pub(crate) const ARROW_LEN: i64 = 900;
/// An arrowhead's half-width.
pub(crate) const ARROW_HALF: i64 = 450;
/// How far the first self-loop of a node reaches out.
pub(crate) const LOOP_OUT: i64 = 1600;
/// How much further each later self-loop reaches.
pub(crate) const LOOP_STEP: i64 = 800;
/// Half the height of a self-loop.
pub(crate) const LOOP_HALF: i64 = 600;
/// Around a subgraph's content.
pub(crate) const CLUSTER_PAD: i64 = 1600;
/// A subgraph title's band.
pub(crate) const TITLE_H: i64 = 2000;
/// Ordering sweeps.
pub(crate) const SWEEPS: usize = 8;
/// Coordinate passes.
pub(crate) const COORD_PASSES: usize = 4;
/// Nodes plus dummy cells.
pub(crate) const MAX_LAYOUT_CELLS: usize = 20_000;
/// The largest number written.
pub(crate) const MAX_COORD: i64 = 99_999_999;
/// A stadium's ports stay this far inside its ends (the radius of its end caps).
pub(crate) const STADIUM_INSET: i64 = 1650;
/// Between a self-loop and its label, and after the label.
pub(crate) const LOOP_LABEL_GAP: i64 = 400;
/// A subgraph title's cap line, below the top of its box.
pub(crate) const TITLE_TOP: i64 = 600;

/// The width of `s` at the 14 px font, rounded half up.
pub(crate) fn tw(s: &str) -> i64 {
    (i64::from(text_width(s)) * 1400 + 500).div_euclid(1000)
}

/// The height of a capital letter at the 14 px font, rounded half up.
pub(crate) fn cap() -> i64 {
    (i64::from(CAP_HEIGHT) * 1400 + 500).div_euclid(1000)
}

/// The size of an edge label's box around text `s`, in the drawing's orientation.
pub(crate) fn label_size(s: &str) -> (i64, i64) {
    (tw(s) + 2 * LABEL_PAD_X, TEXT_H + 2 * LABEL_PAD_Y)
}

/// A point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pt {
    pub x: i64,
    pub y: i64,
}

/// A box: its top-left corner and its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rect {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

impl Rect {
    /// The right edge.
    pub fn right(&self) -> i64 {
        self.x + self.w
    }

    /// The bottom edge.
    pub fn bottom(&self) -> i64 {
        self.y + self.h
    }

    /// The centre: `x + w.div_euclid(2)`, `y + h.div_euclid(2)`.
    pub fn centre(&self) -> Pt {
        Pt {
            x: self.x + self.w.div_euclid(2),
            y: self.y + self.h.div_euclid(2),
        }
    }

    /// The smallest box holding both.
    pub fn union(&self, o: &Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect {
            x,
            y,
            w: self.right().max(o.right()) - x,
            h: self.bottom().max(o.bottom()) - y,
        }
    }
}

/// The flow direction, as the layout sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Orient {
    /// Top down.
    Td,
    /// Bottom up.
    Bt,
    /// Left to right.
    Lr,
    /// Right to left.
    Rl,
}

impl Orient {
    /// Whether ranks run across the drawing (LR, RL): the frame's x is then the drawing's y.
    pub fn across(self) -> bool {
        matches!(self, Orient::Lr | Orient::Rl)
    }

    /// A drawing's `(w, h)` in the frame.
    pub fn size(self, w: i64, h: i64) -> (i64, i64) {
        if self.across() {
            (h, w)
        } else {
            (w, h)
        }
    }

    /// A frame point in the drawing (before normalisation): TD `(x, y)`, BT `(x, −y)`,
    /// LR `(y, x)`, RL `(−y, x)`.
    pub fn point(self, p: Pt) -> Pt {
        match self {
            Orient::Td => p,
            Orient::Bt => Pt { x: p.x, y: -p.y },
            Orient::Lr => Pt { x: p.y, y: p.x },
            Orient::Rl => Pt { x: -p.y, y: p.x },
        }
    }

    /// A frame box in the drawing: its two corners mapped, then ordered.
    pub fn rect(self, r: Rect) -> Rect {
        let a = self.point(Pt { x: r.x, y: r.y });
        let b = self.point(Pt {
            x: r.right(),
            y: r.bottom(),
        });
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        Rect {
            x,
            y,
            w: a.x.max(b.x) - x,
            h: a.y.max(b.y) - y,
        }
    }
}
