//! Wheel routing over a [`MarkdownView`] code block that scrolls sideways,
//! inside a vertical [`ScrollView`]: the block takes a wheel's sideways part
//! only when the gesture is mostly sideways, and the vertical part always
//! reaches the scroll view, so a trackpad's vertical scroll (which carries a
//! little sideways drift) still scrolls the page over a wide code block.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::event::Modifiers;
use crate::geometry::{Point, Size};
use crate::text::Font;
use crate::widget::{App, Widget};
use crate::widgets::ScrollView;

use super::{LayoutItem, MarkdownView};

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");
const SIDE: f64 = 300.0;

/// A 300 x 300 app: a scroll view over markdown that opens with a code block
/// much wider than the view, followed by enough paragraphs to scroll.
fn app_over_wide_code() -> (App, Rc<Cell<f64>>) {
    crate::widget::set_current_viewport(Size::new(SIDE, SIDE));
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font"));
    let mut markdown = format!("```\n{}\nsecond line\n```\n", "x".repeat(400));
    for i in 0..60 {
        markdown.push_str(&format!("\nParagraph {i} of the reply.\n"));
    }
    let offset = Rc::new(Cell::new(0.0));
    let scroll = ScrollView::new(Box::new(MarkdownView::new(&markdown, font)))
        .with_offset_cell(Rc::clone(&offset));
    let mut app = App::new(Box::new(scroll));
    app.layout(Size::new(SIDE, SIDE));
    (app, offset)
}

fn view(app: &App) -> &MarkdownView {
    app.root().children()[0]
        .as_any()
        .expect("as_any")
        .downcast_ref::<MarkdownView>()
        .expect("markdown view")
}

/// The middle of the code block, in the app's y-down screen coordinates.
fn over_code_block(app: &App) -> Point {
    let view = view(app);
    let (y, height) = view
        .items
        .iter()
        .find_map(|item| match item {
            LayoutItem::CodeBlock {
                y,
                height,
                viewport_width,
                content_width,
                ..
            } => {
                assert!(content_width > viewport_width, "the block scrolls sideways");
                Some((*y, *height))
            }
            _ => None,
        })
        .expect("a code block");
    let at = view.bounds();
    let up_y = at.y + y + height * 0.5;
    assert!(up_y > 0.0 && up_y < SIDE, "the block is on screen");
    Point::new(at.x + view.padding + 20.0, SIDE - up_y)
}

fn block_offset(app: &App) -> f64 {
    view(app).block_scroll_offset(0)
}

fn wheel(app: &mut App, at: Point, delta_x: f64, delta_y: f64) {
    app.on_mouse_wheel_xy_mods(at.x, at.y, delta_x, delta_y, Modifiers::default());
}

#[test]
fn a_trackpad_scroll_with_sideways_drift_scrolls_the_page_over_a_wide_code_block() {
    let (mut app, offset) = app_over_wide_code();
    let at = over_code_block(&app);

    wheel(&mut app, at, 0.02, -3.0);

    assert!(
        offset.get() > 0.0,
        "the page scrolled down: {}",
        offset.get()
    );
    assert_eq!(block_offset(&app), 0.0, "the block stays where it was");
}

#[test]
fn a_mostly_sideways_wheel_scrolls_the_code_block() {
    let (mut app, offset) = app_over_wide_code();
    let at = over_code_block(&app);

    wheel(&mut app, at, -2.0, 0.0);
    assert!(block_offset(&app) > 0.0, "the block scrolled right");
    assert_eq!(offset.get(), 0.0, "the page stays put");

    // Mostly sideways with a little vertical: the block takes the sideways
    // part and the page gets the vertical part.
    let before = block_offset(&app);
    wheel(&mut app, at, -2.0, -0.5);
    assert!(
        block_offset(&app) > before,
        "the block scrolled right again"
    );
    assert!(offset.get() > 0.0, "the vertical part scrolled the page");
}
