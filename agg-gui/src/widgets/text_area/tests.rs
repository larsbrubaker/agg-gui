//! Unit tests for the egui-parity capabilities added to [`TextArea`]:
//! hint text, content alignment geometry, selection introspection,
//! programmatic cursor control + shared edit state, and the pre-default
//! key-chord interceptor. These exercise the production widget directly (no
//! copies) so the behaviours the TextEdit demo relies on stay correct.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use super::*;
use crate::event::{Event, EventResult, Key, Modifiers};
use crate::widget::Widget;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// Lay the widget out at a fixed size so `bounds` + the wrap cache are
/// populated before we probe geometry.
fn laid_out(mut ta: TextArea, w: f64, h: f64) -> TextArea {
    ta.layout(Size::new(w, h));
    ta
}

// ── (a) hint text ─────────────────────────────────────────────────────────

#[test]
fn hint_text_defaults_and_overrides() {
    let ta = TextArea::new(font());
    assert_eq!(ta.hint, "Type here…");
    let ta = TextArea::new(font()).with_hint_text("Type something!");
    assert_eq!(ta.hint, "Type something!");
}

// ── (b) content alignment ──────────────────────────────────────────────────

#[test]
fn resolved_alignment_prefers_cell_over_static() {
    let hcell = Rc::new(Cell::new(TextHAlign::Right));
    let vcell = Rc::new(Cell::new(TextVAlign::Bottom));
    let ta = TextArea::new(font())
        .with_content_h_align(TextHAlign::Left)
        .with_content_v_align(TextVAlign::Top)
        .with_h_align_cell(Rc::clone(&hcell))
        .with_v_align_cell(Rc::clone(&vcell));
    assert_eq!(ta.resolved_h_align(), TextHAlign::Right);
    assert_eq!(ta.resolved_v_align(), TextVAlign::Bottom);
    // Flipping the cell flips the resolved value without a rebuild.
    hcell.set(TextHAlign::Center);
    assert_eq!(ta.resolved_h_align(), TextHAlign::Center);
}

#[test]
fn horizontal_align_shifts_line_start() {
    let text = "hi"; // short line so there is slack in a wide box
    let left = laid_out(
        TextArea::new(font())
            .with_text(text)
            .with_content_h_align(TextHAlign::Left),
        400.0,
        120.0,
    );
    let center = laid_out(
        TextArea::new(font())
            .with_text(text)
            .with_content_h_align(TextHAlign::Center),
        400.0,
        120.0,
    );
    let right = laid_out(
        TextArea::new(font())
            .with_text(text)
            .with_content_h_align(TextHAlign::Right),
        400.0,
        120.0,
    );
    let l = &left.cached_lines[0];
    let c = &center.cached_lines[0];
    let r = &right.cached_lines[0];
    let lx = left.line_x_start(l);
    let cx = center.line_x_start(c);
    let rx = right.line_x_start(r);
    assert!(cx > lx, "center should start right of left ({cx} vs {lx})");
    assert!(rx > cx, "right should start right of center ({rx} vs {cx})");
}

#[test]
fn vertical_align_shifts_content_block() {
    // One short line in a tall box: Top keeps line 0 at the top edge,
    // Center/Bottom push it down (smaller Y in the Y-up frame).
    let top = laid_out(
        TextArea::new(font())
            .with_text("x")
            .with_content_v_align(TextVAlign::Top),
        300.0,
        300.0,
    );
    let center = laid_out(
        TextArea::new(font())
            .with_text("x")
            .with_content_v_align(TextVAlign::Center),
        300.0,
        300.0,
    );
    let bottom = laid_out(
        TextArea::new(font())
            .with_text("x")
            .with_content_v_align(TextVAlign::Bottom),
        300.0,
        300.0,
    );
    assert_eq!(top.v_align_shift(), 0.0);
    assert!(center.v_align_shift() > 0.0);
    assert!(bottom.v_align_shift() > center.v_align_shift());
    // Top edge of line 0 moves DOWN (smaller Y) as we go Top→Center→Bottom.
    assert!(top.line_top_y(0) > center.line_top_y(0));
    assert!(center.line_top_y(0) > bottom.line_top_y(0));
}

