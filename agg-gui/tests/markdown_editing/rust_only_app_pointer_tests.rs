//! The rich editor under a real `App`: pointer events go in through `App`'s routing (hit test,
//! click-to-focus, capture, click counts), not into the widgets directly.  A press and release
//! place the caret, a double click selects a word, a toolbar button click bolds the selection,
//! and a click on a link follows the editor's link rule (only the command-key click follows it).
//! No C# counterpart: C#'s editor tests send events straight to the widget.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::event::{Modifiers, MouseButton};
use agg_gui::geometry::Point;
use agg_gui::text::Font;
use agg_gui::widget::set_current_viewport;
use agg_gui::widgets::markdown_editing::{
    DocPosition, MarkdownFormatButtonKind as K, RichBlockGeometry, RichCaret, RichMarkdownEditor,
    RichSelection,
};
use agg_gui::{App, Color, Framebuffer, GfxCtx, Rect, Size, Widget};

use super::rich_markdown_edit_widget_tests::fonts;

const SIZE: Size = Size {
    width: 640.0,
    height: 240.0,
};
const URL: &str = "https://example.test/x";

fn icons() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::FONT_AWESOME_4_7).expect("font awesome"))
}

/// An `App` around an editor holding "Hello brave world" and a paragraph with a link, laid out
/// and painted once, with the link-clicked urls collected.
fn app() -> (App, Rc<RefCell<Vec<String>>>) {
    agg_gui::set_device_scale(1.0);
    agg_gui::theme::set_visuals(agg_gui::theme::Visuals::light());
    set_current_viewport(SIZE);
    let mut editor = RichMarkdownEditor::new(fonts(), icons(), None);
    editor
        .set_markdown("Hello brave world\n\nGo [there](https://example.test/x) now\n")
        .unwrap();
    editor.editor_mut().set_use_mac_key_bindings(false);
    let urls = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&urls);
    editor
        .editor_mut()
        .on_link_clicked(Box::new(move |url| seen.borrow_mut().push(url.to_string())));
    let mut app = App::new(Box::new(editor));
    frame(&mut app);
    (app, urls)
}

/// One frame: layout, then paint (the shell does both each frame).
fn frame(app: &mut App) {
    app.layout(SIZE);
    let mut fb = Framebuffer::new(SIZE.width as u32, SIZE.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::white());
    app.paint(&mut ctx);
}

fn editor(app: &App) -> &RichMarkdownEditor {
    app.root()
        .as_any()
        .and_then(|w| w.downcast_ref::<RichMarkdownEditor>())
        .expect("the root is the rich editor")
}

/// Screen coordinates (Y down) of the caret box centre at `offset` in text block `block`.
fn screen_of(app: &App, block: usize, offset: usize) -> (f64, f64) {
    let editor = editor(app);
    let widget = editor.editor();
    let width = widget.state().style().expect("laid out").caret_width();
    let view = widget.state().view();
    let rect = view
        .layout_of(block)
        .unwrap()
        .caret_rect(RichCaret::new(offset), width)
        .expect("a caret box");
    let center = rect.center();
    let in_editor = widget.view_to_editor(Point::new(
        center.x,
        center.y + view.origin_of(block).unwrap(),
    ));
    let origin = widget.bounds();
    (
        in_editor.x + origin.x,
        SIZE.height - (in_editor.y + origin.y),
    )
}

fn click(app: &mut App, (x, y): (f64, f64), clicks: u32, modifiers: Modifiers) {
    app.on_mouse_move_mods(x, y, modifiers);
    app.on_mouse_down_clicks(x, y, MouseButton::Left, modifiers, clicks);
    app.on_mouse_up(x, y, MouseButton::Left, modifiers);
    frame(app);
}

fn selection(app: &App) -> RichSelection {
    editor(app).editor().selection()
}

fn span(a: (usize, usize), b: (usize, usize)) -> RichSelection {
    RichSelection::new(DocPosition::new(a.0, a.1), DocPosition::new(b.0, b.1))
}

#[test]
fn rust_only_an_app_press_and_release_place_the_caret_and_focus_the_editor() {
    let (mut app, _) = app();
    let on_word = screen_of(&app, 0, 8);
    click(&mut app, on_word, 1, Modifiers::default());
    assert_eq!(selection(&app), RichSelection::at(DocPosition::new(0, 8)));
    assert_eq!(
        app.focused_widget_type_name(),
        Some("RichMarkdownEditWidget")
    );
}

