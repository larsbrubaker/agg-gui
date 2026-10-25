//! The right-click menu's Paste in a browser: like the touch toolbar's, it
//! reads the system clipboard (`clipboard::system_paste`), so text copied in
//! another app pastes even though the page's own clipboard is empty, and it
//! lands when the read resolves. `TextField`, `TextArea` and `RichTextEdit`,
//! driven by a mouse through `App`, with the browser's asynchronous read
//! simulated ([`crate::clipboard::system_paste::simulate`]).

use std::sync::Arc;

use super::touch_pan_fixtures::Board;
use crate::clipboard::system_paste;
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widgets::text_context_menu::TextMenuAction;
use crate::widgets::{Block, RichDoc, RichTextEdit, TextArea, TextField};
use crate::{App, Framebuffer, GfxCtx, Modifiers, MouseButton, Widget};

const W: f64 = 600.0;
const H: f64 = 400.0;
const RECT: Rect = Rect {
    x: 20.0,
    y: 200.0,
    width: 300.0,
    height: 120.0,
};

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(super::TEST_FONT).expect("font"))
}

fn get<T: 'static>(app: &mut App) -> &mut T {
    app.root_mut().children_mut()[0]
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<T>())
        .expect("the widget")
}

fn frame(app: &mut App) {
    app.layout(Size::new(W, H));
    let mut fb = Framebuffer::new(W as u32, H as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

fn click(app: &mut App, at: Point, button: MouseButton) {
    let m = Modifiers::default();
    app.on_mouse_down(at.x, H - at.y, button, m);
    app.on_mouse_up(at.x, H - at.y, button, m);
}

/// Focus the widget, open its menu, click Paste, and resolve the browser's
/// read with "XYZ": `text` reads the widget's text before and after.
fn paste_through_menu<T: 'static>(
    widget: Box<dyn Widget>,
    menu_center: impl Fn(&T, TextMenuAction) -> Option<Point>,
    text: impl Fn(&T) -> String,
) -> (String, String) {
    let _profile = crate::input_profile::profile_test_lock();
    crate::input_profile::set_input_profile(crate::input_profile::InputProfile::Desktop);
    crate::set_device_scale(1.0);
    crate::ux_scale::set_ux_scale(1.0);
    let _clipboard = crate::clipboard::simulate();
    let reads = system_paste::simulate();
    let mut app = App::new(Box::new(Board::new(W, H).place(RECT, widget)));
    frame(&mut app);
    let middle = Point::new(RECT.x + 40.0, RECT.y + RECT.height * 0.5);
    click(&mut app, middle, MouseButton::Left);
    app.on_mouse_down(
        middle.x,
        H - middle.y,
        MouseButton::Right,
        Modifiers::default(),
    );
    frame(&mut app);
    let row = menu_center(get::<T>(&mut app), TextMenuAction::Paste).expect("the menu shows Paste");
    let before = text(get::<T>(&mut app));
    click(
        &mut app,
        Point::new(RECT.x + row.x, RECT.y + row.y),
        MouseButton::Left,
    );
    assert_eq!(reads.pending(), 1, "Paste started a system clipboard read");
    assert_eq!(
        text(get::<T>(&mut app)),
        before,
        "nothing pasted before it resolves"
    );
    reads.resolve(Some("XYZ"));
    frame(&mut app);
    let after = text(get::<T>(&mut app));
    (before, after)
}

#[test]
fn a_text_fields_menu_paste_reads_the_system_clipboard() {
    let field = TextField::new(font()).with_text("abc");
    let (before, after) = paste_through_menu::<TextField>(
        Box::new(field),
        |f, a| f.menu_action_center(a),
        |f| f.text(),
    );
    assert_eq!(before, "abc");
    assert!(after.contains("XYZ"), "{after:?}");
    assert_eq!(after.len(), 6);
}

#[test]
fn a_text_areas_menu_paste_reads_the_system_clipboard() {
    let area = TextArea::new(font()).with_text("abc");
    let (_, after) = paste_through_menu::<TextArea>(
        Box::new(area),
        |a, x| a.menu_action_center(x),
        |a| a.text(),
    );
    assert!(after.contains("XYZ") && after.contains("abc"), "{after:?}");
}

#[test]
fn a_rich_text_edits_menu_paste_reads_the_system_clipboard() {
    let doc = RichDoc::from_blocks(vec![Block::plain("abc")]);
    let editor = RichTextEdit::new(
        doc,
        crate::widgets::rich_text::view::uniform_resolver(font()),
    );
    let (_, after) = paste_through_menu::<RichTextEdit>(
        Box::new(editor),
        |e, x| e.menu_action_center(x),
        |e| e.plain_text(),
    );
    assert!(after.contains("XYZ") && after.contains("abc"), "{after:?}");
}
