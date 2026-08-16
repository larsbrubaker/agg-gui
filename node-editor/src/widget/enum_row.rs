//! Enum property rows on the canvas — the click half.
//!
//! The paint half lives in agg-gui's `property_row` renderer (a
//! segmented strip, one button per variant). This module maps a canvas
//! click onto one of those segments and commits it, so `events.rs` keeps
//! only the dispatch line and stays under the 800-line guardrail.
//!
//! Geometry is *not* recomputed here: it comes from
//! [`agg_gui::widgets::enum_variant_at`], the same function the renderer
//! paints with. If the two were computed separately, the segment the
//! user clicked and the segment that lights up would drift apart the
//! first time either side was tweaked.

use agg_gui::EventResult;

use crate::draw::PropLayout;
use crate::model::{NodeId, PropertyValue};

use super::NodeEditor;

impl NodeEditor {
    /// Handle a click on an enum property row.
    ///
    /// Returns `None` **only** when the row is not an enum row at all;
    /// an enum row always consumes its own clicks, even where the strip
    /// does not reach. "Which row is this?" and "which segment did the
    /// pointer land on?" are separate questions, and answering them
    /// together is what let a click on the row's label fall through to
    /// the free-text editor — where the user could type a value outside
    /// the variant set, which then silently evaluates as the default.
    /// So a miss inside the row (label zone, right pad, gap) is a
    /// consumed no-op: it selects the node and changes nothing.
    ///
    /// Interaction is click-only today. Keyboard variant cycling
    /// (arrow keys on a focused row) needs row focus, which the canvas
    /// does not model yet — the whole property-row layer is painted from
    /// a retained snapshot rather than mounted as focusable widgets.
    pub(super) fn handle_enum_row_click(
        &mut self,
        prop: &PropLayout,
        node_id: NodeId,
        x: f64,
    ) -> Option<EventResult> {
        let kind = prop.editor_kind.as_ref()?;
        let variants = kind.enum_variants()?;
        self.selected.clear();
        self.selected.insert(node_id);
        self.notify_primary_selection(Some(node_id));

        if let Some(variant) = variant_at(prop, kind, variants, x) {
            self.model.lock().unwrap().set_property(
                node_id,
                &prop.name,
                PropertyValue::Text(variant),
            );
            // Same reasoning as the toggle row: the choice can change
            // which rows are visible, not just this row's paint.
            agg_gui::animation::request_draw();
        }
        Some(EventResult::Consumed)
    }
}

/// The variant a click at canvas-space `x` selected on `prop`, or `None`
/// when the click landed inside the row but outside the strip.
///
/// Scale is 1.0 because `prop`'s rect is in **canvas** space: zoom
/// scales the whole canvas uniformly, so the renderer's `8.0 * zoom`
/// screen-px padding is 8.0 canvas units at any zoom.
fn variant_at(
    prop: &PropLayout,
    kind: &agg_gui::widgets::EditorKind,
    variants: &[std::sync::Arc<str>],
    x: f64,
) -> Option<String> {
    let rect = agg_gui::Rect::new(
        prop.top_left[0],
        prop.top_left[1] - prop.size[1],
        prop.size[0],
        prop.size[1],
    );
    let has_label = prop.full_row && !prop.label().is_empty();
    let idx = agg_gui::widgets::enum_variant_at(rect, has_label, kind, x, 1.0)?;
    variants.get(idx).map(|v| v.as_ref().to_string())
}