#[test]
fn rust_only_an_app_double_click_on_a_word_selects_it() {
    let (mut app, _) = app();
    let on_brave = screen_of(&app, 0, 8);
    click(&mut app, on_brave, 1, Modifiers::default());
    click(&mut app, on_brave, 2, Modifiers::default());
    assert_eq!(selection(&app), span((0, 6), (0, 11)), "\"brave\"");
}

#[test]
fn rust_only_an_app_toolbar_bold_click_bolds_the_selection() {
    let (mut app, _) = app();
    let on_brave = screen_of(&app, 0, 8);
    click(&mut app, on_brave, 1, Modifiers::default());
    click(&mut app, on_brave, 2, Modifiers::default());
    assert_eq!(selection(&app), span((0, 6), (0, 11)));

    let bold = bold_button_center(&app);
    click(&mut app, bold, 1, Modifiers::default());

    assert_eq!(
        editor(&app).markdown(),
        "Hello **brave** world\n\nGo [there](https://example.test/x) now\n"
    );
    assert_eq!(
        selection(&app),
        span((0, 6), (0, 11)),
        "the selection stays on the word"
    );
}

#[test]
fn rust_only_an_app_click_on_a_link_follows_it_only_with_the_command_key() {
    let (mut app, urls) = app();
    let on_link = screen_of(&app, 1, 5);

    click(&mut app, on_link, 1, Modifiers::default());
    assert!(
        urls.borrow().is_empty(),
        "a plain click on a link is just a click"
    );
    assert_eq!(selection(&app), RichSelection::at(DocPosition::new(1, 5)));

    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    click(&mut app, on_link, 1, ctrl);
    assert_eq!(*urls.borrow(), vec![URL.to_string()]);

    // Off the link, a command-key click is an ordinary click.
    let off_link = screen_of(&app, 1, 1);
    click(&mut app, off_link, 1, ctrl);
    assert_eq!(urls.borrow().len(), 1);
}

/// The centre, in screen coordinates, of the toolbar's Bold button.
fn bold_button_center(app: &App) -> (f64, f64) {
    fn walk(widget: &dyn Widget, origin: (f64, f64), out: &mut Vec<Rect>) {
        for child in widget.children() {
            let b = child.bounds();
            let at = (origin.0 + b.x, origin.1 + b.y);
            if child.type_name() == "MarkdownFormatButton" {
                out.push(Rect::new(at.0, at.1, b.width, b.height));
            }
            walk(child.as_ref(), at, out);
        }
    }
    let mut buttons = Vec::new();
    walk(app.root(), (0.0, 0.0), &mut buttons);
    buttons.sort_by(|a, b| a.x.total_cmp(&b.x));
    let index = K::ALL
        .iter()
        .position(|k| *k == K::Bold)
        .expect("a Bold button");
    let rect = buttons[index];
    (
        rect.x + rect.width * 0.5,
        SIZE.height - (rect.y + rect.height * 0.5),
    )
}

/// The same double click as a shell sends it: two presses through the `InputForwarder`, which
/// counts the clicks itself (no stated count anywhere).
#[test]
fn rust_only_an_app_forwarded_double_click_selects_the_word() {
    use agg_gui::shell_input::{ClickCount, ForwarderEvent, InputForwarder};

    let (mut app, _) = app();
    let (x, y) = screen_of(&app, 0, 8);
    let mut forwarder = InputForwarder::new();
    forwarder.platform(
        &mut app,
        ForwarderEvent::MouseMove {
            x,
            y,
            modifiers: None,
        },
    );
    for _ in 0..2 {
        forwarder.platform(
            &mut app,
            ForwarderEvent::MouseDown {
                at: None,
                button: MouseButton::Left,
                modifiers: None,
                clicks: ClickCount::Auto,
            },
        );
        forwarder.platform(
            &mut app,
            ForwarderEvent::MouseUp {
                at: None,
                button: MouseButton::Left,
                modifiers: None,
            },
        );
    }
    frame(&mut app);
    assert_eq!(selection(&app), span((0, 6), (0, 11)), "\"brave\"");
}

/// A press, a drag across the text and a release select what was dragged over.
#[test]
fn rust_only_an_app_drag_selects_the_text_between_press_and_release() {
    let (mut app, _) = app();
    let from = screen_of(&app, 0, 2);
    let to = screen_of(&app, 0, 9);
    app.on_mouse_move(from.0, from.1);
    app.on_mouse_down(from.0, from.1, MouseButton::Left, Modifiers::default());
    app.on_mouse_move(to.0, to.1);
    app.on_mouse_up(to.0, to.1, MouseButton::Left, Modifiers::default());
    frame(&mut app);
    assert_eq!(selection(&app), span((0, 2), (0, 9)));
}
