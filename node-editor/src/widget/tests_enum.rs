//! Tests for enum property rows on the canvas — the segmented strip
//! that MatterCAD renders as an `[EnumDisplay]` row.
//!
//! Two halves, both on the live path:
//!
//!   * **Interaction** — a click routed through the real
//!     `NodeEditor::on_mouse_down` must commit the variant under the
//!     pointer (and must *not* fall through to the free-text editor,
//!     which would let the user store a value outside the variant set).
//!   * **Paint** — painting the node subtree the canvas actually built
//!     draws the strip: one accent-filled segment for the current value
//!     and the variant labels beside it.

use super::tests_common::{fixture_with_typed_handle, mk_node, seed_nodes};
use super::*;

use crate::model::{NodeId, PropertyValue, PropertyView};
use agg_gui::draw_ctx::{LinearGradientPaint, RadialGradientPaint};
use agg_gui::widgets::EditorKind;
use agg_gui::{
    Color, CompOp, FillRule, LineCap, LineJoin, Modifiers, MouseButton, Point, TextMetrics,
    TransAffine,
};

const VARIANTS: [&str; 4] = ["Combine", "Subtract", "Intersect", "Subtract & Replace"];

fn enum_kind() -> EditorKind {
    EditorKind::EnumButtons {
        variants: VARIANTS.iter().map(|v| std::sync::Arc::from(*v)).collect(),
    }
}

/// A node carrying one enum property named `operation`, rendered with
/// whichever enum presentation the caller asks for.
fn mk_enum_node_with(current: &str, kind: EditorKind) -> crate::model::NodeView {
    let mut n = mk_node(1, "Boolean", [40.0, 200.0]);
    n.properties.push(PropertyView {
        name: "operation".to_string(),
        display_label: Some("Operation".to_string()),
        current: PropertyValue::Text(current.to_string()),
        min: None,
        max: None,
        bound_input: None,
        editor: None,
        editor_kind: Some(kind),
    });
    n
}

fn editor_with_kind(
    current: &str,
    kind: EditorKind,
) -> (NodeEditor, Arc<Mutex<super::tests_common::Memory>>) {
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_nodes(&mut editor, &memory, vec![mk_enum_node_with(current, kind)]);
    (editor, memory)
}

fn editor_with_enum_node(current: &str) -> (NodeEditor, Arc<Mutex<super::tests_common::Memory>>) {
    editor_with_kind(current, enum_kind())
}

/// The enum row's canvas-space layout.
fn enum_row(editor: &NodeEditor) -> crate::draw::PropLayout {
    let layouts = editor.snapshot_layouts();
    let l = layouts
        .iter()
        .find(|l| l.node_id == NodeId(1))
        .expect("the seeded node has a layout");
    let found = l
        .props()
        .find(|p| p.name == "operation")
        .expect("the node has an enum row")
        .clone();
    found
}

/// Widget-space point at canvas-space `x`, vertically centred on the
/// enum row.
fn row_point_at_x(editor: &NodeEditor, x: f64) -> Point {
    let p = enum_row(editor);
    let cy = p.top_left[1] - p.size[1] * 0.5;
    Point::new(
        x * editor.canvas_scale + editor.canvas_offset[0],
        cy * editor.canvas_scale + editor.canvas_offset[1],
    )
}

/// Widget-space point inside segment `idx` of the node's enum row.
///
/// The x is found by *inverting* the renderer's own hit-test rather than
/// re-deriving the 45 % split and the 8 px pad here: a duplicate of that
/// arithmetic in the test would keep passing if the renderer's geometry
/// and the canvas's hit-test drifted apart, which is the exact bug this
/// file exists to catch.
fn segment_point(editor: &NodeEditor, idx: usize) -> Point {
    let p = enum_row(editor);
    let kind = p.editor_kind.clone().expect("the enum row carries a kind");
    let rect = agg_gui::Rect::new(
        p.top_left[0],
        p.top_left[1] - p.size[1],
        p.size[0],
        p.size[1],
    );
    let has_label = p.full_row && !p.label().is_empty();
    // Step across the row and take the first x the renderer maps to
    // `idx`; a quarter-pixel step is far finer than any segment.
    let mut x = rect.x;
    while x < rect.x + rect.width {
        if agg_gui::widgets::enum_variant_at(rect, has_label, &kind, x, 1.0) == Some(idx) {
            // Aim at the middle of that segment, not its left edge.
            let mut end = x;
            while end < rect.x + rect.width
                && agg_gui::widgets::enum_variant_at(rect, has_label, &kind, end, 1.0) == Some(idx)
            {
                end += 0.25;
            }
            return row_point_at_x(editor, (x + end) * 0.5);
        }
        x += 0.25;
    }
    panic!("the renderer maps no x on the row to segment {idx}");
}

