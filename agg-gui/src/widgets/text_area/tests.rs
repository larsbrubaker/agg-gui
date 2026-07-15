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
