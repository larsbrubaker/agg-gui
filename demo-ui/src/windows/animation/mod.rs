//! Animation demo windows: interactive Bézier curve editor, animated dancing
//! sine waves, and a freehand painting canvas.
//!
//! The three demos each live in their own submodule but share a common style:
//! custom `Widget` implementations with direct `DrawCtx` calls — no layout
//! children for the canvases, just raw path drawing — to show what is possible
//! beyond the standard widget palette.
//!
//! - [`bezier`]   — the "Bézier Curve" demo (`paint_bezier.rs` in egui).
//! - [`dancing`]  — the "Dancing Strings" demo (`dancing_strings.rs` in egui).
//! - [`painting`] — the "Painting" demo (`painting.rs` in egui).
//!
//! Coordinate system: Y-up throughout (origin bottom-left, positive Y upward),
//! matching the agg-gui invariant.

mod bezier;
mod dancing;
mod painting;

pub use bezier::bezier_curve;
pub use dancing::dancing_strings;
pub use painting::painting;
