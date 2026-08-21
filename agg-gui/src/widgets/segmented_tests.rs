//! Unit tests for [`SegmentedControl`](super::SegmentedControl), split out
//! of `segmented.rs` (pulled in via `#[path]` as a child module so the
//! tests can reach private geometry and state).

use super::*;
use crate::event::Modifiers;
use crate::tests::paint_recorder::PaintRecorder;
use crate::theme::{current_visuals, set_visuals, Visuals};

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

fn test_font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn control(labels: &[&str], selected: usize) -> (SegmentedControl, Rc<Cell<usize>>) {
    let cell = Rc::new(Cell::new(selected));
    let ctl = SegmentedControl::new(labels.to_vec(), Rc::clone(&cell), test_font());
    (ctl, cell)
}

fn center(r: Rect) -> Point {
    Point::new(r.x + r.width * 0.5, r.y + r.height * 0.5)
}

fn click(ctl: &mut SegmentedControl, pos: Point) -> EventResult {
    let down = Event::MouseDown {
        pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    };
    let up = Event::MouseUp {
        pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    };
    let _ = ctl.on_event(&down);
    ctl.on_event(&up)
}

fn key(ctl: &mut SegmentedControl, key: Key) -> EventResult {
    ctl.on_event(&Event::KeyDown {
        key,
        modifiers: Modifiers::default(),
    })
}

fn painted(ctl: &mut SegmentedControl) -> PaintRecorder {
    let mut rec = PaintRecorder::new();
    ctl.paint(&mut rec);
    rec
}

// ── Geometry ────────────────────────────────────────────────────────────

#[test]
fn segments_are_equal_width_and_gapless_by_default() {
    let (mut ctl, _) = control(&["A", "Medium", "Very long label"], 0);
    let size = ctl.layout(Size::new(600.0, 0.0));
    assert_eq!(ctl.segments.len(), 3);
    let w0 = ctl.segments[0].width;
    for s in &ctl.segments {
        assert!(
            (s.width - w0).abs() < 1e-9,
            "equal widths, got {:?}",
            ctl.segments
        );
    }
    for pair in ctl.segments.windows(2) {
        assert!(
            (pair[0].x + pair[0].width - pair[1].x).abs() < 1e-9,
            "no gap between segments"
        );
    }
    assert!(
        (size.width - 3.0 * w0).abs() < 1e-9,
        "widget is exactly the strip"
    );
    assert_eq!(size.height, REGULAR_H);
}

#[test]
fn fit_width_gives_each_segment_its_own_width() {
    let (mut ctl, _) = control(&["A", "Very long label"], 0);
    ctl = ctl.with_fit_width(true);
    ctl.layout(Size::new(600.0, 0.0));
    assert!(
        ctl.segments[0].width < ctl.segments[1].width,
        "short label gets a narrower segment: {:?}",
        ctl.segments
    );
}

#[test]
fn stretch_anchor_fills_available_width() {
    let (mut ctl, _) = control(&["One", "Two"], 0);
    ctl = ctl.with_h_anchor(HAnchor::STRETCH);
    let size = ctl.layout(Size::new(400.0, 0.0));
    assert!((size.width - 400.0).abs() < 1e-9);
    assert!((ctl.segments[0].width - 200.0).abs() < 1e-9);
}

#[test]
fn equal_width_mode_distributes_whole_pixels() {
    // 401 px over three segments: floor(401 / 3) = 133 each, the two
    // leftover pixels go to the leading segments — never 133.666… each.
    let (mut ctl, _) = control(&["One", "Two", "Three"], 0);
    ctl = ctl.with_h_anchor(HAnchor::STRETCH);
    let size = ctl.layout(Size::new(401.0, 0.0));
    let widths: Vec<f64> = ctl.segments.iter().map(|s| s.width).collect();
    assert_eq!(widths, vec![134.0, 134.0, 133.0]);
    assert_eq!(size.width, 401.0);
    for s in &ctl.segments {
        assert_eq!(s.x, s.x.floor(), "segment edges on whole pixels: {:?}", ctl.segments);
    }

    // A fractional target floors to whole pixels before the split.
    let (mut ctl, _) = control(&["One", "Two", "Three"], 0);
    ctl = ctl.with_h_anchor(HAnchor::STRETCH);
    let size = ctl.layout(Size::new(400.6, 0.0));
    let widths: Vec<f64> = ctl.segments.iter().map(|s| s.width).collect();
    assert_eq!(widths, vec![134.0, 133.0, 133.0]);
    assert_eq!(size.width, 400.0);
}