// ── (c) selection introspection ─────────────────────────────────────────────

#[test]
fn selection_and_selected_text() {
    let ta = TextArea::new(font()).with_text("hello world");
    // Fresh: cursor at end, no selection.
    assert_eq!(ta.selection(), None);
    assert_eq!(ta.selected_text(), "");
    // Select "world" via the shared state.
    {
        let st = ta.edit_state();
        let mut st = st.borrow_mut();
        st.anchor = 6;
        st.cursor = 11;
    }
    assert_eq!(ta.selection(), Some((6, 11)));
    assert_eq!(ta.selected_text(), "world");
    // Reversed cursor/anchor still yields a sorted range.
    {
        let st = ta.edit_state();
        let mut st = st.borrow_mut();
        st.anchor = 11;
        st.cursor = 6;
    }
    assert_eq!(ta.selection(), Some((6, 11)));
    assert_eq!(ta.selected_text(), "world");
}

// ── (d) programmatic cursor control + shared edit state ─────────────────────

#[test]
fn cursor_to_start_and_end() {
    let mut ta = TextArea::new(font()).with_text("abcdef");
    ta.set_cursor_to_start();
    {
        let st = ta.edit_state();
        let st = st.borrow();
        assert_eq!((st.cursor, st.anchor), (0, 0));
    }
    ta.set_cursor_to_end();
    {
        let st = ta.edit_state();
        let st = st.borrow();
        assert_eq!((st.cursor, st.anchor), (6, 6));
    }
}

#[test]
fn shared_edit_state_survives_across_widgets() {
    // The demo shares one state handle across rebuilt TextAreas; a mutation
    // through the handle must be visible to a second widget built from it.
    let shared = Rc::new(RefCell::new(TextEditState {
        text: "seed".into(),
        cursor: 4,
        anchor: 4,
        epoch: 0,
    }));
    let a = TextArea::new(font()).with_edit_state(Rc::clone(&shared));
    let b = TextArea::new(font()).with_edit_state(Rc::clone(&shared));
    assert_eq!(a.text(), "seed");
    assert_eq!(b.text(), "seed");
    {
        let mut st = shared.borrow_mut();
        st.text = "changed".into();
        st.note_text_change();
    }
    assert_eq!(a.text(), "changed");
    assert_eq!(b.text(), "changed");
}

#[test]
fn external_text_change_reflows_via_epoch() {
    // A long word forced to wrap. After an external replace with narrow text
    // (bumping the epoch), the wrap cache must re-wrap to a single line even
    // though the layout width is unchanged.
    let shared = Rc::new(RefCell::new(TextEditState {
        text: "aaaaaaaaaa bbbbbbbbbb cccccccccc dddddddddd".into(),
        cursor: 0,
        anchor: 0,
        epoch: 0,
    }));
    let mut ta =
        laid_out(TextArea::new(font()).with_edit_state(Rc::clone(&shared)), 80.0, 200.0);
    let wrapped = ta.visual_line_count();
    assert!(wrapped > 1, "expected multi-line wrap, got {wrapped}");
    {
        let mut st = shared.borrow_mut();
        st.text = "hi".into();
        st.cursor = 2;
        st.anchor = 2;
        st.note_text_change();
    }
    // Same width — only the epoch changed. Re-layout and confirm the cache
    // reflowed to a single line.
    ta.layout(Size::new(80.0, 200.0));
    assert_eq!(ta.visual_line_count(), 1);
}

#[test]
fn focus_id_is_exposed() {
    let ta = TextArea::new(font());
    assert_eq!(ta.focus_id(), None);
    let ta = TextArea::new(font()).with_focus_id(4242);
    assert_eq!(ta.focus_id(), Some(4242));
}

