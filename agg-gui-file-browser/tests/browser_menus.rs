//! The browser's right-click menus shown by an app's own menu host (`BrowserMenus`) and the
//! Delete key running an app's own Remove row, driven through agg-gui's `App` with real events on
//! `MemoryFolder`s: the list hands its selection's and its own menu, the tree its folder's menu,
//! to the host at the press (window coordinates, Y up) and opens nothing of its own; the rows the
//! host runs act as the browser's own menu's do (the tree's on the folder clicked); a host that
//! declines leaves the browser opening its own menu; and Delete or Backspace, in the list and in
//! the tree, run the app's command marked `as_remove` when it can run, and nothing when it can't.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{App, Key, Modifiers, MouseButton, Point, Rect, Size, Widget};
use agg_gui_file_browser::{
    ActionScope, BrowserAction, BrowserActions, BrowserCommand, BrowserConfig, BrowserList,
    BrowserMenus, BrowserRef, BrowserState, BrowserTree, Builtins, MenuScope, MenuSource,
};
use agg_gui_files::MemoryFolder;

const WIDTH: f64 = 400.0;
const HEIGHT: f64 = 300.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// What ran: each run's command title and the selection it ran with.
type Ran = Rc<RefCell<Vec<String>>>;

/// An app whose menu is one selection row, "Erase", marked as the menus' Remove (enabled as
/// `erasable` says), then one folder row, "Tidy"; each records what it ran on.
struct EraseMenu {
    ran: Ran,
    erasable: bool,
}

/// `title` recording the selection of the browser `state` it runs in.
fn recording(title: &str, state: &BrowserRef, ran: &Ran) -> BrowserCommand {
    let (live, ran, name) = (state.live(), Rc::clone(ran), title.to_string());
    BrowserCommand::new(title, move || {
        let selected: Vec<String> = live.as_ref().map_or_else(Vec::new, |s| {
            s.borrow()
                .selected_entries()
                .iter()
                .map(|e| e.name().to_string())
                .collect()
        });
        ran.borrow_mut()
            .push(format!("{name} {}", selected.join(",")));
    })
}

impl BrowserActions for EraseMenu {
    fn menu(&self, state: &BrowserRef, _builtins: &Builtins) -> Vec<BrowserAction> {
        vec![
            BrowserAction::Command(
                recording("Erase", state, &self.ran)
                    .with_scope(ActionScope::Selection)
                    .enabled(self.erasable)
                    .as_remove(),
            ),
            BrowserAction::Command(
                recording("Tidy", state, &self.ran).with_scope(ActionScope::Folder),
            ),
        ]
    }
}

/// A menu host recording each menu it is offered; shows them when `shows`.
struct Host {
    offered: RefCell<Vec<(Vec<BrowserAction>, Point, MenuSource)>>,
    shows: bool,
}

impl BrowserMenus for Host {
    fn open(&self, rows: Vec<BrowserAction>, window_at: Point, source: MenuSource) -> bool {
        self.offered.borrow_mut().push((rows, window_at, source));
        self.shows
    }
}

impl Host {
    fn new(shows: bool) -> Rc<Self> {
        Rc::new(Self {
            offered: RefCell::new(Vec::new()),
            shows,
        })
    }

    /// The last menu offered: its row titles, where, and which.
    fn last(&self) -> (Vec<String>, Point, MenuSource) {
        let offered = self.offered.borrow();
        let (rows, at, source) = offered.last().expect("a menu was offered");
        (titles(rows), *at, *source)
    }

    /// Runs the last menu's row `title`.
    fn run(&self, title: &str) {
        let command = self
            .offered
            .borrow()
            .last()
            .and_then(|(rows, _, _)| {
                rows.iter().find_map(|row| match row {
                    BrowserAction::Command(c) if c.title() == title => Some(c.clone()),
                    _ => None,
                })
            })
            .unwrap_or_else(|| panic!("{title} was offered"));
        command.run();
    }
}

