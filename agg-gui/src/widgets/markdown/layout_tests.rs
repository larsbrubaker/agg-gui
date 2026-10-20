//! Unit tests for `layout.rs`, pulled in via `#[path]` as its child `tests`
//! module so they reach the layout pass's private items: wide code blocks and
//! tables scroll in their own box instead of widening the document, a newly
//! loaded image takes its own size and stays dirty until painted, a relative
//! image the provider misses stays fetchable, and a context-menu click does
//! not fall through to the linked image under it.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::event::{Event, EventResult, Modifiers, MouseButton};
use crate::geometry::Point;
use crate::text::Font;
use crate::widget::Widget;

use super::super::ImagePixels;
use super::*;

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn test_font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).expect("test font"))
}

#[test]
fn wide_code_block_does_not_expand_document_width() {
    crate::widget::set_current_viewport(Size::new(220.0, 200.0));
    let mut view = MarkdownView::new(
        "```text\nthis line is intentionally much much wider than the viewport\n```",
        test_font(),
    )
    .with_font_size(12.0)
    .with_padding(8.0);

    let size = view.layout_markdown(Size::new(220.0, 1000.0));

    assert_eq!(size.width, 220.0);
    let code = view.items.iter().find_map(|item| {
        if let LayoutItem::CodeBlock {
            viewport_width,
            content_width,
            ..
        } = item
        {
            Some((*viewport_width, *content_width))
        } else {
            None
        }
    });
    let (viewport_width, content_width) = code.expect("code block item");
    assert!(content_width > viewport_width);
}

#[test]
fn wide_table_does_not_expand_document_width() {
    crate::widget::set_current_viewport(Size::new(220.0, 200.0));
    let mut view = MarkdownView::new(
        "| Column | Value |\n| --- | --- |\n| ThisIsAnExtremelyLongUnbrokenTableCell | another-long-value |",
        test_font(),
    )
    .with_font_size(12.0)
    .with_padding(8.0);

    let size = view.layout_markdown(Size::new(220.0, 1000.0));

    assert_eq!(size.width, 220.0);
    let table = view.items.iter().find_map(|item| {
        if let LayoutItem::Table {
            viewport_width,
            content_width,
            ..
        } = item
        {
            Some((*viewport_width, *content_width))
        } else {
            None
        }
    });
    let (viewport_width, content_width) = table.expect("table item");
    assert!(content_width > viewport_width);
}

/// Regression test for the SVG-badge "wrong scale on first frame"
/// bug: when an image transitions from RemotePending to Ready, the
/// next layout pass must reserve the IMAGE'S dimensions (not the
/// alt-text placeholder dimensions) in its LineRun.
#[test]
fn newly_ready_image_layout_uses_image_dimensions() {
    crate::widget::set_current_viewport(Size::new(400.0, 200.0));
    let mut view = MarkdownView::new("![badge](https://example.com/badge.svg)", test_font())
        .with_font_size(12.0)
        .with_padding(8.0);

    // First layout: image is still RemotePending — placeholder dims.
    view.layout_markdown(Size::new(400.0, 1000.0));
    let placeholder_dims = view.items.iter().find_map(|item| {
        if let LayoutItem::Line { runs, .. } = item {
            runs.iter().find_map(|run| {
                if let LineRun::Image { width, height, .. } = run {
                    Some((*width, *height))
                } else {
                    None
                }
            })
        } else {
            None
        }
    });
    let placeholder_dims = placeholder_dims.expect("first layout produced an image LineRun");

    // Image arrives — dimensions distinct from the placeholder.
    let state = Arc::clone(&view.image_cache[0].state);
    *state.lock().expect("image state") = ImageState::Ready {
        image: ImagePixels {
            data: Arc::new(vec![0u8; 200 * 40 * 4]),
            width: 200,
            height: 40,
            sequence: None,
        },
        seen: false,
    };

    // Second layout pass: the LineRun must now carry the image's
    // dimensions (not the placeholder).
    view.layout_markdown(Size::new(400.0, 1000.0));
    let ready_dims = view.items.iter().find_map(|item| {
        if let LayoutItem::Line { runs, .. } = item {
            runs.iter().find_map(|run| {
                if let LineRun::Image { width, height, .. } = run {
                    Some((*width, *height))
                } else {
                    None
                }
            })
        } else {
            None
        }
    });
    let ready_dims = ready_dims.expect("second layout produced an image LineRun");

    assert_ne!(
        ready_dims, placeholder_dims,
        "ready image must change LineRun dims away from the placeholder",
    );
    assert!(
        (ready_dims.0 - 200.0).abs() < 0.5 && (ready_dims.1 - 40.0).abs() < 0.5,
        "ready image LineRun must use the image's natural dimensions; got {ready_dims:?}",
    );
}

