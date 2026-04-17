//! Pixel-alignment policy for widget bounds and draw-time translation.
//!
//! # Port of MatterCAD / agg-sharp
//!
//! `GuiWidget.DefaultEnforceIntegerBounds` (static) controls whether widgets
//! round their bounds / padding / margin to the physical pixel grid, and is
//! then mirrored on each widget as `EnforceIntegerBounds` so individual
//! widgets can opt out (e.g. a smooth-scrolling marker or a zoomed canvas
//! that genuinely wants sub-pixel positioning).
//!
//! We default to **true** because the vast majority of UI widgets want crisp
//! text and strokes — fractional bounds are the exception, not the rule.
//!
//! # Read site
//!
//! `paint_subtree` reads the effective flag (widget's override, falling back
//! to the global default) and rounds the child-translation to the nearest
//! integer pixel before calling the child's `paint`.  That single snap kills
//! fractional CTM accumulated by cumulative-heights flex layout (e.g. Label
//! `line_h = font_size × 1.5` is fractional for most font sizes), which is
//! what caused the Y-axis pixel fringe on crisp rectangle fills downstream.
//!
//! # Opt-out
//!
//! ```ignore
//! // Globally disable (rare — only for fully sub-pixel render targets):
//! agg_gui::pixel_bounds::set_default_enforce_integer_bounds(false);
//!
//! // Per-widget:
//! my_widget.widget_base_mut().enforce_integer_bounds = false;
//! ```

use std::sync::atomic::{AtomicBool, Ordering};

/// Storage for the process-wide default.  Reads use `Relaxed` — the flag is
/// consulted once per paint; there are no cross-thread ordering requirements
/// beyond "eventually sees the latest write".
static DEFAULT_ENFORCE_INTEGER_BOUNDS: AtomicBool = AtomicBool::new(true);

/// Current process-wide default used to initialise each new widget's
/// `enforce_integer_bounds` field.
pub fn default_enforce_integer_bounds() -> bool {
    DEFAULT_ENFORCE_INTEGER_BOUNDS.load(Ordering::Relaxed)
}

/// Change the process-wide default.  Only affects widgets constructed
/// *after* this call; existing widgets keep whichever value they captured
/// when they were built.  Match MatterCAD semantics (`DefaultEnforceIntegerBounds`
/// setter) exactly.
pub fn set_default_enforce_integer_bounds(enforce: bool) {
    DEFAULT_ENFORCE_INTEGER_BOUNDS.store(enforce, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default must be `true` so the common case — crisp UI text + strokes
    /// — works out of the box.
    #[test]
    fn test_default_is_enforced() {
        // Not order-independent with other tests, so capture then restore.
        let prior = default_enforce_integer_bounds();
        set_default_enforce_integer_bounds(true);
        assert!(default_enforce_integer_bounds());
        set_default_enforce_integer_bounds(prior);
    }

    #[test]
    fn test_setter_round_trip() {
        let prior = default_enforce_integer_bounds();
        set_default_enforce_integer_bounds(false);
        assert!(!default_enforce_integer_bounds());
        set_default_enforce_integer_bounds(true);
        assert!(default_enforce_integer_bounds());
        set_default_enforce_integer_bounds(prior);
    }
}