#[test]
fn narrow_available_width_clamps_the_strip() {
    let (mut ctl, _) = control(&["Alpha", "Beta", "Gamma"], 0);
    let size = ctl.layout(Size::new(60.0, 0.0));
    assert!(size.width <= 60.0 + 1e-9);
    assert!((ctl.segments[2].x + ctl.segments[2].width - size.width).abs() < 1e-9);
}

#[test]
fn compact_is_shorter_than_regular() {
    let (mut regular, _) = control(&["A", "B"], 0);
    let (mut compact, _) = control(&["A", "B"], 0);
    compact = compact.with_compact();
    let r = regular.layout(Size::new(300.0, 0.0));
    let c = compact.layout(Size::new(300.0, 0.0));
    assert_eq!(r.height, REGULAR_H);
    assert_eq!(c.height, COMPACT_H);
    assert!(c.width < r.width, "compact padding + type is narrower");
    assert_eq!(compact.measure_min_height(300.0), COMPACT_H);
}

#[test]
fn labels_are_centered_inside_their_segments() {
    let (mut ctl, _) = control(&["A", "Medium"], 0);
    ctl.layout(Size::new(600.0, 0.0));
    for (i, seg) in ctl.segments.iter().enumerate() {
        let lb = ctl.children[i].bounds();
        let seg_c = seg.x + seg.width * 0.5;
        let lb_c = lb.x + lb.width * 0.5;
        assert!(
            (seg_c - lb_c).abs() < 1.0,
            "segment {i}: {seg:?} vs label {lb:?}"
        );
        assert!(lb.x >= seg.x && lb.x + lb.width <= seg.x + seg.width + 1e-6);
    }
}

// ── Selection: pointer ──────────────────────────────────────────────────

#[test]
fn click_selects_segment_writes_cell_and_fires_on_change() {
    let (ctl, cell) = control(&["First", "Second", "Third"], 0);
    let fired = Rc::new(Cell::new(None));
    let f = Rc::clone(&fired);
    let mut ctl = ctl.on_change(move |i| f.set(Some(i)));
    ctl.layout(Size::new(600.0, 0.0));
    let p = center(ctl.segments[2]);
    let r = click(&mut ctl, p);
    assert_eq!(r, EventResult::Consumed);
    assert_eq!(cell.get(), 2);
    assert_eq!(ctl.selected(), 2);
    assert_eq!(fired.get(), Some(2));
}

#[test]
fn clicking_the_selected_segment_does_not_fire_on_change() {
    let (ctl, _) = control(&["First", "Second"], 1);
    let count = Rc::new(Cell::new(0));
    let c = Rc::clone(&count);
    let mut ctl = ctl.on_change(move |_| c.set(c.get() + 1));
    ctl.layout(Size::new(600.0, 0.0));
    let p = center(ctl.segments[1]);
    click(&mut ctl, p);
    assert_eq!(count.get(), 0);
}

