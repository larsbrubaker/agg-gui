//! Edge resizing of hosted cards (see [`super::hosted`]): where a card's
//! resize bands are, and what a drag on one tells the model.
//!
//! The bands sit in a [`RESIZE_GRIP`]-wide border *outside* the drawn
//! card, as agg-sharp's `WindowWidget` puts its `GrabControl`s in the
//! border around its panel, so a press anywhere on the drawn card reaches
//! the body's controls (a text field running to the card's edge keeps its
//! last pixels). Every hosted card has the right band; a card the model
//! opts in through [`crate::NodeGraphModel::node_vertical_resize`] also
//! has the top and bottom bands. The grip is in canvas units, so it
//! scales with the zoom as the grab handles of a window inside MatterCAD's
//! zoomed node editor do.
//!
//! A drag places the card absolutely from where it was at the press, as
//! `WindowWidget`'s handles do: each edge moves only itself, and a drag
//! past the minimum stops the moving edge instead of sliding the other.
//! The gesture's event plumbing is in `hosted_events.rs`; the layer's
//! hit-test that leaves the bands to the editor is in `hosted_card.rs`.

use crate::model::{NodeGraphModel, NodeId};

use super::hosted::card_height;

/// Width (canvas units) of the band outside a card's drawn edge that
/// resizes the card: agg-sharp `WindowWidget`'s 5-unit `grabWidth`.
pub(crate) const RESIZE_GRIP: f64 = 5.0;

/// Which edge of a card a band drags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResizeEdge {
    Right,
    Top,
    Bottom,
}

/// The band holding `p`, in the coordinates of a `w` x `h` card (origin at
/// its bottom-left, Y up), if any. `vertical`: the card has top and bottom
/// bands. The drawn card itself (edges included) is never a band, and the
/// bands span the card's own extent, not its corners.
pub(crate) fn edge_at(w: f64, h: f64, vertical: bool, p: [f64; 2]) -> Option<ResizeEdge> {
    let [x, y] = p;
    let across = (0.0..=w).contains(&x);
    // Each band leaves out the drawn edge itself (`x > w`, `y > h`,
    // `y < 0`): the card, edges included, is the body's.
    if x > w && x <= w + RESIZE_GRIP && (0.0..=h).contains(&y) {
        Some(ResizeEdge::Right)
    } else if vertical && across && y > h && y <= h + RESIZE_GRIP {
        Some(ResizeEdge::Top)
    } else if vertical && across && (-RESIZE_GRIP..0.0).contains(&y) {
        Some(ResizeEdge::Bottom)
    } else {
        None
    }
}

/// An edge resize in progress: the card as it was at the press.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ResizeDrag {
    pub node: NodeId,
    pub edge: ResizeEdge,
    pub start_width: f64,
    pub start_height: f64,
    /// Canvas y of the card's top edge at the press.
    pub start_top: f64,
    pub start_canvas: [f64; 2],
    /// Least card height a top or bottom drag gives.
    pub min_height: f64,
    /// Least card width a right drag gives (the editor's
    /// `min_card_width`).
    pub min_width: f64,
    /// The card's title-bar height: a card is never shorter than its
    /// title bar and bottom pad.
    pub title_height: f64,
}

impl ResizeDrag {
    /// Size the card for the pointer at `canvas`, measured from the press.
    pub fn apply(&self, model: &mut dyn NodeGraphModel, canvas: [f64; 2]) {
        let dx = canvas[0] - self.start_canvas[0];
        let dy = canvas[1] - self.start_canvas[1];
        let min_height = self.min_height.max(card_height(0.0, self.title_height));
        match self.edge {
            ResizeEdge::Right => {
                let width = (self.start_width + dx).max(self.min_width);
                model.set_node_width(self.node, width);
            }
            ResizeEdge::Bottom => {
                let height = (self.start_height - dy).max(min_height);
                model.set_node_height(self.node, height, self.start_top);
            }
            ResizeEdge::Top => {
                // From the height actually taken, so the minimum stops the
                // top edge rather than walking the bottom.
                let height = (self.start_height + dy).max(min_height);
                let top = self.start_top - self.start_height + height;
                model.set_node_height(self.node, height, top);
            }
        }
    }
}