fn titles(rows: &[BrowserAction]) -> Vec<String> {
    rows.iter()
        .map(|row| match row {
            BrowserAction::Command(c) => c.title().to_string(),
            _ => String::new(),
        })
        .collect()
}

/// Home holding the folder Projects and the file a.txt.
fn home() -> MemoryFolder {
    let home = MemoryFolder::new("Home");
    home.add_folder("Projects").expect("Projects is free");
    home.add_file("a.txt", b"a".to_vec());
    home
}

/// A browser over [`home`] with the app's menu and `host`, showing `view` made from it.
fn browser_app(
    host: Option<Rc<Host>>,
    erasable: bool,
    view: impl FnOnce(&Rc<RefCell<BrowserState>>) -> Box<dyn Widget>,
) -> (App, Rc<RefCell<BrowserState>>, Ran) {
    agg_gui::set_device_scale(1.0);
    let ran: Ran = Rc::default();
    let mut builder = BrowserConfig::builder().actions(Rc::new(EraseMenu {
        ran: Rc::clone(&ran),
        erasable,
    }));
    if let Some(host) = host {
        builder = builder.menus(host);
    }
    let state = BrowserState::new(Box::new(home()), builder.build());
    let mut app = App::new(view(&state));
    app.layout(Size::new(WIDTH, HEIGHT));
    (app, state, ran)
}

fn list_view(state: &Rc<RefCell<BrowserState>>) -> Box<dyn Widget> {
    Box::new(BrowserList::new(state, font()))
}

fn tree_view(state: &Rc<RefCell<BrowserState>>) -> Box<dyn Widget> {
    Box::new(BrowserTree::for_state(state, font()))
}

fn find_path<'a>(
    widget: &'a dyn Widget,
    pred: &dyn Fn(&dyn Widget) -> bool,
    origin: Point,
) -> Option<(&'a dyn Widget, Rect)> {
    let b = widget.bounds();
    let here = Point::new(origin.x + b.x, origin.y + b.y);
    if pred(widget) {
        return Some((widget, Rect::new(here.x, here.y, b.width, b.height)));
    }
    widget
        .children()
        .iter()
        .find_map(|child| find_path(child.as_ref(), pred, here))
}

/// The centre of the widget named `id`, in root coordinates (Y up).
fn centre_of(app: &App, id: &str) -> Point {
    let (_, r) = find_path(app.root(), &|w| w.id() == Some(id), Point::new(0.0, 0.0))
        .unwrap_or_else(|| panic!("{id} shows"));
    Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0)
}

