//! App-level reproduction for the reported bug: **central `with_tooltip`
//! tips never appear when hovering a toolbar button inside a real `Window`**.
//!
//! The controller unit tests (`widgets/tooltip/controller.rs`) drive the state
//! machine with a synthetic `Tipped` leaf that fills the whole viewport, so they
//! never exercise the real `App::hovered` → `deepest_tipped` path through a
//! deep, windowed widget tree. This suite mirrors the RichTextEdit demo tree
//! (a `Window` in a `Stack`, containing a `FlexColumn` panel whose first row is
//! a `FlexRow` of icon `Button`s each carrying `.with_tooltip(...)`, plus an
//! aligned `Rebuilder` overlay slot) and drives the *real* pointer path
//! (`App::on_mouse_move` with screen coords) followed by the same per-frame
//! tooltip pass `App::paint` runs. It then asserts the controller actually
//! shows the hovered button's tip.

use super::*;
use crate::geometry::{Point, Rect};
use crate::text::Font;
use crate::widgets::tooltip::controller;
use crate::widgets::tooltip::{
    advance_tooltip_test_clock, reset_tooltip_test_state, set_tooltip_test_clock, tooltip_timings,
};
use crate::widgets::window::Window;
use crate::{Rebuilder, Stack};
use std::sync::Arc;
use web_time::Instant;

const VP_W: f64 = 700.0;
const VP_H: f64 = 560.0;

fn unit_scale() {
    crate::device_scale::set_device_scale(1.0);
    crate::ux_scale::set_ux_scale(1.0);
}

/// Install the bundled test font as the crate-wide system font (the tooltip
/// controller paints/measures with `current_system_font`), pin the tooltip
/// clock, and clear leftover controller/timing state. Restores on drop.
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        reset_tooltip_test_state();
        controller::reset();
        crate::font_settings::set_system_font(None);
    }
}
fn pin() -> Guard {
    reset_tooltip_test_state();
    controller::reset();
    set_tooltip_test_clock(Some(Instant::now()));
    crate::font_settings::set_system_font(Some(Arc::new(
        Font::from_slice(TEST_FONT).expect("test font must load"),
    )));
    Guard
}

/// A toolbar icon button standing in for the demo's `style_toggle` buttons:
/// a real `Button` carrying a `with_tooltip` string on its `WidgetBase`.
fn tip_button(label: &str, tip: &str, font: &Arc<Font>) -> Box<dyn Widget> {
    let mut b: Box<dyn Widget> = Box::new(
        Button::new(label, Arc::clone(font))
            .with_font_size(14.0)
            .with_compact(),
    );
    b.set_tooltip_text(Some(tip.to_string()));
    b
}

/// Build the RichTextEdit-demo-shaped body: a panel `FlexColumn` whose first
/// child is the two-row toolbar (`FlexColumn` of `FlexRow`s), then a tall
/// editor stand-in, then a source-link stand-in — all wrapped in a
/// hit-transparent `Stack` with an aligned `Rebuilder` overlay (closed = a
/// zero-size box), exactly like `rich_text_demo::rich_text_edit`.
fn demo_body(font: &Arc<Font>) -> Box<dyn Widget> {
    let mut row_one = FlexRow::new().with_gap(4.0);
    row_one = row_one.add(tip_button("\u{F032}", "Bold", font));
    row_one = row_one.add(tip_button("\u{F033}", "Italic", font));
    row_one = row_one.add(tip_button("\u{F0CD}", "Underline", font));
    row_one = row_one.add(tip_button("\u{F0CC}", "Strikethrough", font));

    let mut row_two = FlexRow::new().with_gap(4.0);
    row_two = row_two.add(tip_button("\u{F036}", "Align left", font));
    row_two = row_two.add(tip_button("\u{F037}", "Align center", font));

    let mut toolbar = FlexColumn::new().with_gap(6.0);
    toolbar.push(Box::new(row_one), 0.0);
    toolbar.push(Box::new(row_two), 0.0);

    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(12.0)
        .with_panel_bg();
    col.push(Box::new(toolbar), 0.0);
    // Editor stand-in fills the remaining height.
    col.push(
        Box::new(SizedBox::new().with_height(320.0).with_child(Box::new(
            Container::new().with_background(Color::rgb(0.1, 0.1, 0.1)),
        ))),
        1.0,
    );
    col.push(
        Box::new(Button::new("source", Arc::clone(font)).with_font_size(12.0)),
        0.0,
    );

    // Closed colour-picker overlay: a Rebuilder that builds a zero-size box.
    let overlay: Box<dyn Widget> = Box::new(Rebuilder::new(
        || 0u64,
        || Box::new(SizedBox::new().with_width(0.0).with_height(0.0)),
    ));

    Box::new(
        Stack::new()
            .with_hit_children_only(false)
            .add(Box::new(col))
            .add_aligned(overlay),
    )
}

/// Build the app: a canvas `Stack` holding a single real `Window` whose content
/// is the demo body.
fn build_app(font: &Arc<Font>) -> App {
    let win = Window::new("\u{F1DC} RichTextEdit", Arc::clone(font), demo_body(font))
        .with_bounds(Rect::new(40.0, 40.0, 560.0, 460.0));
    let canvas = Stack::new().add(Box::new(win));
    let mut app = App::new(Box::new(canvas));
    app.layout(Size::new(VP_W, VP_H));
    app
}

/// Depth-first search for the widget whose `tooltip_text()` equals `tip`,
/// accumulating child-bounds offsets to return its world-space (Y-up) rect.
/// Mirrors the offset accumulation the framework's hit-test does.
fn tipped_world_rect(app: &App, tip: &str) -> Rect {
    fn walk(w: &dyn Widget, ox: f64, oy: f64, tip: &str) -> Option<Rect> {
        if w.tooltip_text() == Some(tip) {
            let b = w.bounds();
            return Some(Rect::new(ox, oy, b.width, b.height));
        }
        for child in w.children() {
            let b = child.bounds();
            if let Some(r) = walk(child.as_ref(), ox + b.x, oy + b.y, tip) {
                return Some(r);
            }
        }
        None
    }
    // The root's own bounds contribute no offset (it is placed at 0,0).
    walk(app.root(), 0.0, 0.0, tip).unwrap_or_else(|| panic!("no tipped widget with text {tip:?}"))
}

/// Move the pointer to the world-space (Y-up) point via the real screen path.
fn hover_world(app: &mut App, p: Point) {
    app.on_mouse_move(p.x, VP_H - p.y);
}

/// The reported bug, at the App level: hovering the "Italic" toolbar button in
/// a windowed demo tree and waiting past the initial delay must show its tip.
#[test]
fn hovering_toolbar_button_in_window_shows_tip() {
    unit_scale();
    let _g = pin();
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font"));
    let mut app = build_app(&font);

    let r = tipped_world_rect(&app, "Italic");
    let center = Point::new(r.x + r.width * 0.5, r.y + r.height * 0.5);

    // First frame: pointer enters the button; the tip is not yet due.
    hover_world(&mut app, center);
    app.update_tooltips_for_test();
    assert!(
        !controller::is_visible(),
        "no tip before the initial delay elapses"
    );

    // Advance past the initial delay and run the per-frame tooltip pass again.
    advance_tooltip_test_clock(tooltip_timings().initial_delay);
    app.update_tooltips_for_test();

    assert!(
        controller::is_visible(),
        "tip must appear after the initial delay while hovering the Italic button"
    );
    assert_eq!(controller::visible_text().as_deref(), Some("Italic"));
}
