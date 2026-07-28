//! Tests for [`TextArea::with_scroll_watch`] — the publish channel that lets a
//! sibling widget (the Code Editor demo's line-number gutter) mirror the
//! editor's vertical scroll *and* its source-line-to-visual-row mapping. Kept in
//! its own module so `tests.rs` stays under the 800-line cap. See
//! `text_area/scroll.rs` for the publish sites.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use super::*;
use crate::widget::Widget;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn watch() -> Rc<RefCell<TextAreaScrollInfo>> {
    Rc::new(RefCell::new(TextAreaScrollInfo::default()))
}

/// The offset must mirror the live vertical offset so the gutter follows the
/// viewport. This is the agg-gui half of the "gutter doesn't track scroll"
/// regression.
#[test]
fn scroll_watch_offset_mirrors_live_offset() {
    let w = watch();
    let text = (0..200)
        .map(|i| format!("line{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut ta = TextArea::new(font())
        .with_font_size(13.0)
        .with_text(text)
        .with_scroll_watch(Rc::clone(&w));
    ta.layout(Size::new(200.0, 80.0));

    // Layout publishes the initial (top-of-document) offset.
    assert_eq!(w.borrow().offset_px, 0.0, "watch starts at the top");
    assert!(
        ta.max_scroll_y() > 0.0,
        "200 lines must overflow an 80px box"
    );

    // A wheel notch moves the offset — the binding must track it, not lag.
    assert!(ta.scroll_by_wheel(-40.0), "wheel must move the offset");
    assert!(ta.scroll_offset() > 0.0);
    assert_eq!(
        w.borrow().offset_px,
        ta.scroll_offset(),
        "offset after wheel"
    );

    // Scroll-to-caret (Ctrl+End style jump) must also update the binding.
    ta.set_cursor_to_end();
    ta.ensure_cursor_visible();
    assert_eq!(
        w.borrow().offset_px,
        ta.scroll_offset(),
        "offset after scroll-to-caret"
    );
}

/// `source_line_rows` must account for soft-wrapping: a source line that wraps
/// into several visual rows pushes every later source line's first-row index
/// down by the extra rows. This is what the gutter needs to number wrapped
/// files correctly — the follow-up regression.
#[test]
fn source_line_rows_maps_wrapped_lines() {
    let w = watch();
    // Middle source line is long; a narrow width forces it to soft-wrap; the
    // other two are short enough to stay single-row.
    let text = "short\none two three four five six seven eight nine ten\nlast";
    let mut ta = TextArea::new(font())
        .with_font_size(13.0)
        .with_text(text)
        .with_scroll_watch(Rc::clone(&w));
    ta.layout(Size::new(70.0, 400.0));

    let rows = w.borrow().source_line_rows.clone();
    assert_eq!(rows.len(), 3, "three source lines");
    assert_eq!(rows[0], 0, "line 1 starts at row 0");
    assert_eq!(
        rows[1], 1,
        "line 2 starts on the row after single-row line 1"
    );
    let wrapped_rows = rows[2] - rows[1];
    assert!(
        wrapped_rows >= 3,
        "the long middle line must wrap into 3+ visual rows, got {wrapped_rows}"
    );
    assert!(
        rows[2] < ta.visual_line_count(),
        "the last source line's first row must land within the visual rows"
    );
}

/// The map must refresh when an edit changes wrapping: shrinking the long
/// middle line so it no longer wraps collapses the mapping back to `rows[i]==i`.
#[test]
fn source_line_rows_update_after_edit_changes_wrapping() {
    let w = watch();
    let text = "short\none two three four five six seven eight nine ten\nlast";
    let mut ta = TextArea::new(font())
        .with_font_size(13.0)
        .with_text(text)
        .with_scroll_watch(Rc::clone(&w));
    ta.layout(Size::new(70.0, 400.0));
    assert!(
        w.borrow().source_line_rows[2] > 2,
        "precondition: the middle line wraps before the edit"
    );

    // Replace the wrapping middle line with a short one (an external edit
    // through the shared state, as the demo's buffer edits are).
    {
        let st = ta.edit_state();
        let mut st = st.borrow_mut();
        st.text = "short\ntiny\nlast".to_string();
        st.cursor = 0;
        st.anchor = 0;
        st.note_text_change();
    }
    ta.layout(Size::new(70.0, 400.0));

    assert_eq!(
        w.borrow().source_line_rows,
        vec![0, 1, 2],
        "with no wrapping the map degenerates to rows[i]==i"
    );
}
