//! ColorWheelPicker — App-level integration tests. Extracted from
//! `tests/widgets.rs` to keep that file under the 800-line guardrail.

use super::*;

// ---------------------------------------------------------------------------
// ColorWheelPicker — App-level integration tests.
//
// Layout reminder: viewport coords here are *screen* (Y-down).  The picker
// runs in Y-up local coords so we compute click positions by working out
// the world-space (Y-up) Y first, then flipping via `screen_y = VP_H - y`.
// The picker has a fixed intrinsic size (~220 × variable) and lays out at
// the bottom-left of the viewport with our App wrapper.
// ---------------------------------------------------------------------------

/// Helper: layout the picker into a viewport and return the actual size
/// it occupied in Y-up coords plus a closure that flips Y-up → screen.
fn cwp_layout(
    picker: crate::ColorWheelPicker,
    vp_w: f64,
    vp_h: f64,
) -> (App, f64, f64, impl Fn(f64) -> f64) {
    let mut app = App::new(Box::new(picker));
    app.layout(Size::new(vp_w, vp_h));
    let flip = move |y_up: f64| vp_h - y_up;
    (app, vp_w, vp_h, flip)
}

/// Dragging on the hue ring rotates `h` and therefore moves the
/// reported colour off the starting hue.  Verifies the wheel hit-test
/// fires `on_change` with a *different* RGB triple than the start.
#[test]
fn test_color_wheel_picker_drag_hue_updates_color() {
    use crate::text::Font;
    use crate::ColorWheelPicker;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let start = Color::rgba(1.0, 0.0, 0.0, 1.0); // saturated red → drag will move RGB
    let last = Rc::new(Cell::new(start));
    let last_cb = Rc::clone(&last);
    let picker = ColorWheelPicker::new(start, Arc::clone(&font)).on_change(move |c| {
        if let Some(col) = c {
            last_cb.set(col);
        }
    });

    // Viewport big enough to host the picker (~220 × ~330 for default
    // show_alpha=true, allow_none=false).
    const VP_W: f64 = 240.0;
    const VP_H: f64 = 420.0;
    let (mut app, _, vp_h, flip) = cwp_layout(picker, VP_W, VP_H);

    // Wheel centre in Y-up.  Layout pinned to (0,0): top edge of wheel
    // sits at y_up = picker_h - PAD.  Picker height = bottom = 0; the
    // picker is laid out at origin so wheel centre y_up = picker_h - PAD - WHEEL_SIZE/2.
    let picker_h = crate::widgets::color_wheel_picker::picker_height(false, true);
    let cy_up = picker_h - 10.0 - 200.0 * 0.5; // PAD=10, WHEEL_SIZE=200
    let cx = VP_W * 0.5; // app centres in the available width via WidgetBase default? — no, picker
                         // returns its intrinsic size and App leaves it at (0,0).  Use intrinsic centre.
    let _ = cx; // keep linter quiet — we compute via picker_width below.
    let cx = crate::widgets::color_wheel_picker::picker_width() * 0.5;

    // Pick a point on the ring at 120° (Y-up).  Ring centre radius:
    //   outer = 100 × 85/95 ≈ 89.47,  inner = 100 × 60/95 ≈ 63.16
    //   ring_r = (outer + inner) / 2 ≈ 76.32
    let ring_r = 0.5 * 200.0 * (crate::widgets::color_wheel_picker::WHEEL_OUTER_RATIO
        + crate::widgets::color_wheel_picker::WHEEL_INNER_RATIO)
        * 0.5;
    let angle = 120.0_f64.to_radians();
    let target_x = cx + ring_r * angle.cos();
    let target_y_up = cy_up + ring_r * angle.sin();
    let target_screen_y = flip(target_y_up);

    let _ = vp_h;
    app.on_mouse_down(
        target_x,
        target_screen_y,
        MouseButton::Left,
        Modifiers::default(),
    );
    app.on_mouse_move(target_x, target_screen_y);
    app.on_mouse_up(
        target_x,
        target_screen_y,
        MouseButton::Left,
        Modifiers::default(),
    );

    let got = last.get();
    assert_ne!(
        (start.r, start.g, start.b),
        (got.r, got.g, got.b),
        "hue drag must shift RGB (started at red, got {:?})",
        (got.r, got.g, got.b),
    );
}

