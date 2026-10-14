//! The canvas origin's axes for [`NodeEditor`]: a red line along canvas
//! y = 0 and a green one along x = 0, each 1 unit wide and running the full
//! width or height of the editor (MatterCAD's `NodeEditor.OnDraw`, through
//! `PathEditorAxisLines`). Opt-in with [`NodeEditor::with_axes`].
//!
//! The segments are worked out in editor-local space, clipped to the
//! editor, rather than stroked as huge canvas-space lines: at a large zoom
//! such endpoints would sit far outside the widget, past what a stroke can
//! carry (the reason MatterCAD's `PathEditorAxisLines` exists). `paint.rs`
//! draws them over the canvas background and grid, under the noodles and
//! cards, as MatterCAD draws them before its children.

use agg_gui::{Color, DrawCtx, LineCap};

use super::NodeEditor;

/// The x axis's colour (agg-sharp's `Color.Red`).
pub const AXIS_X_COLOR: Color = Color::rgb(1.0, 0.0, 0.0);
/// The y axis's colour (agg-sharp's `Color.Green`).
pub const AXIS_Y_COLOR: Color = Color::rgb(0.0, 1.0, 0.0);
/// The axes' width, in logical units (agg-sharp's `Graphics2D.Line`
/// default: one pixel times the device scale), with agg's butt caps.
pub const AXIS_WIDTH: f64 = 1.0;

/// A horizontal and a vertical segment, each `[start, end]` in editor-local
/// space, `None` where the axis is scrolled off the editor.
type AxisSegments = (Option<[[f64; 2]; 2]>, Option<[[f64; 2]; 2]>);

/// Where the canvas origin's axes cross a `w` x `h` editor whose canvas
/// origin sits at `origin` (editor-local): the y = 0 line from the left edge
/// to the right, the x = 0 line from the bottom edge to the top. An axis
/// lying exactly on an edge still draws (`PathEditorAxisLines`' inclusive
/// test).
pub(super) fn axis_segments(w: f64, h: f64, origin: [f64; 2]) -> AxisSegments {
    let [ox, oy] = origin;
    let horizontal = (0.0..=h).contains(&oy).then_some([[0.0, oy], [w, oy]]);
    let vertical = (0.0..=w).contains(&ox).then_some([[ox, 0.0], [ox, h]]);
    (horizontal, vertical)
}

impl NodeEditor {
    /// Draw a red x axis and a green y axis through the canvas origin
    /// (MatterCAD's node editor does). Off by default.
    pub fn with_axes(mut self, enabled: bool) -> Self {
        self.presentation.axes = enabled;
        self
    }

    /// Whether the origin's axes are drawn (see [`Self::with_axes`]).
    pub fn axes(&self) -> bool {
        self.presentation.axes
    }

    /// Stroke the axes in editor-local space (the canvas transform popped).
    pub(super) fn paint_axes(&self, ctx: &mut dyn DrawCtx) {
        if !self.presentation.axes {
            return;
        }
        let (horizontal, vertical) =
            axis_segments(self.bounds.width, self.bounds.height, self.canvas_offset);
        ctx.save();
        ctx.set_line_width(AXIS_WIDTH);
        ctx.set_line_cap(LineCap::Butt);
        for (segment, color) in [(horizontal, AXIS_X_COLOR), (vertical, AXIS_Y_COLOR)] {
            if let Some([a, b]) = segment {
                ctx.set_stroke_color(color);
                ctx.begin_path();
                ctx.move_to(a[0], a[1]);
                ctx.line_to(b[0], b[1]);
                ctx.stroke();
            }
        }
        ctx.restore();
    }
}
