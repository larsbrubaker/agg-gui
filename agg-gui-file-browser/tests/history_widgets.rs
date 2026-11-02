//! The history in the browser's widgets, driven through agg-gui's `App` with real key and pointer
//! events: once the list or the folder tree has the keyboard, Control+Z (Command+Z) undoes and
//! Control+Shift+Z, Command+Shift+Z or Control+Y redoes, taken even with nothing to undo; and the
//! notice (`BrowserNotice`) shows the latest change with its Undo button, whose click undoes and
//! turns it into Redo, then holds for five seconds and fades over one.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use agg_gui::text::Font;
use agg_gui::{App, EventResult, Key, Modifiers, MouseButton, Point, Rect, Size, Widget};
use agg_gui_file_browser::{
    BrowserConfig, BrowserList, BrowserNotice, BrowserState, BrowserTree, ViewMode,
};
use agg_gui_files::{HistoryStep, MemoryFolder, StepDone, StepOutcome};

const WIDTH: f64 = 400.0;
const HEIGHT: f64 = 300.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

type Log = Rc<RefCell<Vec<String>>>;

/// A step that logs each undo and redo and finishes at once.
struct Logged {
    log: Log,
}

impl Logged {
    fn finish(self: Box<Self>, what: &str, done: StepDone) {
        self.log.borrow_mut().push(what.to_string());
        done(StepOutcome {
            kept: Some(self),
            failures: Vec::new(),
        });
    }
}

impl HistoryStep for Logged {
    fn description(&self) -> String {
        "Moved gear.stl to Parts".to_string()
    }
    fn undo(self: Box<Self>, done: StepDone) {
        self.finish("undo", done);
    }
    fn redo(self: Box<Self>, done: StepDone) {
        self.finish("redo", done);
    }
}

/// Home holding the folder Parts and the file gear.stl.
fn home() -> MemoryFolder {
    let home = MemoryFolder::new("Home");
    home.add_folder("Parts").expect("Parts is free");
    home.add_file("gear.stl", b"solid".to_vec());
    home
}

/// A browser on `home()` with one recorded step, and its log.
fn browser_with_a_step() -> (Rc<RefCell<BrowserState>>, Log) {
    agg_gui::set_device_scale(1.0);
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    let log = Log::default();
    state.borrow().history().record(Box::new(Logged {
        log: Rc::clone(&log),
    }));
    (state, log)
}

/// The widget named `id` and its rectangle in the root's coordinates (Y up).
fn find<'a>(widget: &'a dyn Widget, id: &str, origin: Point) -> Option<(&'a dyn Widget, Rect)> {
    let b = widget.bounds();
    let here = Point::new(origin.x + b.x, origin.y + b.y);
    if widget.id() == Some(id) {
        return Some((widget, Rect::new(here.x, here.y, b.width, b.height)));
    }
    widget
        .children()
        .iter()
        .find_map(|child| find(child.as_ref(), id, here))
}

fn found<'a>(app: &'a App, id: &str) -> (&'a dyn Widget, Rect) {
    find(app.root(), id, Point::new(0.0, 0.0)).unwrap_or_else(|| panic!("no widget named {id}"))
}

fn property(app: &App, id: &str, key: &str) -> String {
    found(app, id)
        .0
        .properties()
        .into_iter()
        .find(|(k, _)| *k == key)
        .unwrap_or_else(|| panic!("{id} has no {key}"))
        .1
}

/// A left press and release at the middle of the widget named `id`, then a layout.
fn click(app: &mut App, id: &str) {
    let r = found(app, id).1;
    let (x, y) = (r.x + r.width / 2.0, HEIGHT - (r.y + r.height / 2.0));
    app.on_mouse_move(x, y);
    app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
    app.layout(Size::new(WIDTH, HEIGHT));
}

fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        ..Modifiers::default()
    }
}

fn command_shift() -> Modifiers {
    Modifiers {
        meta: true,
        shift: true,
        ..Modifiers::default()
    }
}

#[test]
fn the_lists_keys_undo_and_redo_once_it_has_the_keyboard() {
    let (state, log) = browser_with_a_step();
    state.borrow_mut().set_view_mode(ViewMode::Rows);
    let mut app = App::new(Box::new(BrowserList::new(&state, font())));
    app.layout(Size::new(WIDTH, HEIGHT));
    click(&mut app, "Row Item gear.stl");

    app.on_key_down(Key::Char('z'), ctrl());
    assert_eq!(*log.borrow(), ["undo"]);
    app.on_key_down(Key::Char('Z'), command_shift());
    assert_eq!(*log.borrow(), ["undo", "redo"]);
    app.on_key_down(Key::Char('z'), ctrl());
    app.on_key_down(Key::Char('y'), ctrl());
    assert_eq!(*log.borrow(), ["undo", "redo", "undo", "redo"]);
    // Nothing left to redo: the key is still the browser's.
    let history = state.borrow().history();
    assert!(!history.can_redo());
    app.on_key_down(Key::Char('y'), ctrl());
    assert_eq!(log.borrow().len(), 4);
}