fn committed(memory: &Arc<Mutex<super::tests_common::Memory>>) -> Option<(String, String)> {
    memory
        .lock()
        .unwrap()
        .last_property
        .as_ref()
        .and_then(|(_, name, v)| match v {
            PropertyValue::Text(s) => Some((name.clone(), s.clone())),
            _ => None,
        })
}

#[test]
fn clicking_a_segment_commits_that_variant() {
    for (idx, want) in VARIANTS.iter().enumerate() {
        let (mut editor, memory) = editor_with_enum_node("Combine");
        let p = segment_point(&editor, idx);
        editor.on_mouse_down(p, MouseButton::Left, Modifiers::default());
        assert_eq!(
            committed(&memory),
            Some(("operation".to_string(), want.to_string())),
            "clicking segment {} should commit {}",
            idx,
            want
        );
    }
}

/// The click is consumed by the row, and it selects the node the way
/// every other property interaction does.
#[test]
fn clicking_a_segment_consumes_the_event_and_selects_the_node() {
    let (mut editor, memory) = editor_with_enum_node("Combine");
    let p = segment_point(&editor, 2);
    let result = editor.on_mouse_down(p, MouseButton::Left, Modifiers::default());
    assert_eq!(result, EventResult::Consumed);
    assert_eq!(memory.lock().unwrap().last_selection, Some(NodeId(1)));
}

/// An enum value is a string, so without the enum branch the click would
/// land in the free-text editor and let the user type anything. The row
/// must never open one.
#[test]
fn clicking_an_enum_row_does_not_open_the_text_editor() {
    super::tests_common::install_test_font_once();
    let (mut editor, _memory) = editor_with_enum_node("Combine");
    let p = segment_point(&editor, 1);
    editor.on_mouse_down(p, MouseButton::Left, Modifiers::default());
    assert!(
        editor.overlay.is_none(),
        "an enum row opened an inline text editor"
    );
}

/// …and that includes the parts of the row the strip does *not* cover.
/// The label zone (left 45 %) and the right-hand pad are still an enum
/// row: falling through to the free-text editor there would let the user
/// type a value outside the variant set — which then evaluates as the
/// default and silently loses their choice.
#[test]
fn clicking_the_label_zone_of_an_enum_row_neither_edits_nor_commits() {
    super::tests_common::install_test_font_once();
    for offset in [5.0, 20.0] {
        let (mut editor, memory) = editor_with_enum_node("Combine");
        let x = enum_row(&editor).top_left[0] + offset;
        let p = row_point_at_x(&editor, x);
        let result = editor.on_mouse_down(p, MouseButton::Left, Modifiers::default());

        assert!(
            editor.overlay.is_none(),
            "clicking {}px into the label zone opened a text editor",
            offset
        );
        assert_eq!(
            committed(&memory),
            None,
            "clicking the label zone committed a value"
        );
        assert_eq!(
            result,
            EventResult::Consumed,
            "an enum row must consume its own clicks"
        );
    }
}

/// The right-hand pad past the last segment behaves the same way.
#[test]
fn clicking_past_the_last_segment_neither_edits_nor_commits() {
    super::tests_common::install_test_font_once();
    let (mut editor, memory) = editor_with_enum_node("Combine");
    let row = enum_row(&editor);
    let p = row_point_at_x(&editor, row.top_left[0] + row.size[0] - 1.0);
    editor.on_mouse_down(p, MouseButton::Left, Modifiers::default());

    assert!(
        editor.overlay.is_none(),
        "the right pad opened a text editor"
    );
    assert_eq!(committed(&memory), None);
}

// ------------------------------------------------------------- paint