// ── (e) pre-default key-chord interceptor ───────────────────────────────────

#[test]
fn key_intercept_runs_before_default_and_can_consume() {
    let fired = Rc::new(Cell::new(0u32));
    let fired2 = Rc::clone(&fired);
    let mut ta = laid_out(
        TextArea::new(font()).with_text("abc").with_key_intercept(
            move |key, mods| {
                // Consume Ctrl/Cmd+Y only; let everything else fall through.
                if (mods.ctrl || mods.meta) && matches!(key, Key::Char('y') | Key::Char('Y')) {
                    fired2.set(fired2.get() + 1);
                    true
                } else {
                    false
                }
            },
        ),
        200.0,
        80.0,
    );
    ta.on_event(&Event::FocusGained);

    // Ctrl+Y is intercepted: consumed, no text change.
    let res = ta.on_event(&Event::KeyDown {
        key: Key::Char('y'),
        modifiers: Modifiers {
            ctrl: true,
            ..Default::default()
        },
    });
    assert_eq!(res, EventResult::Consumed);
    assert_eq!(fired.get(), 1);
    assert_eq!(ta.text(), "abc");

    // A plain 'z' is NOT intercepted → default handling inserts it.
    let res = ta.on_event(&Event::KeyDown {
        key: Key::Char('z'),
        modifiers: Modifiers::default(),
    });
    assert_eq!(res, EventResult::Consumed);
    assert_eq!(fired.get(), 1);
    assert_eq!(ta.text(), "abcz");
}

// ── (f) on_change callback ──────────────────────────────────────────────────
//
// Mirrors TextField's `on_change`: fires after every mutation path so a caller
// can capture edits back into a shared cell.

/// Drive one focused KeyDown with default modifiers.
fn key(ta: &mut TextArea, k: Key) {
    ta.on_event(&Event::KeyDown {
        key: k,
        modifiers: Modifiers::default(),
    });
}

#[test]
fn on_change_fires_for_typing_delete_and_records_latest_text() {
    let seen = Rc::new(RefCell::new(Vec::<String>::new()));
    let seen2 = Rc::clone(&seen);
    let mut ta = laid_out(
        TextArea::new(font()).on_change(move |s| seen2.borrow_mut().push(s.to_string())),
        200.0,
        80.0,
    );
    ta.on_event(&Event::FocusGained);

    // Typing: one callback per inserted char, each carrying the running text.
    key(&mut ta, Key::Char('h'));
    key(&mut ta, Key::Char('i'));
    // Delete (backspace): fires again.
    key(&mut ta, Key::Backspace);

    let seen = seen.borrow();
    assert_eq!(*seen, vec!["h".to_string(), "hi".to_string(), "h".to_string()]);
}

#[test]
fn on_change_fires_for_key_intercept_edit_when_epoch_advances() {
    // An interceptor that mutates the shared state and calls `note_text_change`
    // must trigger `on_change`, matching the built-in mutation funnels.
    let changed = Rc::new(Cell::new(0u32));
    let changed2 = Rc::clone(&changed);
    let state = Rc::new(RefCell::new(TextEditState {
        text: "abc".into(),
        cursor: 0,
        anchor: 3, // whole-line selection so the toggle has something to edit
        epoch: 0,
    }));
    let state_key = Rc::clone(&state);
    let mut ta = laid_out(
        TextArea::new(font())
            .with_edit_state(Rc::clone(&state))
            .with_key_intercept(move |key, mods| {
                if (mods.ctrl || mods.meta) && matches!(key, Key::Char('y') | Key::Char('Y')) {
                    let mut st = state_key.borrow_mut();
                    if let Some((lo, hi)) = st.selection_range() {
                        let up = st.text[lo..hi].to_uppercase();
                        st.text.replace_range(lo..hi, &up);
                        st.note_text_change();
                    }
                    true
                } else {
                    false
                }
            })
            .on_change(move |_| changed2.set(changed2.get() + 1)),
        200.0,
        80.0,
    );
    ta.on_event(&Event::FocusGained);
    ta.on_event(&Event::KeyDown {
        key: Key::Char('y'),
        modifiers: Modifiers {
            ctrl: true,
            ..Default::default()
        },
    });
    assert_eq!(changed.get(), 1, "epoch-advancing intercept must fire on_change");
    assert_eq!(state.borrow().text, "ABC");
}