fn of_type<'a, T: 'static>(app: &'a App, type_name: &str) -> &'a T {
    find_path(
        app.root(),
        &|w| w.type_name() == type_name,
        Point::new(0.0, 0.0),
    )
    .and_then(|(w, _)| w.as_any())
    .and_then(|any| any.downcast_ref::<T>())
    .unwrap_or_else(|| panic!("no {type_name}"))
}

/// A press and release at `at` (root coordinates, Y up), then a layout.
fn click_at(app: &mut App, at: Point, button: MouseButton) {
    let y = HEIGHT - at.y;
    app.on_mouse_move(at.x, y);
    app.on_mouse_down_clicks(at.x, y, button, Modifiers::default(), 1);
    app.on_mouse_up(at.x, y, button, Modifiers::default());
    app.layout(Size::new(WIDTH, HEIGHT));
}

fn key(app: &mut App, key: Key) {
    app.on_key_down(key, Modifiers::default());
    app.layout(Size::new(WIDTH, HEIGHT));
}

fn near(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 0.5 && (a.y - b.y).abs() < 0.5
}

#[test]
fn the_lists_menus_go_to_the_apps_host_at_the_press_and_run_as_the_lists_own() {
    let host = Host::new(true);
    let (mut app, _state, ran) = browser_app(Some(Rc::clone(&host)), true, list_view);

    let at = centre_of(&app, "Row Item a.txt");
    click_at(&mut app, at, MouseButton::Right);
    let (rows, window_at, source) = host.last();
    assert_eq!(rows, ["Erase"], "the selection's menu");
    assert_eq!(source, MenuSource::List(MenuScope::Selection));
    assert!(near(window_at, at), "{window_at:?} is the press {at:?}");
    let list: &BrowserList = of_type(&app, "BrowserList");
    assert!(!list.menu_is_open(), "the list opens nothing of its own");
    host.run("Erase");
    assert_eq!(*ran.borrow(), ["Erase a.txt"]);

    key(&mut app, Key::Escape);
    let empty = Point::new(200.0, 5.0);
    click_at(&mut app, empty, MouseButton::Right);
    let (rows, window_at, source) = host.last();
    assert_eq!(rows, ["Tidy"], "the list's own menu");
    assert_eq!(source, MenuSource::List(MenuScope::Folder));
    assert!(
        near(window_at, empty),
        "{window_at:?} is the press {empty:?}"
    );
}

#[test]
fn the_trees_menu_goes_to_the_apps_host_and_its_rows_act_on_the_folder_clicked() {
    let host = Host::new(true);
    let (mut app, state, ran) = browser_app(Some(Rc::clone(&host)), true, tree_view);
    let at = centre_of(&app, "Projects Tree Node");
    click_at(&mut app, at, MouseButton::Right);
    let (rows, window_at, source) = host.last();
    assert_eq!(rows, ["Erase"]);
    assert_eq!(source, MenuSource::Tree);
    assert!(near(window_at, at), "{window_at:?} is the press {at:?}");
    let tree: &BrowserTree = of_type(&app, "TreeView");
    assert!(!tree.menu_is_open(), "the tree opens nothing of its own");

    // The list's selection is something else when the host runs the row.
    let a = state
        .borrow()
        .entries()
        .iter()
        .position(|e| e.name() == "a.txt");
    state.borrow_mut().go_to(0);
    if let Some(a) = a {
        BrowserState::click(&state, a, Modifiers::default());
    }
    host.run("Erase");
    assert_eq!(*ran.borrow(), ["Erase Projects"]);
    assert!(state.borrow().temporary_selection().is_none());
}

#[test]
fn a_host_that_declines_leaves_the_browser_opening_its_own_menu() {
    let host = Host::new(false);
    let (mut app, _state, _ran) = browser_app(Some(Rc::clone(&host)), true, list_view);
    let at = centre_of(&app, "Row Item a.txt");
    click_at(&mut app, at, MouseButton::Right);
    assert_eq!(host.last().0, ["Erase"], "offered first");
    let list: &BrowserList = of_type(&app, "BrowserList");
    assert!(list.menu_is_open());
    assert_eq!(list.menu_rows(), [("Erase".to_string(), true)]);
}

#[test]
fn the_delete_key_runs_the_apps_own_remove_row_in_the_list_and_the_tree() {
    let (mut app, _state, ran) = browser_app(None, true, list_view);
    let at = centre_of(&app, "Row Item a.txt");
    click_at(&mut app, at, MouseButton::Left);
    key(&mut app, Key::Delete);
    assert_eq!(*ran.borrow(), ["Erase a.txt"]);

    let (mut app, _state, ran) = browser_app(None, true, tree_view);
    let at = centre_of(&app, "Projects Tree Node");
    click_at(&mut app, at, MouseButton::Left);
    key(&mut app, Key::Backspace);
    assert_eq!(*ran.borrow(), ["Erase Projects"]);
}

#[test]
fn the_delete_key_runs_nothing_when_the_remove_row_cannot_run() {
    let (mut app, state, ran) = browser_app(None, false, list_view);
    let at = centre_of(&app, "Row Item a.txt");
    click_at(&mut app, at, MouseButton::Left);
    key(&mut app, Key::Delete);
    assert!(ran.borrow().is_empty());
    assert_eq!(state.borrow().entries().len(), 2, "nothing removed");
}