/// Records fills and text so the strip's segments and labels can be
/// asserted after a real subtree paint.
#[derive(Default)]
struct StripRecorder {
    fill_color: Color,
    filled_rects: Vec<([f64; 4], Color)>,
    /// Filled free-form paths: `(bounding box, colour, point count)`.
    /// Icons arrive this way — as polygons, not as rects.
    filled_polys: Vec<([f64; 4], Color, usize)>,
    texts: Vec<String>,
    pending: Option<[f64; 4]>,
    poly: Vec<[f64; 2]>,
}

impl StripRecorder {
    fn filled_in(&self, color: Color) -> usize {
        self.filled_rects
            .iter()
            .filter(|(_, c)| {
                (c.r - color.r).abs() < 1e-6
                    && (c.g - color.g).abs() < 1e-6
                    && (c.b - color.b).abs() < 1e-6
            })
            .count()
    }

    /// Polygon fills whose colour matches `color`, in paint order.
    fn polys_in(&self, color: Color) -> Vec<([f64; 4], usize)> {
        self.filled_polys
            .iter()
            .filter(|(_, c, _)| {
                (c.r - color.r).abs() < 1e-6
                    && (c.g - color.g).abs() < 1e-6
                    && (c.b - color.b).abs() < 1e-6
            })
            .map(|(b, _, n)| (*b, *n))
            .collect()
    }
}

impl DrawCtx for StripRecorder {
    fn set_fill_color(&mut self, color: Color) {
        self.fill_color = color;
    }
    fn set_stroke_color(&mut self, _color: Color) {}
    fn set_fill_linear_gradient(&mut self, _g: LinearGradientPaint) {}
    fn set_fill_radial_gradient(&mut self, _g: RadialGradientPaint) {}
    fn set_line_width(&mut self, _w: f64) {}
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
        self.pending = None;
        self.poly.clear();
    }
    fn move_to(&mut self, x: f64, y: f64) {
        self.poly.push([x, y]);
    }
    fn line_to(&mut self, x: f64, y: f64) {
        self.poly.push([x, y]);
    }
    fn cubic_to(&mut self, _a: f64, _b: f64, _c: f64, _d: f64, _e: f64, _f: f64) {}
    fn quad_to(&mut self, _cx: f64, _cy: f64, _x: f64, _y: f64) {}
    fn arc_to(&mut self, _cx: f64, _cy: f64, _r: f64, _s: f64, _e: f64, _ccw: bool) {}
    fn circle(&mut self, _cx: f64, _cy: f64, _r: f64) {}
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.pending = Some([x, y, w, h]);
    }
    fn rounded_rect(&mut self, x: f64, y: f64, w: f64, h: f64, _r: f64) {
        self.pending = Some([x, y, w, h]);
    }
    fn close_path(&mut self) {}
    fn fill(&mut self) {
        if let Some(r) = self.pending.take() {
            self.filled_rects.push((r, self.fill_color));
        }
        if self.poly.len() >= 3 {
            let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
            for p in &self.poly {
                b[0] = b[0].min(p[0]);
                b[1] = b[1].min(p[1]);
                b[2] = b[2].max(p[0]);
                b[3] = b[3].max(p[1]);
            }
            self.filled_polys
                .push((b, self.fill_color, self.poly.len()));
        }
        self.poly.clear();
    }
    fn stroke(&mut self) {}
    fn fill_and_stroke(&mut self) {}
    fn draw_triangles_aa(&mut self, _v: &[[f32; 3]], _i: &[u32], _c: Color) {}
    fn fill_text(&mut self, text: &str, _x: f64, _y: f64) {
        self.texts.push(text.to_string());
    }
    fn fill_text_gsv(&mut self, text: &str, _x: f64, _y: f64, _size: f64) {
        self.texts.push(text.to_string());
    }
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

fn paint_enum_node(current: &str) -> StripRecorder {
    paint_node_with_kind(current, enum_kind())
}

fn paint_node_with_kind(current: &str, kind: EditorKind) -> StripRecorder {
    let (mut editor, _memory) = editor_with_kind(current, kind);
    let mut recorder = StripRecorder::default();
    let node_widget = editor
        .children_mut()
        .iter_mut()
        .find(|c| c.type_name() == "NodeWidget")
        .expect("the canvas built a NodeWidget for the seeded node");
    agg_gui::widget::paint_subtree(node_widget.as_mut(), &mut recorder);
    recorder
}