#[test]
fn the_trees_keys_undo_and_redo_and_are_taken_with_nothing_to_undo() {
    let (state, log) = browser_with_a_step();
    let mut tree = BrowserTree::for_state(&state, font());
    tree.layout(Size::new(WIDTH, HEIGHT));
    let mut app = App::new(Box::new(tree));
    app.layout(Size::new(WIDTH, HEIGHT));
    click(&mut app, "Parts Tree Node");

    app.on_key_down(Key::Char('z'), ctrl());
    assert_eq!(*log.borrow(), ["undo"]);
    app.on_key_down(Key::Char('y'), ctrl());
    assert_eq!(*log.borrow(), ["undo", "redo"]);

    // Straight to the tree: with nothing to redo the key is still taken.
    let key = agg_gui::Event::KeyDown {
        key: Key::Char('y'),
        modifiers: ctrl(),
    };
    let mut tree = BrowserTree::for_state(&state, font());
    assert_eq!(tree.on_event(&key), EventResult::Consumed);
    let other = agg_gui::Event::KeyDown {
        key: Key::Char('q'),
        modifiers: ctrl(),
    };
    assert_eq!(tree.on_event(&other), EventResult::Ignored);
}

#[test]
fn the_notices_button_undoes_and_then_offers_redo() {
    let (state, log) = browser_with_a_step();
    let mut app = App::new(Box::new(BrowserNotice::new(&state, font())));
    app.layout(Size::new(WIDTH, HEIGHT));
    assert_eq!(
        property(&app, "Library Notice", "text"),
        "Moved gear.stl to Parts"
    );
    assert_eq!(property(&app, "Library Notice Button", "label"), "Undo");

    click(&mut app, "Library Notice Button");
    assert_eq!(*log.borrow(), ["undo"]);
    assert_eq!(
        property(&app, "Library Notice", "text"),
        "Undone: Moved gear.stl to Parts"
    );
    assert_eq!(property(&app, "Library Notice Button", "label"), "Redo");
    click(&mut app, "Library Notice Button");
    assert_eq!(*log.borrow(), ["undo", "redo"]);
}

#[test]
fn the_notice_holds_five_seconds_then_fades_over_one() {
    let _clock = agg_gui::clock::scoped_virtual(None);
    let (state, _) = browser_with_a_step();
    let mut app = App::new(Box::new(BrowserNotice::new(&state, font())));
    app.layout(Size::new(WIDTH, HEIGHT));
    let alpha = |app: &App| property(app, "Library Notice", "alpha");
    assert_eq!(alpha(&app), "1");
    agg_gui::clock::advance(Duration::from_millis(5000));
    app.layout(Size::new(WIDTH, HEIGHT));
    assert_eq!(alpha(&app), "1");
    agg_gui::clock::advance(Duration::from_millis(500));
    app.layout(Size::new(WIDTH, HEIGHT));
    assert_eq!(alpha(&app), "0.5");
    agg_gui::clock::advance(Duration::from_millis(600));
    app.layout(Size::new(WIDTH, HEIGHT));
    assert_eq!(alpha(&app), "0");
    assert!(!app.root().is_visible());

    // A new change shows at full strength again.
    state
        .borrow()
        .history()
        .announce("Nothing to move".to_string());
    app.layout(Size::new(WIDTH, HEIGHT));
    assert_eq!(alpha(&app), "1");
    assert_eq!(property(&app, "Library Notice", "text"), "Nothing to move");
    assert!(find(app.root(), "Library Notice Button", Point::new(0.0, 0.0)).is_none());
}

#[test]
fn the_notice_fades_on_a_clock_set_before_the_epoch_too() {
    // A virtual clock standing before the UI epoch reads zero time since the epoch until it
    // passes it: an age counted from the epoch would never grow here.
    let before = agg_gui::clock::epoch()
        .checked_sub(Duration::from_secs(60))
        .expect("a minute before the epoch is a time");
    let _clock = agg_gui::clock::scoped_virtual(Some(before));
    let (state, _) = browser_with_a_step();
    let mut app = App::new(Box::new(BrowserNotice::new(&state, font())));
    app.layout(Size::new(WIDTH, HEIGHT));
    agg_gui::clock::advance(Duration::from_millis(5500));
    app.layout(Size::new(WIDTH, HEIGHT));
    assert_eq!(property(&app, "Library Notice", "alpha"), "0.5");
}
