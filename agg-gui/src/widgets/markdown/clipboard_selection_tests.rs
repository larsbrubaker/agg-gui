//! Tests ported 1:1 from agg-sharp
//! `Tests/Agg.Tests/MarkdigAgg/MarkdownClipboardSelectionTests.cs`: copying a
//! [`MarkdownView`] selection writes its words as plain text and its styled
//! HTML, a full selection copies the whole document's styled HTML, a
//! selection is painted, and the read-only right-click menu copies.
//! C#'s `SimulatedClipboard` is [`crate::clipboard::simulate`].

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, Modifiers, MouseButton};
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::widget::Widget;

use super::MarkdownView;

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn view_at(markdown: &str, size: Size) -> MarkdownView {
    crate::widget::set_current_viewport(size);
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font"));
    let mut view = MarkdownView::new(markdown, font);
    view.layout(size);
    view.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    view
}

fn view(markdown: &str) -> MarkdownView {
    view_at(markdown, Size::new(600.0, 2000.0))
}

fn select(view: &mut MarkdownView, text: &str) {
    let start = view
        .plain_text()
        .find(text)
        .expect("text in the plain text");
    view.set_selection(start, start + text.len());
}

fn html() -> String {
    crate::clipboard::get_html().unwrap_or_default()
}

#[test]
fn copy_selection_stores_plain_text_and_heading_html() {
    let _clipboard = crate::clipboard::simulate();
    let mut widget = view("# Preview Heading\n\nBody copy target.\n");
    select(&mut widget, "Preview Heading");

    assert!(widget.try_copy_selection_to_clipboard());
    assert_eq!(
        crate::clipboard::get_text().as_deref(),
        Some("Preview Heading")
    );
    assert!(html().contains("<h1 style="), "{}", html());
    assert!(html().contains(">Preview Heading</h1>"), "{}", html());
}

#[test]
fn full_selection_uses_markdown_html_export() {
    let _clipboard = crate::clipboard::simulate();
    let mut widget = view("# Title\n\nParagraph text.\n");
    let len = widget.plain_text().len();
    widget.set_selection(0, len);

    assert!(widget.try_copy_selection_to_clipboard());
    let html = html();
    assert!(html.contains("<h1 style="), "{html}");
    assert!(html.contains(">Title</h1>"), "{html}");
    assert!(html.contains("<p style="), "{html}");
    assert!(html.contains(">Paragraph text.</p>"), "{html}");
}

#[test]
fn full_selection_uses_inline_styles_suitable_for_email_clients() {
    let _clipboard = crate::clipboard::simulate();
    let mut widget = view(
        "# Styled Title\n\n- First item\n- Second item\n\n```cs\nConsole.WriteLine(\"Hello\");\n```\n",
    );
    let len = widget.plain_text().len();
    widget.set_selection(0, len);

    assert!(widget.try_copy_selection_to_clipboard());
    let html = html();
    assert!(html.contains("<h1 style="), "{html}");
    assert!(html.contains("font-size:20pt"), "{html}");
    assert!(html.contains("<pre style="), "{html}");
    assert!(html.contains("font-family:Consolas"), "{html}");
}

fn render(widget: &mut MarkdownView, size: Size) -> Vec<u8> {
    let mut fb = Framebuffer::new(size.width as u32, size.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.set_fill_color(Color::white());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, size.width, size.height);
        ctx.fill();
        widget.paint(&mut ctx);
    }
    fb.pixels().to_vec()
}

#[test]
fn selection_changes_rendered_pixels_to_show_highlight() {
    let size = Size::new(400.0, 120.0);
    let mut widget = view_at("# Preview Heading\n\nBody copy target.\n", size);
    let before = render(&mut widget, size);

    select(&mut widget, "Preview Heading");
    let after = render(&mut widget, size);

    assert_ne!(before, after);
}

#[test]
fn partial_selection_preserves_rendered_markdown_structure_in_html() {
    let _clipboard = crate::clipboard::simulate();
    let mut widget = view(
        "Ignore before.\n\n**Bold text**\n\n~~Strike~~ and `inline code` and [MatterCAD](https://example.com/docs)\n\n---\n\n- First bullet\n- Second bullet\n  - Nested bullet\n\n| Feature | Status |\n| --- | --- |\n| Links | Ready |\n\nIgnore after.\n",
    );
    let text = widget.plain_text().to_string();
    let start = text.find("Bold text").expect("bold text");
    let end = text.find("Ready").expect("ready") + "Ready".len();
    widget.set_selection(start, end);

    assert!(widget.try_copy_selection_to_clipboard());
    let html = html();
    assert!(html.contains("<strong style="), "{html}");
    assert!(html.contains("<del style="), "{html}");
    assert!(html.contains("<code style="), "{html}");
    assert!(
        html.contains("<a href=\"https://example.com/docs\" style="),
        "{html}"
    );
    assert!(html.contains("<hr style="), "{html}");
    assert!(html.contains("<ul style="), "{html}");
    assert!(html.contains("<li style="), "{html}");
    assert!(html.contains("<table style="), "{html}");
    assert!(html.contains("<th style="), "{html}");
    assert!(html.contains("<td style="), "{html}");
}

#[test]
fn right_click_menu_shows_readonly_markdown_actions_and_can_copy_selection() {
    let _clipboard = crate::clipboard::simulate();
    let size = Size::new(600.0, 300.0);
    let mut widget = view_at("# Preview Heading\n\nBody copy target.\n", size);
    let selection_text = "Preview Heading";
    select(&mut widget, selection_text);
    widget.on_event(&Event::MouseDown {
        pos: Point::new(10.0, 10.0),
        button: MouseButton::Right,
        modifiers: Modifiers::default(),
    });

    assert_eq!(widget.context_menu_item_enabled("Cut"), Some(false));
    assert_eq!(widget.context_menu_item_enabled("Copy"), Some(true));
    assert_eq!(widget.context_menu_item_enabled("Paste"), Some(false));
    assert_eq!(widget.context_menu_item_enabled("Select All"), Some(true));

    let copy = widget.context_menu_item_center("Copy").expect("copy row");
    widget.on_event(&Event::MouseDown {
        pos: copy,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });

    assert_eq!(
        crate::clipboard::get_text().as_deref(),
        Some(selection_text)
    );
    assert!(html().contains(">Preview Heading</h1>"), "{}", html());
}

#[test]
fn rust_only_a_link_in_a_table_cell_copies_as_a_link() {
    let _clipboard = crate::clipboard::simulate();
    let mut widget = view(
        "Before.\n\n| Item | Notes |\n| --- | --- |\n| Docs | See [Guide](https://example.com/guide) **now** |\n",
    );
    select(&mut widget, "Item");

    assert!(widget.try_copy_selection_to_clipboard());
    let html = html();
    let cell = html
        .split("<td")
        .find(|cell| cell.contains("Guide"))
        .unwrap_or_else(|| panic!("a cell with the link: {html}"));
    assert!(
        cell.contains("<a href=\"https://example.com/guide\" style="),
        "{html}"
    );
    assert!(cell.contains(">Guide</a>"), "{html}");
    // Only the link's own text is inside the anchor.
    assert!(
        !cell.contains("See </a>") && !cell.contains("now</a>"),
        "{html}"
    );
}