#[test]
fn on_change_silent_for_intercept_without_text_edit() {
    // An interceptor that consumes the key but leaves text untouched (epoch
    // unchanged) must NOT fire on_change.
    let changed = Rc::new(Cell::new(0u32));
    let changed2 = Rc::clone(&changed);
    let mut ta = laid_out(
        TextArea::new(font())
            .with_text("abc")
            .with_key_intercept(|_key, _mods| true) // consume everything, edit nothing
            .on_change(move |_| changed2.set(changed2.get() + 1)),
        200.0,
        80.0,
    );
    ta.on_event(&Event::FocusGained);
    key(&mut ta, Key::Char('z'));
    assert_eq!(changed.get(), 0, "no text change → no on_change");
}

// ── (h) internal vertical scrolling ─────────────────────────────────────────
//
// Exercises the scroll-to-cursor math and wheel handling against the real
// widget. `multiline(n, w, h)` builds a TextArea with `n` hard-broken lines in
// an `h`-tall box; a small box makes the content overflow so `max_scroll_y > 0`.

/// Build a laid-out TextArea whose content is `n` hard-broken lines. The
/// initial cursor sits at the end (last line), matching `with_text`.
fn multiline(n: usize, w: f64, h: f64) -> TextArea {
    let text = (0..n).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n");
    laid_out(
        TextArea::new(font()).with_font_size(13.0).with_text(text),
        w,
        h,
    )
}

#[test]
fn scroll_to_cursor_reveals_last_line_and_first_line() {
    let mut ta = multiline(12, 200.0, 80.0);
    let max = ta.max_scroll_y();
    assert!(max > 0.0, "12 lines in an 80px box must overflow; max={max}");

    // Cursor is at the end → scroll should pin the bottom of the last line to
    // the bottom of the viewport, i.e. offset == max_scroll.
    ta.ensure_cursor_visible();
    assert!(
        (ta.scroll_offset() - max).abs() < 0.5,
        "caret at end must scroll to bottom: off={} max={max}",
        ta.scroll_offset()
    );

    // Move the caret to the very start → scroll back to the top.
    ta.set_cursor_to_start();
    ta.ensure_cursor_visible();
    assert_eq!(
        ta.scroll_offset(),
        0.0,
        "caret at start must scroll to the top"
    );
}

#[test]
fn caret_geometry_moves_on_screen_after_scroll_to_cursor() {
    // Before scrolling, the last line sits below the viewport (negative Y in
    // the Y-up frame); after scroll-to-cursor it lands inside [0, height].
    let mut ta = multiline(12, 200.0, 80.0);
    let cursor = ta.cursor();
    let y_before = ta.pos_for_cursor(cursor).y;
    assert!(
        y_before < 0.0,
        "caret should start off the bottom of the viewport, got y={y_before}"
    );
    ta.ensure_cursor_visible();
    let y_after = ta.pos_for_cursor(cursor).y;
    assert!(
        (0.0..=80.0).contains(&y_after),
        "caret must be on-screen after scroll: y={y_after}"
    );
}

#[test]
fn content_that_fits_never_scrolls() {
    let mut ta = multiline(3, 300.0, 300.0);
    assert_eq!(ta.max_scroll_y(), 0.0, "3 lines fit a 300px box");
    ta.ensure_cursor_visible();
    assert_eq!(ta.scroll_offset(), 0.0);
    assert!(
        !ta.scroll_by_wheel(-40.0),
        "wheel is a no-op when nothing overflows"
    );
    assert_eq!(ta.scroll_offset(), 0.0);
}

