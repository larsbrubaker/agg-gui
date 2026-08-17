//! Tests for the per-node error badge on the **live** paint path.
//!
//! The canvas does not render through `crate::draw::draw_node`; it builds
//! a retained tree of `NodeWidget` / `NodeHeaderWidget` children in
//! `rebuild_children` and paints those. So these tests seed a real
//! `NodeEditor`, run its real layout, and paint the node subtree the
//! framework would paint — anything asserted here is on the path the
//! user sees.
//!
//! (The node subtree is painted directly rather than through the whole
//! editor because `NodeEditor` caches its subtree into a CPU backbuffer
//! and blits it, which would hide every individual draw call from the
//! recorder. Everything above the node widget is unchanged by this
//! feature.)

use super::tests_common::{fixture_with_typed_handle, mk_node, seed_nodes};
use super::*;

use agg_gui::draw_ctx::{LinearGradientPaint, RadialGradientPaint};
use agg_gui::{Color, CompOp, FillRule, LineCap, LineJoin, TextMetrics, TransAffine};

const MESSAGE: &str = "input 'b' is not a closed solid";

/// Records the shapes the badge is made of, and the colour each was
/// drawn in.
#[derive(Default)]
struct BadgeRecorder {
    fill_color: Color,
    stroke_color: Color,
    line_width: f64,
    filled_circles: Vec<([f64; 2], f64, Color)>,
    strokes: Vec<(Color, f64)>,
    pending_circle: Option<([f64; 2], f64)>,
}

impl BadgeRecorder {
    /// True when some circle of `radius` was filled in `color`.
    fn filled_circle(&self, radius: f64, color: Color) -> bool {
        self.filled_circles
            .iter()
            .any(|(_, r, c)| (*r - radius).abs() < 1e-9 && same(*c, color))
    }
    fn stroked(&self, color: Color, width: f64) -> bool {
        self.strokes
            .iter()
            .any(|(c, w)| same(*c, color) && (*w - width).abs() < 1e-9)
    }
}

fn same(a: Color, b: Color) -> bool {
    (a.r - b.r).abs() < 1e-6 && (a.g - b.g).abs() < 1e-6 && (a.b - b.b).abs() < 1e-6
}

impl DrawCtx for BadgeRecorder {
    fn set_fill_color(&mut self, color: Color) {
        self.fill_color = color;
    }
    fn set_stroke_color(&mut self, color: Color) {
        self.stroke_color = color;
    }
    fn set_fill_linear_gradient(&mut self, _g: LinearGradientPaint) {}
    fn set_fill_radial_gradient(&mut self, _g: RadialGradientPaint) {}
    fn set_line_width(&mut self, w: f64) {
        self.line_width = w;
    }
    fn set_line_join(&mut self, _join: LineJoin) {}
    fn set_line_cap(&mut self, _cap: LineCap) {}
    fn set_miter_limit(&mut self, _limit: f64) {}
    fn set_line_dash(&mut self, _dashes: &[f64], _offset: f64) {}
    fn set_blend_mode(&mut self, _mode: CompOp) {}
    fn set_global_alpha(&mut self, _alpha: f64) {}
    fn set_fill_rule(&mut self, _rule: FillRule) {}
    fn set_font(&mut self, _font: std::sync::Arc<agg_gui::Font>) {}
    fn set_font_size(&mut self, _size: f64) {}
    fn clip_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    fn reset_clip(&mut self) {}
    fn clear(&mut self, _color: Color) {}
    fn begin_path(&mut self) {
        self.pending_circle = None;
    }
    fn move_to(&mut self, _x: f64, _y: f64) {}
    fn line_to(&mut self, _x: f64, _y: f64) {}
    fn cubic_to(&mut self, _a: f64, _b: f64, _c: f64, _d: f64, _e: f64, _f: f64) {}
    fn quad_to(&mut self, _cx: f64, _cy: f64, _x: f64, _y: f64) {}
    fn arc_to(&mut self, _cx: f64, _cy: f64, _r: f64, _s: f64, _e: f64, _ccw: bool) {}
    fn circle(&mut self, cx: f64, cy: f64, r: f64) {
        self.pending_circle = Some(([cx, cy], r));
    }
    fn rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    fn rounded_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64, _r: f64) {}
    fn close_path(&mut self) {}
    fn fill(&mut self) {
        if let Some((c, r)) = self.pending_circle.take() {
            self.filled_circles.push((c, r, self.fill_color));
        }
    }
    fn stroke(&mut self) {
        self.strokes.push((self.stroke_color, self.line_width));
    }
    fn fill_and_stroke(&mut self) {}
    fn draw_triangles_aa(&mut self, _v: &[[f32; 3]], _i: &[u32], _c: Color) {}
    fn fill_text(&mut self, _text: &str, _x: f64, _y: f64) {}
    fn fill_text_gsv(&mut self, _text: &str, _x: f64, _y: f64, _size: f64) {}
    fn measure_text(&self, _text: &str) -> Option<TextMetrics> {
        None
    }
    fn transform(&self) -> TransAffine {
        TransAffine::new()
    }
    fn set_transform(&mut self, _m: TransAffine) {}
    fn reset_transform(&mut self) {}
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _tx: f64, _ty: f64) {}
    fn rotate(&mut self, _radians: f64) {}
    fn scale(&mut self, _sx: f64, _sy: f64) {}
}