/// The strip reaches the screen through the real widget tree: the
/// selected variant is filled in the accent colour and every variant is
/// labelled (possibly truncated to a prefix).
///
/// The accent count is asserted *relative* to a row whose value matches
/// no variant: node chrome (title bar, selection) uses the accent colour
/// too, and pinning its absolute count here would make this test fail on
/// unrelated canvas changes.
#[test]
fn the_strip_paints_the_selected_variant_in_the_accent_colour() {
    let visuals = agg_gui::theme::current_visuals();
    let recorder = paint_enum_node("Intersect");
    let baseline = paint_enum_node("Frobnicate").filled_in(visuals.accent);

    assert_eq!(
        recorder.filled_in(visuals.accent),
        baseline + 1,
        "selecting a variant adds exactly one accent-filled segment"
    );
    for v in VARIANTS {
        assert!(
            recorder
                .texts
                .iter()
                .any(|t| !t.is_empty() && v.starts_with(t.as_str())),
            "no label painted for variant {} (painted: {:?})",
            v,
            recorder.texts
        );
    }
}

/// A value outside the variant set highlights nothing rather than
/// guessing — the row still shows every choice.
#[test]
fn an_unknown_value_still_paints_every_variant() {
    let recorder = paint_enum_node("Frobnicate");
    for v in VARIANTS {
        assert!(
            recorder
                .texts
                .iter()
                .any(|t| !t.is_empty() && v.starts_with(t.as_str())),
            "no label painted for variant {} (painted: {:?})",
            v,
            recorder.texts
        );
    }
}

// -------------------------------------------------------- icon strip

/// The four literal colours the test icons are painted in — distinct
/// from each other and from any theme colour, so a fill can be traced
/// back to exactly one segment.
const ICON_COLORS: [Color; 4] = [
    Color::rgba(0.9, 0.1, 0.1, 1.0),
    Color::rgba(0.1, 0.9, 0.1, 1.0),
    Color::rgba(0.1, 0.1, 0.9, 1.0),
    Color::rgba(0.9, 0.9, 0.1, 1.0),
];

/// Register one distinguishable icon per variant under `prefix` and
/// return an `EnumIcons` kind naming them.
///
/// Each icon is a different-sized square in a different colour, so the
/// recorded fills say which segment drew which icon — the "the four
/// icons paint non-empty and distinct" smoke test.
fn register_test_icons(prefix: &str) -> EditorKind {
    use agg_gui::vector_icon::{register_icon, IconColor, VectorIcon};
    let mut pairs = Vec::new();
    for (i, variant) in VARIANTS.iter().enumerate() {
        let id = format!("{prefix}.{i}");
        let side = 16.0 + i as f64 * 8.0;
        let icon = VectorIcon::new(64.0)
            .with_svg_path_nonzero(
                &format!("M0 0 H{side} V{side} H0 Z"),
                IconColor::Literal(ICON_COLORS[i]),
            )
            .expect("test icon path parses");
        register_icon(id.clone(), icon);
        pairs.push((variant.to_string(), id));
    }
    EditorKind::enum_icons(
        pairs
            .iter()
            .map(|(v, i)| (v.as_str(), i.as_str()))
            .collect::<Vec<_>>(),
    )
}

/// Every segment paints its registered icon: four non-empty polygon
/// fills, one per colour, ordered left to right and never overlapping.
#[test]
fn the_icon_strip_paints_one_icon_per_segment() {
    let kind = register_test_icons("test.node_editor.strip");
    let recorder = paint_node_with_kind("Combine", kind);

    let mut previous_right = f64::MIN;
    for (i, color) in ICON_COLORS.iter().enumerate() {
        let polys = recorder.polys_in(*color);
        assert_eq!(polys.len(), 1, "segment {i} painted {} icons", polys.len());
        let (bbox, points) = polys[0];
        assert!(points >= 4, "segment {i}'s icon has only {points} points");
        assert!(
            bbox[2] > bbox[0] && bbox[3] > bbox[1],
            "segment {i}'s icon has an empty bounding box: {bbox:?}"
        );
        assert!(
            bbox[0] > previous_right,
            "segment {i}'s icon overlaps its left neighbour: {bbox:?}"
        );
        previous_right = bbox[2];
    }
}

