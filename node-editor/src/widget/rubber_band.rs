//! Rubber-band selection on [`NodeEditor`]: in the Select interaction
//! mode a left press on empty canvas that drags past the click slop draws
//! a selection rectangle, and every card it touches is selected.
//!
//! The gesture is the [`CanvasState::RubberBand`] state; `events.rs`
//! starts it on the press, feeds it the moves and ends it on the release
//! or Escape. While it drags, `self.selected` already holds the selection
//! the release will make, so the cards show their rings as a live preview
//! (`hit_test::shows_ring` hides the host's primary-selection ring then).
//! The release only settles the primary selection. A press released
//! inside the slop is a click on empty canvas: it clears the selection
//! unless Shift was held, as it always has.
//!
//! The rectangle lives in canvas space and is painted in `finish_paint`
//! over the cards, in the palette's `selection_band_*` colours. Selecting
//! with the band raises no hosted card; simplified cards keep their usual
//! order, selected ones drawn on top.

use std::collections::HashSet;

use agg_gui::{DrawCtx, Modifiers, Point};

use crate::draw::NodeLayoutInfo;
use crate::model::NodeId;

use super::add_menu::CLICK_SLOP;
use super::{CanvasState, NodeEditor};

/// How the cards the band touches combine with the selection held at the
/// press, chosen by the modifiers held at the press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BandMode {
    /// No modifier: the touched cards become the selection.
    Replace,
    /// Shift: the touched cards are added to it.
    Add,
    /// Shift with Ctrl (Cmd on a Mac): each touched card flips in or out
    /// of it. Ctrl/Cmd alone on empty canvas pans the view (`events.rs`).
    Toggle,
}

impl BandMode {
    pub(super) fn from_modifiers(modifiers: Modifiers) -> Self {
        let command = modifiers.ctrl || agg_gui::platform::command_modifier_pressed(modifiers);
        match (modifiers.shift, command) {
            (true, true) => BandMode::Toggle,
            (true, false) => BandMode::Add,
            _ => BandMode::Replace,
        }
    }

    /// The selection for this mode from the one held at the press and the
    /// cards the band touches.
    fn combine(self, base: &HashSet<NodeId>, touched: &HashSet<NodeId>) -> HashSet<NodeId> {
        match self {
            BandMode::Replace => touched.clone(),
            BandMode::Add => base.union(touched).copied().collect(),
            BandMode::Toggle => base.symmetric_difference(touched).copied().collect(),
        }
    }
}

/// The band between two canvas corners as `[left, bottom, right, top]`.
fn band_rect(a: [f64; 2], b: [f64; 2]) -> [f64; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[0].max(b[0]),
        a[1].max(b[1]),
    ]
}

/// Whether a card's body meets the band; touching an edge is enough.
fn touches(l: &NodeLayoutInfo, band: [f64; 4]) -> bool {
    let (left, top) = (l.top_left[0], l.top_left[1]);
    let (right, bottom) = (left + l.size[0], top - l.size[1]);
    left <= band[2] && right >= band[0] && bottom <= band[3] && top >= band[1]
}

impl NodeEditor {
    /// Start the band (still a click) for a left press on empty canvas.
    pub(super) fn begin_rubber_band(
        &mut self,
        pos: Point,
        canvas_pos: [f64; 2],
        modifiers: Modifiers,
    ) {
        self.interaction = CanvasState::RubberBand {
            start_local: pos,
            start_canvas: canvas_pos,
            cursor_canvas: canvas_pos,
            mode: BandMode::from_modifiers(modifiers),
            base: self.selected.clone(),
            active: false,
        };
    }