#[test]
fn press_then_release_elsewhere_does_not_select() {
    let (mut ctl, cell) = control(&["First", "Second"], 0);
    ctl.layout(Size::new(600.0, 0.0));
    let down = Event::MouseDown {
        pos: center(ctl.segments[1]),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    };
    let up = Event::MouseUp {
        pos: Point::new(-50.0, -50.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    };
    ctl.on_event(&down);
    ctl.on_event(&up);
    assert_eq!(cell.get(), 0, "drag-off cancels the press");
    assert!(ctl.pressed.is_none());
}

#[test]
fn click_outside_any_segment_is_ignored() {
    let (mut ctl, cell) = control(&["First", "Second"], 0);
    ctl.layout(Size::new(600.0, 0.0));
    let r = click(&mut ctl, Point::new(-10.0, 5.0));
    assert_eq!(r, EventResult::Ignored);
    assert_eq!(cell.get(), 0);
}

#[test]
fn external_cell_write_is_reflected() {
    let (mut ctl, cell) = control(&["First", "Second", "Third"], 0);
    ctl.layout(Size::new(600.0, 0.0));
    cell.set(1);
    assert_eq!(ctl.selected(), 1);
    // Out-of-range values are clamped on the next layout.
    cell.set(99);
    ctl.layout(Size::new(600.0, 0.0));
    assert_eq!(cell.get(), 2);
}

// ── Enabled gates ───────────────────────────────────────────────────────

#[test]
fn disabled_segment_cannot_be_clicked_hovered_or_stepped_to() {
    let (ctl, cell) = control(&["First", "Second", "Third"], 0);
    let mut ctl = ctl.with_segment_enabled_fn(|i| i != 1);
    ctl.layout(Size::new(600.0, 0.0));

    let p = center(ctl.segments[1]);
    let r = click(&mut ctl, p);
    assert_eq!(r, EventResult::Ignored);
    assert_eq!(cell.get(), 0);

    ctl.on_event(&Event::MouseMove { pos: p });
    assert_eq!(ctl.hovered, None, "disabled segments don't hover");

    // Right from 0 skips the disabled 1 and lands on 2.
    assert_eq!(key(&mut ctl, Key::ArrowRight), EventResult::Consumed);
    assert_eq!(cell.get(), 2);
    // Left from 2 skips back to 0.
    assert_eq!(key(&mut ctl, Key::ArrowLeft), EventResult::Consumed);
    assert_eq!(cell.get(), 0);
}

#[test]
fn whole_control_disabled_ignores_input_and_is_not_focusable() {
    let enabled = Rc::new(Cell::new(false));
    let e = Rc::clone(&enabled);
    let (ctl, cell) = control(&["First", "Second"], 0);
    let mut ctl = ctl.with_enabled_fn(move || e.get());
    ctl.layout(Size::new(600.0, 0.0));
    assert!(!ctl.is_focusable());
    let p = center(ctl.segments[1]);
    assert_eq!(click(&mut ctl, p), EventResult::Ignored);
    assert_eq!(key(&mut ctl, Key::ArrowRight), EventResult::Ignored);
    assert_eq!(cell.get(), 0);

    // Re-enabling live (no rebuild) restores interaction.
    enabled.set(true);
    assert!(ctl.is_focusable());
    assert_eq!(click(&mut ctl, p), EventResult::Consumed);
    assert_eq!(cell.get(), 1);
}

// ── Keyboard ────────────────────────────────────────────────────────────

#[test]
fn arrow_keys_move_selection_and_stop_at_the_ends() {
    let (mut ctl, cell) = control(&["First", "Second", "Third"], 0);
    ctl.layout(Size::new(600.0, 0.0));
    assert_eq!(key(&mut ctl, Key::ArrowRight), EventResult::Consumed);
    assert_eq!(cell.get(), 1);
    assert_eq!(key(&mut ctl, Key::ArrowRight), EventResult::Consumed);
    assert_eq!(cell.get(), 2);
    // At the end: nothing to move to, the key is left for the parent.
    assert_eq!(key(&mut ctl, Key::ArrowRight), EventResult::Ignored);
    assert_eq!(cell.get(), 2);
    assert_eq!(key(&mut ctl, Key::Home), EventResult::Consumed);
    assert_eq!(cell.get(), 0);
    assert_eq!(key(&mut ctl, Key::End), EventResult::Consumed);
    assert_eq!(cell.get(), 2);
    // Unrelated keys are ignored.
    assert_eq!(key(&mut ctl, Key::Enter), EventResult::Ignored);
}

#[test]
fn focus_events_toggle_the_ring_state() {
    let (mut ctl, _) = control(&["A", "B"], 0);
    assert_eq!(ctl.on_event(&Event::FocusGained), EventResult::Consumed);
    assert!(ctl.focused);
    assert_eq!(ctl.on_event(&Event::FocusLost), EventResult::Consumed);
    assert!(!ctl.focused);
}

// ── Paint ───────────────────────────────────────────────────────────────

#[test]
fn exactly_one_segment_is_filled_with_the_accent() {
    set_visuals(Visuals::light());
    let v = current_visuals();
    for sel in 0..3 {
        let (mut ctl, _) = control(&["First", "Second", "Third"], sel);
        ctl.layout(Size::new(600.0, 0.0));
        let rec = painted(&mut ctl);
        assert_eq!(
            rec.fills_with(v.accent),
            1,
            "selected={sel}: exactly one accent fill"
        );
        // Track: a rounded-rect fill; selected pill: a freeform path.
        assert!(rec
            .fills
            .iter()
            .any(|op| op.kind == crate::tests::paint_recorder::PathKind::RoundedRect));
        assert!(rec.fills.iter().any(|op| op.color == v.accent
            && op.kind == crate::tests::paint_recorder::PathKind::Freeform));
    }
}

#[test]
fn hovering_an_unselected_segment_does_not_add_a_second_accent_fill() {
    set_visuals(Visuals::light());
    let v = current_visuals();
    let (mut ctl, _) = control(&["First", "Second", "Third"], 0);
    ctl.layout(Size::new(600.0, 0.0));
    let p = center(ctl.segments[2]);
    ctl.on_event(&Event::MouseMove { pos: p });
    assert_eq!(ctl.hovered, Some(2));
    let rec = painted(&mut ctl);
    assert_eq!(rec.fills_with(v.accent), 1);
    assert_eq!(
        rec.fills_with(v.widget_bg_hovered),
        1,
        "hover fill on segment 2"
    );
}

#[test]
fn dividers_are_skipped_next_to_the_selected_segment() {
    set_visuals(Visuals::light());
    let v = current_visuals();
    // 4 segments → 3 divider slots. With 0 selected, slot 1 is skipped
    // (it borders the selection), leaving 2 dividers + 1 track outline
    // stroke in widget_stroke.
    let (mut ctl, _) = control(&["A", "B", "C", "D"], 0);
    ctl.layout(Size::new(600.0, 0.0));
    let rec = painted(&mut ctl);
    assert_eq!(rec.strokes_with(v.widget_stroke), 2 + 1);
    // Middle selection (index 1) skips slots 1 and 2 → 1 divider + outline.
    let (mut ctl, _) = control(&["A", "B", "C", "D"], 1);
    ctl.layout(Size::new(600.0, 0.0));
    let rec = painted(&mut ctl);
    assert_eq!(rec.strokes_with(v.widget_stroke), 1 + 1);
}

#[test]
fn disabled_control_paints_without_accent() {
    set_visuals(Visuals::light());
    let v = current_visuals();
    let (ctl, _) = control(&["A", "B"], 0);
    let mut ctl = ctl.with_enabled_fn(|| false);
    ctl.layout(Size::new(600.0, 0.0));
    let rec = painted(&mut ctl);
    assert_eq!(rec.fills_with(v.accent), 0);
}

#[test]
fn focused_control_paints_a_focus_ring() {
    set_visuals(Visuals::light());
    let v = current_visuals();
    let (mut ctl, _) = control(&["A", "B"], 0);
    ctl.layout(Size::new(600.0, 0.0));
    assert_eq!(painted(&mut ctl).strokes_with(v.accent_focus), 0);
    ctl.on_event(&Event::FocusGained);
    assert_eq!(painted(&mut ctl).strokes_with(v.accent_focus), 1);
}

#[test]
fn per_corner_path_degenerates_to_plain_rect_with_zero_radii() {
    let mut rec = PaintRecorder::new();
    rec.begin_path();
    SegmentedControl::path_rect_corners(&mut rec, Rect::new(0.0, 0.0, 10.0, 4.0), 0.0, 0.0);
    rec.fill();
    assert_eq!(rec.fills.len(), 1);
    assert_eq!(
        rec.fills[0].kind,
        crate::tests::paint_recorder::PathKind::Freeform
    );
}

#[test]
fn empty_control_is_inert() {
    let cell = Rc::new(Cell::new(0));
    let labels: Vec<String> = Vec::new();
    let mut ctl = SegmentedControl::new(labels, cell, test_font());
    let size = ctl.layout(Size::new(100.0, 0.0));
    assert_eq!(size.width, 0.0);
    assert!(!ctl.is_focusable());
    assert_eq!(key(&mut ctl, Key::ArrowRight), EventResult::Ignored);
    let _ = painted(&mut ctl);
}
