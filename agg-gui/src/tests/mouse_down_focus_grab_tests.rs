//! agg-sharp `MouseDownFocusGrabTests`
//! (Tests/Agg.Tests/Agg.UI/MouseDownFocusGrabTests.cs): a press that lands
//! somewhere that cannot take the keyboard moves the focus off a text field,
//! which is what commits its edit (EditComplete), and a press handler that
//! drops some other widget's focus does not leave the keyboard stranded.
//!
//! Translation: in agg-sharp the focus grab hands the keyboard to the widget
//! the press landed in (the "Host", the "Toolbar"), and keys travel down the
//! children that contain the focus, so a focus left on the parentless
//! `SystemWindow` dead-ends them. agg-gui's click-to-focus rule
//! (`widget/app/pointer.rs`) gives focus only to focusable widgets and
//! otherwise clears it, and with nothing focused a key takes the
//! unconsumed-key path through the visible tree. So "the host takes the
//! keyboard" is asserted as "nothing is focused and the next key reaches the
//! tree", and "the field loses the keyboard" through its `on_edit_complete`.
//! The pressing-a-button case of the first test is
//! `button_click_focus.rs::opted_out_button_click_clears_previous_focus`; this
//! one presses blank space, which hits no focusable widget at all.

use super::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use crate::text::Font;
use crate::{DrawCtx, Event, EventResult, Rect};

const WINDOW: Size = Size {
    width: 600.0,
    height: 400.0,
};

/// A widget that takes the mouse but declines the keyboard - a toolbar
/// button, in the shape the widget tree sees it - and runs `on_press` from
/// its mouse-down handler (C#'s `MouseDownCaptured`).
struct DeclinesFocusWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    on_press: Box<dyn FnMut()>,
    pressed: Rc<Cell<bool>>,
}

impl Widget for DeclinesFocusWidget {
    fn type_name(&self) -> &'static str {
        "DeclinesFocusWidget"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        Size::new(available.width, 40.0)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn is_focusable(&self) -> bool {
        false
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown { .. } => {
                self.pressed.set(true);
                (self.on_press)();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

/// A field (focus id `id`) whose committed edits land in the returned cell.
fn field(id: u64) -> (TextField, Rc<RefCell<Vec<String>>>) {
    let completed = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&completed);
    let f = TextField::new(Arc::new(Font::from_slice(TEST_FONT).unwrap()))
        .with_font_size(14.0)
        .with_focus_id(id)
        .on_edit_complete(move |t| sink.borrow_mut().push(t.to_string()));
    (f, completed)
}

/// Records every key that went unconsumed by the focused widget and the tree.
fn record_unconsumed(app: &mut App) -> Rc<RefCell<Vec<Key>>> {
    let unconsumed = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&unconsumed);
    app.set_global_key_handler(move |key, _| {
        sink.borrow_mut().push(key);
        true
    });
    unconsumed
}

/// The load-bearing case: pressing blank space takes the keyboard off
/// whatever had it. A text field losing the focus this way is what commits
/// its edit, so this is the path EditComplete hangs off.
#[test]
fn pressing_blank_space_moves_the_focus_onto_the_widget_owning_it() {
    const FIELD_ID: u64 = 4101;
    let (field, completed) = field(FIELD_ID);
    let mut host = Container::new().with_padding(10.0);
    host.children_mut().push(Box::new(field));
    let mut app = App::new(Box::new(host));
    let unconsumed = record_unconsumed(&mut app);
    crate::focus::request_focus(FIELD_ID);
    app.layout(WINDOW);
    assert_eq!(
        app.focused_widget_type_name(),
        Some("TextField"),
        "the fixture starts with the field holding the keyboard"
    );
    app.on_key_down(Key::Char('7'), Modifiers::default());
    assert!(unconsumed.borrow().is_empty(), "the field takes the typing");

    // the far corner of the host (Y-down), which nothing else covers
    let (x, y) = (WINDOW.width - 20.0, WINDOW.height - 20.0);
    app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(x, y, MouseButton::Left, Modifiers::default());

    assert!(
        app.focused_widget_type_name().is_none(),
        "pressing away from a field has to take the keyboard off it - that is what commits the edit"
    );
    assert_eq!(
        *completed.borrow(),
        vec!["7".to_string()],
        "the field lost the keyboard and committed its edit, once"
    );
    app.on_key_down(Key::Char('8'), Modifiers::default());
    assert_eq!(
        *unconsumed.borrow(),
        vec![Key::Char('8')],
        "the next key goes to the tree the press landed in, not into the field"
    );
}

/// The press handler itself unfocusing something in another part of the tree
/// is not the focus "moving on" to somewhere new: it must not leave the
/// keyboard where key events dead-end, nor linger to blur a later focus.
#[test]
fn a_handler_unfocusing_elsewhere_does_not_stand_the_grab_down() {
    const FIELD_ID: u64 = 4102;
    let (field, completed) = field(FIELD_ID);
    let pressed = Rc::new(Cell::new(false));
    // what a command button does: acts on the field, which drops the field's
    // claim on the keyboard
    let button = DeclinesFocusWidget {
        bounds: Rect::default(),
        children: Vec::new(),
        on_press: Box::new(|| crate::focus::release_focus(FIELD_ID)),
        pressed: Rc::clone(&pressed),
    };
    let mut toolbar = Container::new().with_padding(5.0);
    toolbar.children_mut().push(Box::new(button));
    let mut window = Container::new().with_padding(10.0);
    window.children_mut().push(Box::new(field));
    window.children_mut().push(Box::new(toolbar));
    let mut app = App::new(Box::new(window));
    let unconsumed = record_unconsumed(&mut app);
    crate::focus::request_focus(FIELD_ID);
    app.layout(WINDOW);
    assert_eq!(app.focused_widget_type_name(), Some("TextField"));
    app.on_key_down(Key::Char('z'), Modifiers::default());

    // Below the field (Y-down): inside the toolbar's button.
    let (x, y) = (100.0, 70.0);
    app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
    app.layout(WINDOW);
    assert!(pressed.get(), "the press reached the button");

    assert!(
        app.focused_widget_type_name().is_none(),
        "the button declines the keyboard, so the field's claim on it is gone"
    );
    assert_eq!(
        *completed.borrow(),
        vec!["z".to_string()],
        "the field lost the keyboard, and committed its edit, exactly once"
    );
    assert!(
        crate::focus::take_blur_request().is_none(),
        "the handler's unfocus was settled by the frame, not left pending"
    );
    app.on_key_down(Key::Char('q'), Modifiers::default());
    assert_eq!(
        *unconsumed.borrow(),
        vec![Key::Char('q')],
        "key events reach the tree rather than dead-ending"
    );

    // A later focus is not undone by the earlier handler's unfocus.
    crate::focus::request_focus(FIELD_ID);
    app.layout(WINDOW);
    assert_eq!(
        app.focused_widget_type_name(),
        Some("TextField"),
        "the field can take the keyboard back"
    );
}
