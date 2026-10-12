//! The rich editor inside an agg-gui `App` on a HiDPI display.  The App lays
//! its tree out in logical units and paints it under a
//! `ctx.scale(device_scale)` transform, so the editor must lay out in logical
//! units like every other widget: its text then comes out exactly as tall as
//! a `Label` at the same point size, twice as tall in device pixels at device
//! scale 2 as at 1, and a click lands on the word drawn under it.  (C#'s
//! widgets laid out in device pixels with no paint transform; the ported
//! tests elsewhere paint the editor the way the App does.)

use agg_gui::widgets::markdown_editing::RichMarkdownEditWidget;
use agg_gui::widgets::Label;
use agg_gui::{App, Color, Framebuffer, GfxCtx, Modifiers, MouseButton, Size};

use super::rich_markdown_edit_widget_tests::fonts;

const LOGICAL: Size = Size {
    width: 400.0,
    height: 120.0,
};

/// The App around `root` laid out and painted at `scale` into a white
/// device-pixel framebuffer (Y-up).
fn paint_in_app(app: &mut App, scale: f64) -> Framebuffer {
    let (w, h) = (LOGICAL.width * scale, LOGICAL.height * scale);
    app.layout(Size::new(w, h));
    let mut fb = Framebuffer::new(w as u32, h as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        app.paint(&mut ctx);
    }
    fb
}

fn editor_app() -> App {
    let mut editor = RichMarkdownEditWidget::new(fonts(), None);
    editor.set_markdown("Hello world\n").unwrap();
    App::new(Box::new(editor))
}

fn editor_of(app: &mut App) -> &mut RichMarkdownEditWidget {
    app.root_mut()
        .as_any_mut()
        .and_then(|w| w.downcast_mut::<RichMarkdownEditWidget>())
        .expect("the root is the editor")
}

fn inked(fb: &Framebuffer, x: u32, y: u32) -> bool {
    let i = ((y * fb.width() + x) * 4) as usize;
    fb.pixels()[i] < 160
}

/// The inked rows' height and the inked columns, over the whole buffer.
fn ink_extent(fb: &Framebuffer) -> (u32, Vec<u32>) {
    let (w, h) = (fb.width(), fb.height());
    let rows: Vec<u32> = (0..h)
        .filter(|&y| (0..w).any(|x| inked(fb, x, y)))
        .collect();
    let columns = (0..w)
        .filter(|&x| (0..h).any(|y| inked(fb, x, y)))
        .collect();
    let height = match (rows.first(), rows.last()) {
        (Some(lo), Some(hi)) => hi - lo + 1,
        _ => 0,
    };
    (height, columns)
}

fn at_scale<T>(scale: f64, run: impl FnOnce() -> T) -> T {
    agg_gui::set_device_scale(scale);
    let result = run();
    agg_gui::set_device_scale(1.0);
    result
}

/// The editor's body text (10 points = 13.33 px) next to a `Label` at that size.
fn label_ink_height(scale: f64) -> u32 {
    at_scale(scale, || {
        let font = fonts().regular;
        let label = Label::new("Hello world", font)
            .with_font_size(10.0 * 96.0 / 72.0)
            .with_color(Color::black());
        let mut app = App::new(Box::new(label));
        ink_extent(&paint_in_app(&mut app, scale)).0
    })
}

fn editor_ink_height(scale: f64) -> u32 {
    at_scale(scale, || {
        let mut app = editor_app();
        ink_extent(&paint_in_app(&mut app, scale)).0
    })
}

#[test]
fn rust_only_text_is_twice_as_tall_in_device_pixels_at_device_scale_2() {
    let one = editor_ink_height(1.0);
    let two = editor_ink_height(2.0);
    assert!(one > 0);
    assert!(
        (f64::from(two) - 2.0 * f64::from(one)).abs() <= 2.0,
        "ink height {two} at scale 2 vs {one} at scale 1"
    );
}

#[test]
fn rust_only_text_matches_a_label_at_the_same_point_size() {
    for scale in [1.0, 2.0] {
        let editor = editor_ink_height(scale);
        let label = label_ink_height(scale);
        assert!(
            (f64::from(editor) - f64::from(label)).abs() <= 1.0,
            "editor {editor} vs label {label} at scale {scale}"
        );
    }
}

#[test]
fn rust_only_a_click_on_a_word_places_the_caret_in_it_at_device_scale_2() {
    at_scale(2.0, || {
        let mut app = editor_app();
        let fb = paint_in_app(&mut app, 2.0);
        let (_, columns) = ink_extent(&fb);
        // The widest gap between inked columns is the space between the words.
        let (gap_at, _) = columns
            .windows(2)
            .enumerate()
            .max_by_key(|(_, pair)| pair[1] - pair[0])
            .expect("ink");
        let world = &columns[gap_at + 1..];
        let middle = f64::from(world[world.len() / 2]);
        // The row the text is drawn on, in Y-down screen pixels.
        let rows: Vec<u32> = (0..fb.height())
            .filter(|&y| inked(&fb, world[0], y) || inked(&fb, world[world.len() / 2], y))
            .collect();
        let y_up = f64::from(rows[rows.len() / 2]);
        let screen_y = f64::from(fb.height()) - y_up;
        app.on_mouse_down(middle, screen_y, MouseButton::Left, Modifiers::default());
        app.on_mouse_up(middle, screen_y, MouseButton::Left, Modifiers::default());
        let caret = editor_of(&mut app).selection().caret;
        assert_eq!(caret.block_index, 0);
        assert!(
            (7..=10).contains(&caret.offset),
            "a click in the middle of \"world\" put the caret at {}",
            caret.offset
        );
    });
}