/// Lay out a real editor over one node and paint the node subtree the
/// canvas built for it.
fn paint_one_node(error: Option<&str>) -> BadgeRecorder {
    paint_node(error, None)
}

/// Same, with both severities under the caller's control.
fn paint_node(error: Option<&str>, warning: Option<&str>) -> BadgeRecorder {
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    let mut node = mk_node(1, "Boolean", [20.0, 200.0]);
    node.error = error.map(|s| s.to_string());
    node.warning = warning.map(|s| s.to_string());
    seed_nodes(&mut editor, &memory, vec![node]);

    let mut recorder = BadgeRecorder::default();
    let node_widget = editor
        .children_mut()
        .iter_mut()
        .find(|c| c.type_name() == "NodeWidget")
        .expect("the canvas built a NodeWidget for the seeded node");
    agg_gui::widget::paint_subtree(node_widget.as_mut(), &mut recorder);
    recorder
}

/// The node the canvas actually built carries the host's message, so the
/// inspector (and the paint pass) can see it.
#[test]
fn the_canvas_node_widget_carries_the_hosts_error() {
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    let mut node = mk_node(1, "Boolean", [20.0, 200.0]);
    node.error = Some(MESSAGE.to_string());
    seed_nodes(&mut editor, &memory, vec![node]);

    let widget = editor
        .children()
        .iter()
        .find(|c| c.type_name() == "NodeWidget")
        .expect("a NodeWidget for the seeded node");
    assert!(
        widget
            .properties()
            .iter()
            .any(|(k, v)| *k == "error" && v == MESSAGE),
        "the retained node widget carries the message"
    );
}

/// Painting that widget draws the badge and the error outline.
#[test]
fn a_failed_node_paints_a_badge_and_an_error_outline() {
    let palette = CanvasPalette::from_visuals(&agg_gui::theme::current_visuals());
    let recorder = paint_one_node(Some(MESSAGE));

    assert!(
        recorder.filled_circle(crate::draw_error::ERROR_BADGE_RADIUS, palette.node_error),
        "the `!` badge is filled in the error colour"
    );
    assert!(
        recorder.stroked(palette.node_error, crate::draw_error::ERROR_OUTLINE_WIDTH),
        "the body is re-outlined in the error colour"
    );
}

/// A healthy node paints neither.
#[test]
fn a_healthy_node_paints_no_error_chrome() {
    let palette = CanvasPalette::from_visuals(&agg_gui::theme::current_visuals());
    let recorder = paint_one_node(None);

    assert!(!recorder.filled_circle(crate::draw_error::ERROR_BADGE_RADIUS, palette.node_error));
    assert!(!recorder.stroked(palette.node_error, crate::draw_error::ERROR_OUTLINE_WIDTH));
}