    /// Follow the pointer: once it passes the click slop the band selects
    /// every card it touches. Returns false while the press is still a
    /// click (nothing to redraw).
    pub(super) fn drag_rubber_band(
        &mut self,
        layouts: &[NodeLayoutInfo],
        pos: Point,
        canvas_pos: [f64; 2],
    ) -> bool {
        let CanvasState::RubberBand {
            start_local,
            start_canvas,
            cursor_canvas,
            mode,
            base,
            active,
        } = &mut self.interaction
        else {
            return false;
        };
        *cursor_canvas = canvas_pos;
        if !*active {
            if (pos.x - start_local.x).hypot(pos.y - start_local.y) < CLICK_SLOP {
                return false;
            }
            *active = true;
        }
        let band = band_rect(*start_canvas, canvas_pos);
        let touched: HashSet<NodeId> = layouts
            .iter()
            .filter(|l| touches(l, band))
            .map(|l| l.node_id)
            .collect();
        self.selected = mode.combine(base, &touched);
        self.backbuffer.invalidate();
        true
    }

    /// The release: a click clears the selection (unless Shift was held);
    /// a band keeps what it selected and settles the primary selection —
    /// the current one if the band kept it, else the top-most selected
    /// card, else none — reporting it only when it changes.
    pub(super) fn finish_rubber_band(&mut self, state: CanvasState, pos: Point) {
        let CanvasState::RubberBand { active, mode, .. } = &state else {
            return;
        };
        if !*active {
            if *mode == BandMode::Replace {
                self.selected.clear();
                self.notify_primary_selection(None);
            }
            return;
        }
        // The release point ends the band even if no move reported it.
        self.interaction = state;
        let layouts = self.snapshot_layouts();
        self.drag_rubber_band(&layouts, pos, self.local_to_canvas(pos));
        self.interaction = CanvasState::Idle;
        // The host's primary selection, or the one last reported to a host
        // that keeps no copy (`primary_selection`'s default is `None`).
        let host = self.model.lock().unwrap().primary_selection();
        let current = host.or(self.reported_primary.get());
        let primary = if current.is_some_and(|id| self.selected.contains(&id)) {
            current
        } else {
            // Paint order once the band's selection is in place.
            self.snapshot_layouts()
                .iter()
                .rev()
                .find(|l| self.selected.contains(&l.node_id))
                .map(|l| l.node_id)
        };
        if primary != current {
            self.notify_primary_selection(primary);
        }
        self.backbuffer.invalidate();
        agg_gui::animation::request_draw();
    }

    /// Escape during a band: drop it and put back the selection held at
    /// the press. False when no band is in progress.
    pub(super) fn cancel_rubber_band(&mut self) -> bool {
        if !matches!(self.interaction, CanvasState::RubberBand { .. }) {
            return false;
        }
        if let CanvasState::RubberBand { base, .. } =
            std::mem::replace(&mut self.interaction, CanvasState::Idle)
        {
            self.selected = base;
        }
        self.backbuffer.invalidate();
        agg_gui::animation::request_draw();
        true
    }

    /// The band's rectangle in editor-local space, `[x, y, w, h]` from its
    /// bottom-left; `None` while no band is showing.
    pub(super) fn rubber_band_local_rect(&self) -> Option<[f64; 4]> {
        let CanvasState::RubberBand {
            start_canvas,
            cursor_canvas,
            active: true,
            ..
        } = &self.interaction
        else {
            return None;
        };
        let [left, bottom, right, top] = band_rect(*start_canvas, *cursor_canvas);
        let (s, o) = (self.canvas_scale, self.canvas_offset);
        Some([
            left * s + o[0],
            bottom * s + o[1],
            (right - left) * s,
            (top - bottom) * s,
        ])
    }

    /// Paint the band over the cards, in editor-local space.
    pub(super) fn paint_rubber_band(&self, ctx: &mut dyn DrawCtx) {
        let Some([x, y, w, h]) = self.rubber_band_local_rect() else {
            return;
        };
        ctx.set_fill_color(self.palette.selection_band_fill);
        ctx.begin_path();
        ctx.rect(x, y, w, h);
        ctx.fill();
        // A 1 px outline centred half a pixel in, so it lies on the band.
        let half = 0.5;
        ctx.set_stroke_color(self.palette.selection_band_border);
        ctx.set_line_width(2.0 * half);
        ctx.begin_path();
        ctx.rect(
            x + half,
            y + half,
            (w - 2.0 * half).max(0.0),
            (h - 2.0 * half).max(0.0),
        );
        ctx.stroke();
    }
}
