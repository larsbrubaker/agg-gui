//! Searching from the search box over a `MemoryFolder`, through agg-gui's `App` with real pointer,
//! key and text events on agg-gui's virtual clock: typing waits out the pause, then lists the
//! results under their section; a selected result stays selected when the results sort again;
//! Escape (or the clear button) returns to the folder searched and blanks the box; and a search
//! typed past is disposed, so only the latest query's results ever show.
//!
//! The section a result lists under is the place it was found in (the folder searched, for
//! everything under it).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::Widget;
use agg_gui::{App, DrawCtx, Event, EventResult, Key, Modifiers, MouseButton, Point, Rect, Size};
use agg_gui_file_browser::{
    run_ready_local_tasks, BrowserConfig, BrowserList, BrowserState, SearchBox, SearchController,
    ViewMode, DEBOUNCE,
};
use agg_gui_files::MemoryFolder;

const WIDTH: f64 = 400.0;
const HEIGHT: f64 = 400.0;
/// The search box's height, at the window's top.
const BOX: f64 = 40.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// The search box over the list, as a browser places them.
struct Column {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Column {
    fn type_name(&self) -> &'static str {
        "Column"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        let list_h = available.height - BOX;
        self.children[0].layout(Size::new(available.width, BOX));
        self.children[0].set_bounds(Rect::new(0.0, list_h, available.width, BOX));
        self.children[1].layout(Size::new(available.width, list_h));
        self.children[1].set_bounds(Rect::new(0.0, 0.0, available.width, list_h));
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Home holding gear a.stl, gear b.stl and bracket.stl, and Parts holding gear c.stl.
fn home() -> MemoryFolder {
    let home = MemoryFolder::new("Home");
    home.add_file("gear b.stl", b"b".to_vec());
    home.add_file("gear a.stl", b"a".to_vec());
    home.add_file("bracket.stl", b"c".to_vec());
    let parts = home.add_folder("Parts").expect("free");
    parts.add_file("gear c.stl", b"c".to_vec());
    home
}

struct Browser {
    app: App,
    state: Rc<RefCell<BrowserState>>,
    controller: Rc<SearchController>,
}

fn browser() -> Browser {
    agg_gui::set_device_scale(1.0);
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    state.borrow_mut().set_view_mode(ViewMode::Rows);
    let controller = SearchController::new(&state);
    let search = SearchBox::new(&state, &controller, font(), 0.0);
    let column = Column {
        bounds: Rect::default(),
        children: vec![Box::new(search), Box::new(BrowserList::new(&state, font()))],
    };
    let mut app = App::new(Box::new(column));
    app.layout(Size::new(WIDTH, HEIGHT));
    Browser {
        app,
        state,
        controller,
    }
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

fn click(app: &mut App, id: &str) {
    let r = find(app.root(), id, Point::new(0.0, 0.0))
        .unwrap_or_else(|| panic!("no widget named {id}"))
        .1;
    let (x, y) = (r.x + r.width / 2.0, HEIGHT - (r.y + r.height / 2.0));
    app.on_mouse_move(x, y);
    app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
    app.layout(Size::new(WIDTH, HEIGHT));
}

/// Types `text` into the search box, then lets the pause pass and the search run to its end.
fn search_for(b: &mut Browser, text: &str) {
    type_only(b, text);
    agg_gui::clock::advance(DEBOUNCE);
    b.app.layout(Size::new(WIDTH, HEIGHT));
    finish(b);
}

/// Types `text` into the search box (giving it the keys first), as keystrokes.
fn type_only(b: &mut Browser, text: &str) {
    click(&mut b.app, "Library Search Input");
    for c in text.chars() {
        b.app.on_text_input(&c.to_string());
    }
    b.app.layout(Size::new(WIDTH, HEIGHT));
}

/// Runs the search's work until it has landed, then a layout.
fn finish(b: &mut Browser) {
    for _ in 0..1000 {
        run_ready_local_tasks();
    }
    b.app.layout(Size::new(WIDTH, HEIGHT));
}

fn shown(b: &Browser) -> Vec<String> {
    let mut out = Vec::new();
    collect(b.app.root(), "BrowserRow", &mut out);
    out
}

fn sections(b: &Browser) -> Vec<String> {
    let mut out = Vec::new();
    collect(b.app.root(), "SectionHeaderRow", &mut out);
    out
}

fn collect(widget: &dyn Widget, kind: &str, out: &mut Vec<String>) {
    if widget.type_name() == kind {
        out.push(widget.id().unwrap_or_default().to_string());
    }
    for child in widget.children() {
        collect(child.as_ref(), kind, out);
    }
}

fn selected(b: &Browser) -> Vec<String> {
    let entries = b.state.borrow().selected_entries();
    entries.iter().map(|e| e.name().to_string()).collect()
}

#[test]
fn typing_waits_out_the_pause_then_lists_the_results_under_their_sections() {
    let _clock = agg_gui::clock::scoped_virtual(None);
    let mut b = browser();
    type_only(&mut b, "gear");
    assert!(!b.controller.is_search_active(), "not before the pause");
    agg_gui::clock::advance(DEBOUNCE);
    b.app.layout(Size::new(WIDTH, HEIGHT));
    assert!(b.controller.is_search_active());
    finish(&mut b);
    assert_eq!(b.state.borrow().depth(), 2, "the results stand below Home");
    assert_eq!(
        shown(&b),
        [
            "Row Item gear a.stl",
            "Row Item gear b.stl",
            "Row Item gear c.stl"
        ]
    );
    // The results found under Home, Parts' among them, are Home's section.
    assert_eq!(sections(&b), ["Search Section Home"]);
}

#[test]
fn a_selected_result_stays_selected_when_the_results_sort_again() {
    let _clock = agg_gui::clock::scoped_virtual(None);
    let mut b = browser();
    search_for(&mut b, "gear");
    click(&mut b.app, "Row Item gear b.stl");
    assert_eq!(selected(&b), ["gear b.stl"]);
    b.state.borrow_mut().set_user_sort_ascending(false);
    b.app.layout(Size::new(WIDTH, HEIGHT));
    assert_eq!(
        shown(&b),
        [
            "Row Item gear c.stl",
            "Row Item gear b.stl",
            "Row Item gear a.stl"
        ]
    );
    assert_eq!(selected(&b), ["gear b.stl"]);
}

#[test]
fn escape_returns_to_the_folder_searched_with_its_selection_and_blanks_the_box() {
    let _clock = agg_gui::clock::scoped_virtual(None);
    let mut b = browser();
    click(&mut b.app, "Row Item bracket.stl");
    assert_eq!(selected(&b), ["bracket.stl"]);
    search_for(&mut b, "gear");
    assert_eq!(b.state.borrow().depth(), 2);

    b.app.on_key_down(Key::Escape, Modifiers::default());
    b.app.layout(Size::new(WIDTH, HEIGHT));
    assert_eq!(b.state.borrow().path_names(), ["Home"]);
    assert!(!b.controller.is_search_active());
    let text = find(b.app.root(), "Library Search Panel", Point::new(0.0, 0.0))
        .expect("the box")
        .0
        .properties();
    assert_eq!(text, [("text", String::new())]);
    assert_eq!(selected(&b), ["bracket.stl"]);

    // The clear button does the same.
    search_for(&mut b, "gear");
    click(&mut b.app, "Close Search");
    assert_eq!(b.state.borrow().path_names(), ["Home"]);
}

#[test]
fn a_search_typed_past_is_disposed_and_only_the_latest_query_shows() {
    let _clock = agg_gui::clock::scoped_virtual(None);
    let mut b = browser();
    type_only(&mut b, "gear");
    agg_gui::clock::advance(DEBOUNCE);
    b.app.layout(Size::new(WIDTH, HEIGHT));
    let first = b
        .controller
        .current_results()
        .expect("the first search runs");
    // More typing before the first search has landed.
    for c in " b".chars() {
        b.app.on_text_input(&c.to_string());
    }
    agg_gui::clock::advance(DEBOUNCE);
    b.app.layout(Size::new(WIDTH, HEIGHT));
    let second = b
        .controller
        .current_results()
        .expect("the second search runs");
    assert!(!Rc::ptr_eq(&first, &second));
    finish(&mut b);
    assert_eq!(second.query(), "gear b");
    assert_eq!(shown(&b), ["Row Item gear b.stl"]);
    assert_eq!(
        b.state.borrow().depth(),
        2,
        "one set of results on the path"
    );
    // The search typed past stopped: it was disposed, not left running.
    assert!(!first.is_searching());
}