#[test]
fn ready_remote_image_stays_dirty_until_painted() {
    crate::widget::set_current_viewport(Size::new(220.0, 200.0));
    let mut view = MarkdownView::new("![badge](https://example.com/badge.svg)", test_font())
        .with_font_size(12.0)
        .with_padding(8.0);

    view.layout_markdown(Size::new(220.0, 1000.0));
    let state = Arc::clone(&view.image_cache[0].state);
    *state.lock().expect("image state") = ImageState::Ready {
        image: ImagePixels {
            data: Arc::new(vec![255, 0, 0, 255]),
            width: 1,
            height: 1,
            sequence: None,
        },
        seen: false,
    };

    view.layout_markdown(Size::new(220.0, 1000.0));

    assert!(
        view.needs_draw(),
        "layout must not clear a freshly loaded image before retained parents repaint"
    );
}

#[test]
fn relative_image_provider_miss_stays_fetchable() {
    crate::widget::set_current_viewport(Size::new(220.0, 200.0));
    let mut view = MarkdownView::new("![hero](agg-gui/readme_hero.png)", test_font())
        .with_font_size(12.0)
        .with_padding(8.0)
        .with_image_provider(|_| None);

    view.layout_markdown(Size::new(220.0, 1000.0));

    let state = view.image_cache[0].state.lock().expect("image state");
    assert!(
        matches!(*state, ImageState::RemotePending),
        "relative images should remain fetchable when the provider misses"
    );
}

#[test]
fn context_menu_action_click_consumes_followup_mouse_up() {
    crate::widget::set_current_viewport(Size::new(260.0, 200.0));
    let opened_link = Rc::new(Cell::new(false));
    let opened_link_for_cb = Rc::clone(&opened_link);
    let mut view = MarkdownView::new("[![hero](hero.png)](https://example.com/demo)", test_font())
        .with_font_size(12.0)
        .with_padding(8.0)
        .with_image_provider(|_| Some((vec![255; 96 * 48 * 4], 96, 48)))
        .on_link_click(move |_| opened_link_for_cb.set(true));

    view.layout_markdown(Size::new(260.0, 1000.0));
    let image = view
        .selectable_fragments
        .iter()
        .find(|fragment| {
            matches!(
                fragment.kind,
                super::super::selection::SelectableKind::Image { .. }
            )
        })
        .expect("image fragment");
    let menu_pos = Point::new(image.text_x + 8.0, image.y + image.height - 4.0);
    let copy_row_pos = Point::new(menu_pos.x + 12.0, menu_pos.y - 12.0);

    assert_eq!(
        view.on_event(&Event::MouseDown {
            pos: menu_pos,
            button: MouseButton::Right,
            modifiers: Modifiers::default(),
        }),
        EventResult::Consumed
    );
    assert_eq!(
        view.on_event(&Event::MouseDown {
            pos: copy_row_pos,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }),
        EventResult::Consumed
    );
    assert_eq!(
        view.on_event(&Event::MouseUp {
            pos: copy_row_pos,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }),
        EventResult::Consumed
    );

    assert!(
        !opened_link.get(),
        "clicking a context-menu action must not fall through to the linked image"
    );
}