/// With `allow_none = true`, ticking the **No Color (Pass Through)**
/// checkbox should make `on_change` deliver `None` on its next pass.
#[test]
fn test_color_wheel_picker_no_color_returns_none() {
    use crate::text::Font;
    use crate::ColorWheelPicker;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let start = Color::rgba(0.2, 0.7, 0.4, 1.0);
    let last_was_none = Rc::new(Cell::new(false));
    let last_was_none_cb = Rc::clone(&last_was_none);
    let picker = ColorWheelPicker::new(start, Arc::clone(&font))
        .with_allow_none(true)
        .on_change(move |c| {
            last_was_none_cb.set(c.is_none());
        });

    const VP_W: f64 = 240.0;
    const VP_H: f64 = 500.0;
    let (mut app, _, _, flip) = cwp_layout(picker, VP_W, VP_H);

    // No-Color checkbox row sits between preview and buttons in the
    // local layout walk.  Cursor (top-down, Y-up):
    //   y_top after PAD       = picker_h - PAD
    //   after wheel + gap     = ... - 200 - 6
    //   after alpha + gap     = ... - 22 - 6      (show_alpha default true)
    //   after hex + gap       = ... - 26 - 6
    //   after preview + gap   = ... - 32 - 6
    //   nocolor band: y_top - 22 .. y_top
    let picker_h = crate::widgets::color_wheel_picker::picker_height(true, true);
    let y_top = picker_h - 10.0 - 200.0 - 6.0 - 22.0 - 6.0 - 26.0 - 6.0 - 32.0 - 6.0;
    let nocolor_y_centre = y_top - 11.0;
    let click_x = 20.0; // a few px in from the left edge — well inside the box glyph
    let screen_y = flip(nocolor_y_centre);

    app.on_mouse_down(click_x, screen_y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(click_x, screen_y, MouseButton::Left, Modifiers::default());

    // Trigger one more layout so the picker drains the checkbox cell
    // and re-fires on_change with the new pass-through state.
    app.layout(Size::new(VP_W, VP_H));

    assert!(
        last_was_none.get(),
        "ticking 'No Color' must cause on_change to deliver None",
    );
}

/// Cancel must restore the picker to its starting colour and re-fire
/// `on_change` with the saved colour (so listeners see the snap-back).
#[test]
fn test_color_wheel_picker_cancel_restores_original() {
    use crate::text::Font;
    use crate::ColorWheelPicker;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let start = Color::rgba(1.0, 0.0, 0.0, 1.0); // pure red
    let last = Rc::new(Cell::new(start));
    let cancel_fired = Rc::new(Cell::new(false));
    let last_cb = Rc::clone(&last);
    let cancel_cb = Rc::clone(&cancel_fired);
    let picker = ColorWheelPicker::new(start, Arc::clone(&font))
        .on_change(move |c| {
            if let Some(col) = c {
                last_cb.set(col);
            }
        })
        .on_cancel(move || cancel_cb.set(true));

    const VP_W: f64 = 240.0;
    const VP_H: f64 = 420.0;
    let (mut app, _, _, flip) = cwp_layout(picker, VP_W, VP_H);

    // First, drag the hue so the working colour is *not* red anymore.
    let picker_h = crate::widgets::color_wheel_picker::picker_height(false, true);
    let cy_up = picker_h - 10.0 - 100.0;
    let cx = crate::widgets::color_wheel_picker::picker_width() * 0.5;
    let ring_r = 0.5 * 200.0 * (crate::widgets::color_wheel_picker::WHEEL_OUTER_RATIO
        + crate::widgets::color_wheel_picker::WHEEL_INNER_RATIO)
        * 0.5;
    let angle = 200.0_f64.to_radians();
    let drag_x = cx + ring_r * angle.cos();
    let drag_y = cy_up + ring_r * angle.sin();
    let drag_screen_y = flip(drag_y);
    app.on_mouse_down(drag_x, drag_screen_y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(drag_x, drag_screen_y, MouseButton::Left, Modifiers::default());

    let after_drag = last.get();
    assert_ne!(
        (start.r, start.g, start.b),
        (after_drag.r, after_drag.g, after_drag.b),
        "sanity: drag must have shifted hue before we test the restore",
    );

    // Now click Cancel: bottom-left button row, half-width.
    //   btn_w = (picker_w - 2*PAD - ROW_GAP) / 2 = (220 - 20 - 6)/2 = 97
    //   Cancel rect = (PAD=10, PAD=10, 97, 30)
    let cancel_cx = 10.0 + 97.0 * 0.5; // 58.5
    let cancel_cy_up = 10.0 + 30.0 * 0.5; // 25
    let cancel_screen_y = flip(cancel_cy_up);
    // Button only fires its on_click on MouseUp while both `hovered` and
    // `pressed` are true — we have to issue an explicit MouseMove first
    // so the Button's `hit_test` flips `hovered` on.
    app.on_mouse_move(cancel_cx, cancel_screen_y);
    app.on_mouse_down(
        cancel_cx,
        cancel_screen_y,
        MouseButton::Left,
        Modifiers::default(),
    );
    app.on_mouse_up(
        cancel_cx,
        cancel_screen_y,
        MouseButton::Left,
        Modifiers::default(),
    );
    // Cancel was consumed by the Button child — drain the flag via a
    // re-layout (the picker's layout pass picks up button outcomes).
    app.layout(Size::new(VP_W, VP_H));

    assert!(cancel_fired.get(), "Cancel button must fire on_cancel");
    let restored = last.get();
    let drift = (restored.r - start.r).abs()
        + (restored.g - start.g).abs()
        + (restored.b - start.b).abs();
    assert!(
        drift < 1e-4,
        "Cancel must restore the starting colour via on_change (got {:?})",
        (restored.r, restored.g, restored.b),
    );
}
