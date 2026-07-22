//! Tests for [`TextArea::with_scroll_watch`] — the publish channel that lets a
//! sibling widget (the Code Editor demo's line-number gutter) mirror the
//! editor's vertical scroll. Kept in its own module so `tests.rs` stays under
//! the 800-line cap. See `text_area/scroll.rs` for the publish sites.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use super::*;
use crate::widget::Widget;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// A scroll-watch cell must mirror the live vertical offset so a sibling widget
/// (the line-number gutter) can follow the viewport. This is the agg-gui half of
/// the "gutter doesn't track scroll" regression.
#[test]
fn scroll_watch_cell_mirrors_offset() {
    let watch = Rc::new(Cell::new(-1.0));
    let text = (0..200)
        .map(|i| format!("line{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut ta = TextArea::new(font())
        .with_font_size(13.0)
        .with_text(text)
        .with_scroll_watch(Rc::clone(&watch));
    ta.layout(Size::new(200.0, 80.0));

    // Layout publishes the initial (top-of-document) offset.
    assert_eq!(watch.get(), 0.0, "watch starts at the top after layout");
    assert!(ta.max_scroll_y() > 0.0, "200 lines must overflow an 80px box");

    // A wheel notch moves the offset — the cell must track it, not lag a frame.
    assert!(ta.scroll_by_wheel(-40.0), "wheel must move the offset");
    assert!(ta.scroll_offset() > 0.0);
    assert_eq!(
        watch.get(),
        ta.scroll_offset(),
        "watch cell must mirror the live scroll offset after a wheel scroll"
    );

    // Scroll-to-caret (Ctrl+End style jump) must also update the cell.
    ta.set_cursor_to_end();
    ta.ensure_cursor_visible();
    assert_eq!(
        watch.get(),
        ta.scroll_offset(),
        "watch cell must mirror the offset after scroll-to-caret"
    );
}