#[test]
fn wheel_scrolls_within_bounds_and_clamps() {
    let mut ta = multiline(12, 200.0, 80.0);
    let max = ta.max_scroll_y();

    // Positive delta means "see content above"; a negative delta scrolls the
    // content down (offset grows).
    assert!(ta.scroll_by_wheel(-40.0), "wheel must move the offset");
    assert!(ta.scroll_offset() > 0.0);

    // Spinning far down clamps at max_scroll (and then reports no movement).
    for _ in 0..50 {
        ta.scroll_by_wheel(-40.0);
    }
    assert!((ta.scroll_offset() - max).abs() < 0.5);
    assert!(!ta.scroll_by_wheel(-40.0), "clamped at the bottom");

    // Spinning back up returns to the top.
    for _ in 0..50 {
        ta.scroll_by_wheel(40.0);
    }
    assert_eq!(ta.scroll_offset(), 0.0);
}

#[test]
fn caret_visible_segment_clamps_and_hides_off_screen() {
    use super::widget_impl::caret_visible_segment;
    // Inner band [8, 72]. A caret line fully inside is returned unchanged.
    assert_eq!(caret_visible_segment(20.0, 18.0, 8.0, 72.0), Some((20.0, 38.0)));
    // A caret straddling the bottom edge is clamped up to `inner_lo`.
    assert_eq!(caret_visible_segment(0.0, 18.0, 8.0, 72.0), Some((8.0, 18.0)));
    // Straddling the top edge is clamped down to `inner_hi`.
    assert_eq!(caret_visible_segment(64.0, 18.0, 8.0, 72.0), Some((64.0, 72.0)));
    // A line scrolled entirely below the inner rect draws nothing.
    assert_eq!(caret_visible_segment(-40.0, 18.0, 8.0, 72.0), None);
    // Entirely above the inner rect also draws nothing.
    assert_eq!(caret_visible_segment(200.0, 18.0, 8.0, 72.0), None);
}

#[test]
fn key_intercept_edit_scrolls_caret_into_view() {
    // An interceptor that moves the caret to the very end of a long buffer
    // (and bumps the epoch) must scroll it into view, mirroring the built-in
    // edit funnel — otherwise the caret would sit off-screen after the edit.
    let state = Rc::new(RefCell::new(TextEditState {
        text: (0..30).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n"),
        cursor: 0,
        anchor: 0,
        epoch: 0,
    }));
    let state_key = Rc::clone(&state);
    let mut ta = laid_out(
        TextArea::new(font())
            .with_font_size(13.0)
            .with_edit_state(Rc::clone(&state))
            .with_key_intercept(move |key, _mods| {
                if matches!(key, Key::Char('g')) {
                    // Jump the caret to the document end and append a char so
                    // the epoch advances (text actually changed).
                    let mut st = state_key.borrow_mut();
                    st.text.push('!');
                    let end = st.text.len();
                    st.cursor = end;
                    st.anchor = end;
                    st.note_text_change();
                    true
                } else {
                    false
                }
            }),
        200.0,
        80.0,
    );
    ta.on_event(&Event::FocusGained);
    assert_eq!(ta.scroll_offset(), 0.0, "starts at the top");
    ta.on_event(&Event::KeyDown {
        key: Key::Char('g'),
        modifiers: Modifiers::default(),
    });
    assert!(
        ta.scroll_offset() > 0.0,
        "intercept edit at document end must scroll the caret into view, got {}",
        ta.scroll_offset()
    );
    let cursor = ta.cursor();
    let y = ta.pos_for_cursor(cursor).y;
    assert!(
        (0.0..=80.0).contains(&y),
        "caret must be on-screen after intercept edit: y={y}"
    );
}

