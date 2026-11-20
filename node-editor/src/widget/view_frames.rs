//! View-only frames: the frames of a pan or zoom, which change nothing but
//! the canvas transform.
//!
//! A frame's layout runs whenever agg-gui's invalidation epoch moved, and a
//! pan or zoom step has to move it: the host's widgets placed over cards
//! (MatterCAD's Open buttons and rename box) follow the view in their own
//! layout. But laying the hosted card bodies out again, reading every node
//! and socket from the model again, and re-rasterising every card for a
//! view that only slid or scaled cost a frame tens of times over budget.
//!
//! So the editor knows when a frame is view-only: every draw request since
//! its last layout was its own view change ([`NodeEditor::request_view_draw`],
//! made by the pan and zoom gestures, the wheel and the fit tween), or there
//! was none and the frame is the one a settled zoom asked for to draw the
//! cards crisp again, and the
//! editor's size, command queue, collapse channel, pending centring and
//! overlay are as they were. Such a layout ([`NodeEditor::layout_view_only`])
//! only hands the hosted layer the new view; the cards keep their bodies,
//! measured geometry and cached layers (`hosted_card.rs`), so a pan paints by
//! compositing each card's layer at its new place. Any other draw request in
//! between — an edit, a hover, a model change anywhere in the app — makes the
//! next layout a full one, which re-lays everything out and invalidates every
//! card's layer as before ([`NodeEditor::invalidate_canvas`]). A zoom
//! composites the cards' layers stretched while it moves and rasterises them
//! again, crisp, once it settles (`hosted_card.rs`). The canvas's own paint
//! reuses its model read across such frames (`paint_cache.rs`).
//!
//! Only a hosted-card editor takes the short layout: a simplified card
//! ([`super::nodes`]) bakes the view into its bounds and is rebuilt by the
//! full one.

use agg_gui::animation::{invalidation_epoch, request_draw};
use agg_gui::Size;

use super::hosted_card::LAYER_TYPE_NAME;
use super::NodeEditor;

/// What the editor remembers to tell a view-only frame from any other.
#[derive(Default)]
pub(crate) struct ViewFrames {
    /// The invalidation epoch when the last layout ended.
    laid_out_at: Option<u64>,
    /// The epoch right after the latest of the editor's own view-change
    /// requests, while every request since the last layout was one.
    chain: Option<u64>,
    /// `layout()` is running: a view request made there (the fit tween)
    /// starts a chain whatever came before, since this layout handles it.
    in_layout: bool,
    /// The size of the last layout.
    size: Option<Size>,
}

impl NodeEditor {
    /// Ask for the frame a change of the view alone needs: the canvas is
    /// drawn again (the editor's own layer), the cards' layers are kept, and
    /// the next layout may be view-only.
    pub(super) fn request_view_draw(&mut self) {
        let before = invalidation_epoch();
        let continues = self.view_frames.in_layout
            || Some(before) == self.view_frames.laid_out_at
            || Some(before) == self.view_frames.chain;
        request_draw();
        self.view_frames.chain = continues.then(invalidation_epoch);
        self.backbuffer.invalidate();
    }

    /// Draw the whole canvas again: the editor's layer and every hosted
    /// card's, from a fresh read of the model (`paint_cache.rs`). For
    /// anything but a change of the view alone.
    pub(crate) fn invalidate_canvas(&mut self) {
        self.backbuffer.invalidate();
        self.paint_cache.generation = self.paint_cache.generation.wrapping_add(1);
        if let Some(layer) = super::hosted::layer_in(&mut self.children) {
            layer.invalidate_cards();
        }
    }

    /// Whether the layout at `available` is view-only (see the module docs).
    fn is_view_only(&self, available: Size) -> bool {
        let epoch = invalidation_epoch();
        // The editor's own view requests alone since the last layout, or none at all on the
        // frame the stretched cards settle on (laid out because the layer asked to be drawn).
        let settling = self.view_frames.laid_out_at == Some(epoch)
            && super::hosted::layer_in_ref(&self.children).is_some_and(|l| l.settle_due());
        let only_view = self.view_frames.chain == Some(epoch) || settling;
        let hosted_only = self.children.len() == 1
            && self.children[0].type_name() == LAYER_TYPE_NAME
            && self.hosted.factory.is_some();
        only_view
            && hosted_only
            && self.view_frames.size == Some(available)
            && self.pending_collapse.get().is_none()
            && !self.view.center_pending
            && self.overlay.is_none()
            && self.hosted.resize.is_none()
            && self
                .command_handle
                .as_ref()
                .is_none_or(|handle| handle.is_empty())
    }

    /// Start of `layout()`: whether this layout is view-only. From here
    /// until [`Self::end_layout`] a view request starts a fresh chain.
    pub(super) fn begin_layout(&mut self, available: Size) -> bool {
        let view_only = self.is_view_only(available);
        self.view_frames.chain = None;
        self.view_frames.in_layout = true;
        view_only
    }

    /// End of `layout()`.
    pub(super) fn end_layout(&mut self, available: Size) {
        self.view_frames.in_layout = false;
        self.view_frames.size = Some(available);
        self.view_frames.laid_out_at = Some(invalidation_epoch());
    }

    /// The view-only layout: tick the fit tween, and hand the hosted layer
    /// the view. Everything else stays as the last full layout left it.
    pub(super) fn layout_view_only(&mut self, available: Size) {
        self.tick_view_animation();
        let (offset, scale) = (self.canvas_offset, self.canvas_scale);
        if let Some(layer) = super::hosted::layer_in(&mut self.children) {
            layer.set_view(available, offset, scale);
        }
    }
}