/// The icons differ from one another — a strip of four identical glyphs
/// would pass a "something painted" check and still be useless.
#[test]
fn the_icons_in_the_strip_are_distinct() {
    let kind = register_test_icons("test.node_editor.distinct");
    let recorder = paint_node_with_kind("Combine", kind);
    let widths: Vec<f64> = ICON_COLORS
        .iter()
        .map(|c| {
            let p = recorder.polys_in(*c);
            assert_eq!(p.len(), 1);
            p[0].0[2] - p[0].0[0]
        })
        .collect();
    for pair in widths.windows(2) {
        assert!(
            (pair[0] - pair[1]).abs() > 0.5,
            "two segments painted the same artwork: {widths:?}"
        );
    }
}

/// An id with nothing registered under it must not blank the segment:
/// the strip falls back to the variant's name, i.e. exactly what the
/// button strip would have shown.
#[test]
fn an_unregistered_icon_id_falls_back_to_the_variant_label() {
    let kind = EditorKind::enum_icons(
        VARIANTS
            .iter()
            .map(|v| (*v, "test.node_editor.nothing_is_registered_here"))
            .collect::<Vec<_>>(),
    );
    let recorder = paint_node_with_kind("Combine", kind);
    for v in VARIANTS {
        assert!(
            recorder
                .texts
                .iter()
                .any(|t| !t.is_empty() && v.starts_with(t.as_str())),
            "no fallback label painted for variant {} (painted: {:?})",
            v,
            recorder.texts
        );
    }
}

/// Selection chrome is the button strip's, unchanged: one extra
/// accent-filled segment relative to a value that matches no variant.
#[test]
fn the_icon_strip_marks_the_selected_variant_like_the_button_strip() {
    let visuals = agg_gui::theme::current_visuals();
    let selected = paint_node_with_kind(
        "Intersect",
        register_test_icons("test.node_editor.selected"),
    )
    .filled_in(visuals.accent);
    let baseline = paint_node_with_kind(
        "Frobnicate",
        register_test_icons("test.node_editor.selected"),
    )
    .filled_in(visuals.accent);
    assert_eq!(selected, baseline + 1);
}

/// Hit-testing is the button strip's too — the icon row commits the
/// variant under the pointer through the same real `on_mouse_down`.
#[test]
fn clicking_an_icon_segment_commits_that_variant() {
    for (idx, want) in VARIANTS.iter().enumerate() {
        let kind = register_test_icons("test.node_editor.click");
        let (mut editor, memory) = editor_with_kind("Combine", kind);
        let p = segment_point(&editor, idx);
        editor.on_mouse_down(p, MouseButton::Left, Modifiers::default());
        assert_eq!(
            committed(&memory),
            Some(("operation".to_string(), want.to_string())),
            "clicking icon segment {} should commit {}",
            idx,
            want
        );
    }
}

/// An `Ink` role follows the theme: an unselected segment's linework is
/// painted in the theme text colour, not in a baked-in grey.
#[test]
fn an_ink_icon_path_is_painted_in_the_theme_text_colour() {
    use agg_gui::vector_icon::{register_icon, IconColor, VectorIcon};
    let id = "test.node_editor.ink";
    let icon = VectorIcon::new(64.0)
        .with_svg_path_nonzero("M0 0 H52 V52 H0 Z", IconColor::Ink)
        .expect("test icon path parses");
    register_icon(id, icon);
    let kind = EditorKind::enum_icons(VARIANTS.iter().map(|v| (*v, id)).collect::<Vec<_>>());

    let recorder = paint_node_with_kind("Combine", kind);
    let text_color = agg_gui::theme::current_visuals().text_color;
    // Four segments, of which the selected one paints in the light
    // selected-content colour instead — so three ink-coloured icons.
    assert_eq!(
        recorder.polys_in(text_color).len(),
        3,
        "ink icons painted: {:?}",
        recorder.polys_in(text_color)
    );
}