#[test]
fn first_line_top_not_clipped_at_scroll_top() {
    // Bug 1: at scroll offset 0 the first visual line's glyph top
    // (baseline + ascent) must not poke above the padded inner-rect clip edge,
    // otherwise the ascenders of the first line are visibly cut off. Asserts
    // via `line_baseline_y`, the very helper `paint` uses to place glyphs.
    let mut ta = multiline(12, 200.0, 80.0);
    ta.set_cursor_to_start();
    ta.ensure_cursor_visible();
    assert_eq!(ta.scroll_offset(), 0.0, "scrolled to the very top");

    let inner_top = ta.bounds.height - ta.padding; // Y-up clip top edge
    let ascent = ta.font.ascender_px(ta.font_size);
    let glyph_top = ta.line_baseline_y(0) + ascent;
    assert!(
        glyph_top <= inner_top + 0.01,
        "first line ascenders clipped: glyph_top={glyph_top} > inner_top={inner_top}"
    );
}

// ── (g) highlight segmentation ──────────────────────────────────────────────
//
// The highlighter paint path must split a line into gap-free, non-overlapping
// colour segments so every glyph is filled exactly once (no double-paint on
// AA fringes). These exercise the production `segment_highlight` directly.

use super::widget_impl::segment_highlight;
use crate::color::Color;

const BASE: Color = Color::rgb(1.0, 1.0, 1.0);
const RUN: Color = Color::rgb(1.0, 0.0, 0.0);

/// Every byte of the text is covered by exactly one segment, in order, with
/// no overlaps and no gaps.
fn assert_covers_once(text: &str, segs: &[(usize, usize, Color)]) {
    let mut pos = 0usize;
    for &(s, e, _) in segs {
        assert_eq!(s, pos, "segment start must abut the previous end: {segs:?}");
        assert!(e > s, "segment must be non-empty: {segs:?}");
        pos = e;
    }
    assert_eq!(pos, text.len(), "segments must cover the whole line: {segs:?}");
}

#[test]
fn segment_highlight_fills_gaps_and_runs_once() {
    let text = "let x = 1;";
    // Colour "let" and "1" only; the rest are gaps in the base colour.
    let spans = [(0usize, 3usize, RUN), (8usize, 9usize, RUN)];
    let segs = segment_highlight(text, &spans, BASE);
    assert_covers_once(text, &segs);
    assert_eq!(
        segs,
        vec![
            (0, 3, RUN),   // "let"
            (3, 8, BASE),  // " x = "
            (8, 9, RUN),   // "1"
            (9, 10, BASE), // ";"
        ]
    );
}

#[test]
fn segment_highlight_no_spans_is_single_base_run() {
    let text = "plain";
    let segs = segment_highlight(text, &[], BASE);
    assert_eq!(segs, vec![(0, 5, BASE)]);
}

#[test]
fn segment_highlight_drops_invalid_and_resolves_overlap() {
    let text = "abcdef";
    // Reversed, out-of-range, and non-char-boundary-safe-but-overlapping spans.
    let spans = [
        (2usize, 2usize, RUN),  // empty → dropped
        (4usize, 3usize, RUN),  // reversed → dropped
        (0usize, 10usize, RUN), // out of range → dropped
        (0usize, 3usize, RUN),  // valid
        (2usize, 5usize, BASE), // overlaps the previous → clamped to [3,5)
    ];
    let segs = segment_highlight(text, &spans, BASE);
    assert_covers_once(text, &segs);
    // First span wins bytes 0..3; the overlapper contributes only 3..5.
    assert_eq!(
        segs,
        vec![(0, 3, RUN), (3, 5, BASE), (5, 6, BASE)]
    );
}

#[test]
fn segment_highlight_empty_text_is_empty() {
    assert!(segment_highlight("", &[(0, 0, RUN)], BASE).is_empty());
}
