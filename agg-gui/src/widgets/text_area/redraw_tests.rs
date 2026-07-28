//! Redraw-cadence and wheel-scroll tests for [`TextArea`].
//!
//! These lock in two UX invariants exercised against the production widget:
//!
//! * A focused, idle editor must NOT report a per-frame draw need. Its caret
//!   blink is transition-scheduled: `needs_draw` stays `false` between 500 ms
//!   flip boundaries and `next_draw_deadline` reports the exact next boundary,
//!   so the host wakes twice a second instead of spinning the render loop.
//! * One mouse-wheel notch scrolls ≈ 3 line heights (the Windows convention),
//!   not a single pixel.

use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::widget::Widget;
use web_time::Instant;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn laid_out(mut ta: TextArea, w: f64, h: f64) -> TextArea {
    ta.layout(Size::new(w, h));
    ta
}

// ── Caret-blink redraw cadence ──────────────────────────────────────────────

#[test]
fn focused_idle_editor_does_not_redraw_every_frame() {
    let mut ta = laid_out(TextArea::new(font()).with_text("hello"), 200.0, 100.0);
    // Simulate a paint that just recorded the current blink phase (phase 0).
    ta.focused = true;
    let now = Instant::now();
    ta.focus_time = Some(now);
    ta.blink_last_phase.set(0);

    // The idle focused editor must not keep the render loop awake.
    assert!(
        !ta.needs_draw(),
        "idle focused editor should not report a per-frame draw need"
    );

    // Instead it schedules a wake at the next 500 ms flip boundary.
    let deadline = ta
        .next_draw_deadline()
        .expect("focused editor schedules a blink wake");
    let remaining = deadline.saturating_duration_since(now);
    assert!(
        remaining > Duration::ZERO && remaining <= Duration::from_millis(500),
        "next blink deadline should be within one 500 ms interval, got {remaining:?}"
    );
}

#[test]
fn blink_boundary_requests_one_draw_then_reschedules() {
    let mut ta = laid_out(TextArea::new(font()).with_text("hello"), 200.0, 100.0);
    ta.focused = true;
    // Focused 600 ms ago → current phase 1, last painted phase 0: a flip is due.
    let past = Instant::now() - Duration::from_millis(600);
    ta.focus_time = Some(past);
    ta.blink_last_phase.set(0);
    assert!(
        ta.needs_draw(),
        "a crossed blink boundary should request exactly one draw"
    );

    // Simulate the paint that records the new phase; no further redraw is due
    // until the next boundary.
    ta.blink_last_phase.set(1);
    assert!(
        !ta.needs_draw(),
        "after painting the new phase the editor should go idle again"
    );
    let deadline = ta
        .next_draw_deadline()
        .expect("still schedules the next flip");
    let remaining = deadline.saturating_duration_since(Instant::now());
    assert!(
        remaining <= Duration::from_millis(500),
        "reschedule should be within one interval, got {remaining:?}"
    );
}

#[test]
fn unfocused_editor_is_quiet() {
    let ta = laid_out(TextArea::new(font()).with_text("hello"), 200.0, 100.0);
    assert!(!ta.needs_draw(), "unfocused editor must not request draws");
    assert!(
        ta.next_draw_deadline().is_none(),
        "unfocused editor schedules no wake"
    );
}

// ── Wheel scrolling speed ───────────────────────────────────────────────────

#[test]
fn one_wheel_notch_scrolls_about_three_lines() {
    // Enough lines to overflow a short viewport so scrolling can happen.
    let text = (0..40)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut ta = laid_out(TextArea::new(font()).with_text(&text), 200.0, 100.0);
    let line_h = ta.cached_line_h;
    assert!(
        line_h > 0.0,
        "layout should populate the cached line height"
    );

    let before = ta.vbar.offset;
    // One notch downward. `delta_y` is in line-units (1.0 == one notch); a
    // negative value scrolls the content down (offset increases).
    assert!(ta.scroll_by_wheel(-1.0), "a notch should move the offset");
    let moved = ta.vbar.offset - before;
    let expected = 3.0 * line_h;
    assert!(
        (moved - expected).abs() < 0.5,
        "one notch should scroll ~3 lines: moved {moved}, expected {expected}"
    );
}