/// The badge arrives from an *asynchronous* host evaluation and changes
/// nothing else about the layout, so the paint fingerprint has to notice
/// it — otherwise the badge waits for the next unrelated interaction.
#[test]
fn an_error_changes_the_paint_fingerprint() {
    let (model, _memory) = fixture_with_typed_handle();
    let editor = NodeEditor::new(model);

    let healthy = crate::draw::layout_node(&mk_node(1, "Boolean", [20.0, 200.0]));
    let broken = {
        let mut n = mk_node(1, "Boolean", [20.0, 200.0]);
        n.error = Some(MESSAGE.to_string());
        crate::draw::layout_node(&n)
    };

    assert_ne!(
        editor.compute_fingerprint(std::slice::from_ref(&healthy), None),
        editor.compute_fingerprint(std::slice::from_ref(&broken), None),
        "an error must invalidate the cached child tree"
    );
}

// ---------------------------------------------------------------------------
// Warning severity — the same chrome in the palette's amber
// ---------------------------------------------------------------------------

const DEGRADED: &str = "2 of 5 parts are not watertight solids";

/// A node that produced degraded-but-usable output wears the badge in
/// the warning colour, not the error colour.
#[test]
fn a_degraded_node_paints_an_amber_badge_and_outline() {
    let palette = CanvasPalette::from_visuals(&agg_gui::theme::current_visuals());
    let recorder = paint_node(None, Some(DEGRADED));

    assert!(
        recorder.filled_circle(crate::draw_error::ERROR_BADGE_RADIUS, palette.node_warning),
        "the `!` badge is filled in the warning colour"
    );
    assert!(
        recorder.stroked(palette.node_warning, crate::draw_error::ERROR_OUTLINE_WIDTH),
        "the body is re-outlined in the warning colour"
    );
    assert!(
        !recorder.filled_circle(crate::draw_error::ERROR_BADGE_RADIUS, palette.node_error),
        "and never in the error colour"
    );
}

/// Only one badge fits on a title bar, so the error wins when a node
/// carries both.
#[test]
fn an_error_beats_a_warning_on_the_same_node() {
    let palette = CanvasPalette::from_visuals(&agg_gui::theme::current_visuals());
    let recorder = paint_node(Some(MESSAGE), Some(DEGRADED));

    assert!(recorder.filled_circle(crate::draw_error::ERROR_BADGE_RADIUS, palette.node_error));
    assert!(recorder.stroked(palette.node_error, crate::draw_error::ERROR_OUTLINE_WIDTH));
    assert!(
        !recorder.filled_circle(crate::draw_error::ERROR_BADGE_RADIUS, palette.node_warning),
        "the warning colour never appears while an error is present"
    );
}

/// The widget carries the warning text too, so hosts (and the F12
/// inspector) can read which message produced the badge.
#[test]
fn the_canvas_node_widget_carries_the_hosts_warning() {
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    let mut node = mk_node(1, "Boolean", [20.0, 200.0]);
    node.warning = Some(DEGRADED.to_string());
    seed_nodes(&mut editor, &memory, vec![node]);

    let widget = editor
        .children()
        .iter()
        .find(|c| c.type_name() == "NodeWidget")
        .expect("a NodeWidget for the seeded node");
    assert!(widget
        .properties()
        .iter()
        .any(|(k, v)| *k == "warning" && v == DEGRADED));
}

/// Same async story as the error badge: nothing else about the layout
/// changes when a warning arrives, so the fingerprint has to notice.
#[test]
fn a_warning_changes_the_paint_fingerprint() {
    let (model, _memory) = fixture_with_typed_handle();
    let editor = NodeEditor::new(model);

    let healthy = crate::draw::layout_node(&mk_node(1, "Boolean", [20.0, 200.0]));
    let degraded = {
        let mut n = mk_node(1, "Boolean", [20.0, 200.0]);
        n.warning = Some(DEGRADED.to_string());
        crate::draw::layout_node(&n)
    };

    assert_ne!(
        editor.compute_fingerprint(std::slice::from_ref(&healthy), None),
        editor.compute_fingerprint(std::slice::from_ref(&degraded), None),
        "a warning must invalidate the cached child tree"
    );
}
