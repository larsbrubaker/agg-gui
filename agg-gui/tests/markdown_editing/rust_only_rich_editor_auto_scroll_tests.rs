//! Rust-only: a drag held past the edge of a rich editor shorter than 2 px.
//!
//! C# `RichEditorMouse.AutoScrollStep` holds the drag's y inside the view
//! with `Math.Clamp(lastDrag.Y, 1, editor.Height - 1)`, which throws once the
//! editor is under 2 px tall (the floor passes the ceiling); `f64::clamp`
//! panics the same way. The editor holds the y with the floor winning
//! instead, so the drag keeps selecting and scrolling.

use std::sync::Arc;

use agg_gui::event::{Modifiers, MouseButton};
use agg_gui::geometry::Point;
use agg_gui::text::Font;
use agg_gui::widgets::markdown_editing::{RichEditorState, RichLayoutFonts, RichLayoutStyle};

static REGULAR: &[u8] = include_bytes!("../../../demo/assets/Nunito_Regular.ttf");

fn style() -> RichLayoutStyle {
    RichLayoutStyle::new(
        RichLayoutFonts {
            regular: Arc::new(Font::from_slice(REGULAR).expect("demo font")),
            bold: None,
            mono: None,
        },
        Some(1.0),
    )
}

#[test]
fn rust_only_a_drag_past_the_edge_of_a_view_under_2_px_does_not_panic() {
    let markdown = (0..40)
        .map(|i| format!("Paragraph {i}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    for height in [1.5, 0.5, 0.0] {
        let mut editor = RichEditorState::new(Box::new(style));
        editor.set_markdown(&markdown).unwrap();
        editor.set_viewport(400.0, height).unwrap();
        // Scrolled to the bottom, so a drag held above the view scrolls up.
        editor.set_scroll_offset_from_top(f64::MAX);
        let bottom = editor.scroll_offset_from_top();
        editor
            .mouse_down(
                Point::new(10.0, height / 2.0),
                MouseButton::Left,
                1,
                Modifiers::default(),
            )
            .unwrap();
        editor.mouse_move(Point::new(10.0, height + 50.0));
        assert!(editor.auto_scrolling(), "held above a {height} px view");
        assert!(
            editor.scroll_offset_from_top() < bottom,
            "the view scrolled up from {bottom} in a {height} px view"
        );
        assert!(editor.auto_scroll_step(), "a second step scrolls on");
    }
}

#[test]
fn rust_only_a_drag_in_a_0_1_and_2_px_tall_editor_does_not_panic() {
    let markdown = (0..40)
        .map(|i| format!("Paragraph {i}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    for height in [0.0, 1.0, 2.0] {
        let mut editor = RichEditorState::new(Box::new(style));
        editor.set_markdown(&markdown).unwrap();
        editor.set_viewport(400.0, height).unwrap();
        editor.set_scroll_offset_from_top(f64::MAX);
        let bottom = editor.scroll_offset_from_top();
        editor
            .mouse_down(
                Point::new(10.0, height / 2.0),
                MouseButton::Left,
                1,
                Modifiers::default(),
            )
            .unwrap();
        // Inside the view, then held past its top and bottom edges.
        editor.mouse_move(Point::new(10.0, height / 2.0));
        editor.mouse_move(Point::new(10.0, height + 50.0));
        assert!(editor.auto_scrolling(), "held above a {height} px view");
        assert!(
            editor.scroll_offset_from_top() < bottom,
            "the view scrolled up from {bottom} in a {height} px view"
        );
        while editor.auto_scroll_step() {}
        assert!(!editor.auto_scrolling());
        editor.mouse_move(Point::new(10.0, -50.0));
        editor.auto_scroll_step();
        editor.mouse_up();
    }
}
