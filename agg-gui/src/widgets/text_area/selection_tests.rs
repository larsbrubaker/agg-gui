//! Double-click word / triple-click line selection tests for [`TextArea`].
//!
//! Kept separate from `tests.rs` (which is near the 800-line cap) but exercises
//! the same production widget: the pointer-selection funnels
//! (`begin_pointer_selection` / `extend_selection_drag`) that `widget_impl`
//! drives on mouse down and drag, plus a real `MouseDown` event path so the
//! multi-click counting is covered end to end.

use std::sync::Arc;

use super::*;
use crate::event::{Event, MouseButton};
use crate::geometry::Point;
use crate::widget::Widget;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn laid_out(text: &str, w: f64, h: f64) -> TextArea {
    let mut ta = TextArea::new(font()).with_text(text);
    ta.layout(Size::new(w, h));
    ta
}

/// A double-click (clicks == 2) selects the word under the byte offset.
#[test]
fn double_click_selects_word() {
    let mut ta = laid_out("hello world", 400.0, 120.0);
    // Offset 8 sits inside "world" (bytes 6..11).
    ta.begin_pointer_selection(8, 2, false);
    assert_eq!(ta.selection(), Some((6, 11)));
    assert_eq!(ta.selected_text(), "world");
}

/// Double-click on a punctuation run selects just that run, not the words.
#[test]
fn double_click_stops_at_punctuation() {
    let mut ta = laid_out("foo.bar", 400.0, 120.0);
    ta.begin_pointer_selection(1, 2, false);
    assert_eq!(ta.selected_text(), "foo");
    ta.begin_pointer_selection(5, 2, false);
    assert_eq!(ta.selected_text(), "bar");
}

/// A triple-click (clicks == 3) selects the whole logical line (paragraph),
/// even though the paragraph wraps across several visual lines.
#[test]
fn triple_click_selects_logical_line() {
    let text = "first line\nsecond line here\nthird";
    let mut ta = laid_out(text, 400.0, 200.0);
    // Offset 15 is inside "second line here" (the middle paragraph).
    ta.begin_pointer_selection(15, 3, false);
    assert_eq!(ta.selected_text(), "second line here");
}

/// Dragging after a double-click extends the selection by whole words.
#[test]
fn word_drag_extends_by_whole_words() {
    let text = "alpha beta gamma";
    let mut ta = laid_out(text, 400.0, 120.0);
    // Double-click "beta" (bytes 6..10).
    ta.begin_pointer_selection(7, 2, false);
    assert_eq!(ta.selected_text(), "beta");
    // Drag right into "gamma": selection should snap to the end of "gamma".
    ta.extend_selection_drag(13);
    assert_eq!(ta.selected_text(), "beta gamma");
    // Drag left into "alpha": selection should snap to the start of "alpha".
    ta.extend_selection_drag(2);
    assert_eq!(ta.selected_text(), "alpha beta");
}

/// A real double `MouseDown` at the same point selects the word, proving the
/// multi-click counter is wired through the event handler.
#[test]
fn double_mouse_down_event_selects_word() {
    let mut ta = laid_out("hello world", 400.0, 120.0);
    ta.on_event(&Event::FocusGained);
    // Local point at the left edge of byte 8, nudged right so hit-testing lands
    // squarely inside "world".
    let p = ta.pos_for_cursor(8);
    let click = Point::new(p.x + 1.0, p.y + 1.0);
    let down = Event::MouseDown {
        pos: click,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    };
    ta.on_event(&down);
    ta.on_event(&Event::MouseUp {
        pos: click,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    ta.on_event(&down);
    assert_eq!(ta.selected_text(), "world");
}

// ── Text-rendering pipeline: LCD routing + baseline snapping ────────────────

/// The editor must follow the System LCD setting the same way `Label` /
/// `TextField` do: an `LcdCoverage` backbuffer when LCD is on, a grayscale
/// `Rgba` backbuffer when it is off. Toggling the global flag changes the
/// render-path decision on the next frame.
#[test]
fn backbuffer_mode_follows_lcd_setting() {
    use crate::widget::BackbufferMode;
    // Standard density so the high-scale hard cap in `lcd_enabled` doesn't mask
    // the explicit override under test.
    crate::device_scale::set_device_scale(1.0);
    crate::ux_scale::set_ux_scale(1.0);

    let ta = laid_out("hello", 400.0, 120.0);

    crate::font_settings::set_lcd_enabled(true);
    assert_eq!(ta.backbuffer_mode(), BackbufferMode::LcdCoverage);

    crate::font_settings::set_lcd_enabled(false);
    assert_eq!(ta.backbuffer_mode(), BackbufferMode::Rgba);

    crate::font_settings::clear_lcd_enabled_override();
}

/// With hinting on, every visual line's baseline lands on an integer pixel row
/// at scale 1 — the same crisp-vertical treatment `Label` gets — so multi-line
/// text does not blur across two rows.
#[test]
fn line_baselines_snap_to_pixel_grid_when_hinting_on() {
    // Font size chosen so the raw (unsnapped) baseline is fractional, proving
    // the snap is doing real work rather than coincidentally landing integer.
    let mut ta = TextArea::new(font())
        .with_text("line one\nline two\nline three")
        .with_font_size(13.0);
    ta.layout(Size::new(400.0, 200.0));

    crate::font_settings::set_hinting_enabled(true);
    for i in 0..ta.visual_line_count() {
        let y = ta.line_baseline_y(i);
        assert_eq!(y.fract(), 0.0, "line {i} baseline {y} not on the pixel grid");
    }
    crate::font_settings::set_hinting_enabled(false);
}
