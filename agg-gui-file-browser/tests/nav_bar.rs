//! The navigation pieces over a browser on a `MemoryFolder`, through agg-gui's `App` with real
//! pointer and key events: the breadcrumb's links and up button move the browser; the view and
//! sorting menus show the state's choices marked and a pick changes the state and the folder's
//! remembered view; the Undo and Redo buttons follow the history (greyed with nothing to do, the
//! step and key in the tooltip, the key as the look's hint while hovered) and act; and the
//! splitter drags the tree's width and remembers it.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::Widget;
use agg_gui::{App, DrawCtx, Event, EventResult, Key, Modifiers, MouseButton, Point, Rect, Size};
use agg_gui_file_browser::{
    folder_bread_crumb_for, history_key_text, sorting_button, view_options_button, BrowserConfig,
    BrowserLook, BrowserMemory, BrowserNames, BrowserSplitter, BrowserState, BrowserStyle,
    DefaultLook, HistoryButtons, ViewMode, ViewState, SPLITTER_WIDTH,
};
use agg_gui_files::{HistoryStep, MemoryFolder, SortKey, StepDone, StepOutcome};

const WIDTH: f64 = 400.0;
const HEIGHT: f64 = 300.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// A row of widgets along the window's top, each as wide as it asks.
struct Bar {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Bar {
    fn type_name(&self) -> &'static str {
        "Bar"
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
        let mut x = 0.0;
        for child in &mut self.children {
            let size = child.layout(Size::new(available.width - x, 40.0));
            let width = size.width.max(32.0);
            child.set_bounds(Rect::new(x, available.height - 40.0, width, 32.0));
            x += width + 8.0;
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn app_of(children: Vec<Box<dyn Widget>>) -> App {
    agg_gui::set_device_scale(1.0);
    let mut app = App::new(Box::new(Bar {
        bounds: Rect::default(),
        children,
    }));
    app.layout(Size::new(WIDTH, HEIGHT));
    app
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

fn click_at(app: &mut App, at: Point) {
    let y = HEIGHT - at.y;
    app.on_mouse_move(at.x, y);
    app.on_mouse_down(at.x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(at.x, y, MouseButton::Left, Modifiers::default());
    app.layout(Size::new(WIDTH, HEIGHT));
}

fn click(app: &mut App, id: &str) {
    let r = found(app, id).1;
    click_at(app, Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0));
}

/// Home holding Projects (holding Brackets) and a file.
fn home() -> MemoryFolder {
    let home = MemoryFolder::new("Home");
    let projects = home.add_folder("Projects").expect("free");
    projects.add_folder("Brackets").expect("free");
    home.add_file("gear.stl", b"solid".to_vec());
    home
}

/// Opens the folders named `names` from the root down.
fn open_down(state: &Rc<RefCell<BrowserState>>, names: &[&str]) {
    for name in names {
        let link = state
            .borrow()
            .active_container()
            .expect("a folder shows")
            .child_containers()
            .into_iter()
            .find(|l| l.name() == *name)
            .expect("listed");
        state.borrow_mut().open(link.as_ref());
    }
}

#[test]
fn the_breadcrumb_and_up_button_move_the_browser() {
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    open_down(&state, &["Projects", "Brackets"]);
    let look = Rc::clone(&state.borrow().config().look);
    let names = state.borrow().path_names();
    let go = Rc::clone(&state);
    let go_to: Rc<dyn Fn(usize)> = Rc::new(move |depth| go.borrow_mut().go_to(depth));
    let crumb = folder_bread_crumb_for(&names, go_to, &look, &font());
    let mut app = app_of(vec![Box::new(crumb)]);
    click(&mut app, "Bread Crumb Button Projects");
    assert_eq!(state.borrow().path_names(), ["Home", "Projects"]);
    // The trail was built at Brackets: its up button goes to the folder above Brackets.
    click(&mut app, "Library Up Button");
    assert_eq!(state.borrow().path_names(), ["Home", "Projects"]);
}

/// Memory recording each view remembered.
#[derive(Default)]
struct Remembering {
    views: RefCell<Vec<(Vec<String>, ViewMode)>>,
    width: RefCell<Option<f64>>,
}

impl BrowserMemory for Remembering {
    fn set_view(&self, path_names: &[String], view: &ViewState) {
        self.views
            .borrow_mut()
            .push((path_names.to_vec(), view.view_mode));
    }
    fn tree_width(&self) -> Option<f64> {
        *self.width.borrow()
    }
    fn set_tree_width(&self, width: f64) {
        *self.width.borrow_mut() = Some(width);
    }
}

/// Opens the menu of the button named `id`, moves down to its `steps`th row (the first Down
/// lights the first row; separators are skipped) and picks it with Enter.
fn pick(app: &mut App, id: &str, steps: usize) {
    click(app, id);
    assert_eq!(property(app, id, "menu_open"), "true");
    for _ in 0..steps {
        app.on_key_down(Key::ArrowDown, Modifiers::default());
    }
    app.on_key_down(Key::Enter, Modifiers::default());
    app.layout(Size::new(WIDTH, HEIGHT));
}

fn menu_rows(app: &App, id: &str) -> Vec<(String, bool)> {
    let (widget, _) = found(app, id);
    let any = widget.as_any();
    any.and_then(|a| a.downcast_ref::<agg_gui_file_browser::NavMenuButton>())
        .expect("a menu button")
        .menu_rows()
}

#[test]
fn the_view_and_sorting_menus_change_the_state_and_its_memory() {
    let memory = Rc::new(Remembering::default());
    let config = BrowserConfig::builder().memory(memory.clone()).build();
    let state = BrowserState::new(Box::new(home()), config);
    let view = view_options_button(&state, &font(), true);
    let sort = sorting_button(&state, &font());
    let mut app = app_of(vec![Box::new(view), Box::new(sort)]);

    let id = "Library View Options Button";
    assert_eq!(property(&app, id, "tooltip"), "View Settings");
    click(&mut app, id);
    let rows = menu_rows(&app, id);
    let titles: Vec<&str> = rows.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Show Folders",
            "",
            "View List",
            "View Icons",
            "View Large Icons"
        ]
    );
    assert!(rows[0].1, "folders show");
    click_at(&mut app, Point::new(WIDTH - 5.0, 5.0));
    assert_eq!(
        property(&app, id, "menu_open"),
        "false",
        "a click outside closes it"
    );

    // The icon view is marked; down past Show Folders, View List and View Icons (the separator
    // is skipped) to View Large Icons.
    assert!(rows[3].1, "the icon view shows");
    pick(&mut app, id, 4);
    assert_eq!(state.borrow().view_mode(), ViewMode::Icons256);
    assert_eq!(
        memory.views.borrow().last().map(|(p, m)| (p.clone(), *m)),
        Some((vec!["Home".to_string()], ViewMode::Icons256))
    );
    pick(&mut app, id, 1);
    assert!(
        !state.borrow().show_containers(),
        "Show Folders toggled off"
    );

    let sort_id = "Print Library View Options";
    assert_eq!(property(&app, sort_id, "tooltip"), "Sorting");
    pick(&mut app, sort_id, 1);
    assert!(state.borrow().sort().has_flag(SortKey::CreatedDate));
    // Past the keys and the separator, to Descending.
    pick(&mut app, sort_id, 5);
    assert!(!state.borrow().sort().ascending);
    click(&mut app, sort_id);
    let marked: Vec<String> = menu_rows(&app, sort_id)
        .into_iter()
        .filter(|(_, m)| *m)
        .map(|(t, _)| t)
        .collect();
    assert_eq!(marked, ["Date Created", "Descending"]);
}

/// A step that logs each undo and redo.
struct Logged(Rc<RefCell<Vec<String>>>);

impl HistoryStep for Logged {
    fn description(&self) -> String {
        "Moved gear.stl to Parts".to_string()
    }
    fn undo(self: Box<Self>, done: StepDone) {
        self.0.borrow_mut().push("undo".into());
        done(StepOutcome {
            kept: Some(self),
            failures: Vec::new(),
        });
    }
    fn redo(self: Box<Self>, done: StepDone) {
        self.0.borrow_mut().push("redo".into());
        done(StepOutcome {
            kept: Some(self),
            failures: Vec::new(),
        });
    }
}

/// The default look, recording the hints it is asked to show.
#[derive(Default)]
struct Hinting {
    inner: DefaultLook,
    hints: RefCell<Vec<Option<String>>>,
}

impl BrowserLook for Hinting {
    fn style(&self) -> BrowserStyle {
        self.inner.style()
    }
    fn names(&self) -> &BrowserNames {
        self.inner.names()
    }
    fn show_hint(&self, hint: Option<&str>) {
        self.hints.borrow_mut().push(hint.map(str::to_string));
    }
}

#[test]
fn the_undo_and_redo_buttons_follow_the_history_and_act() {
    let look = Rc::new(Hinting::default());
    let config = BrowserConfig::builder().look(look.clone()).build();
    let state = BrowserState::new(Box::new(home()), config);
    let buttons = HistoryButtons::new(&state, font());
    let mut app = app_of(vec![Box::new(buttons)]);
    let (undo, redo) = ("Library Undo Button", "Library Redo Button");
    assert_eq!(property(&app, undo, "enabled"), "false");
    assert_eq!(property(&app, redo, "enabled"), "false");

    let log = Rc::new(RefCell::new(Vec::new()));
    state
        .borrow()
        .history()
        .record(Box::new(Logged(Rc::clone(&log))));
    app.layout(Size::new(WIDTH, HEIGHT));
    let (undo_key, redo_key) = (
        history_key_text(false, &font()),
        history_key_text(true, &font()),
    );
    assert_eq!(property(&app, undo, "enabled"), "true");
    assert_eq!(
        property(&app, undo, "tooltip"),
        format!("Undo - Moved gear.stl to Parts ({undo_key})")
    );
    assert_eq!(property(&app, undo, "hint"), undo_key);

    click(&mut app, undo);
    assert_eq!(*log.borrow(), ["undo"]);
    assert!(
        look.hints.borrow().contains(&Some(undo_key.clone())),
        "hinted while hovered"
    );
    assert_eq!(property(&app, undo, "enabled"), "false");
    assert_eq!(property(&app, redo, "enabled"), "true");
    assert_eq!(
        property(&app, redo, "tooltip"),
        format!("Redo - Moved gear.stl to Parts ({redo_key})")
    );
    click(&mut app, redo);
    assert_eq!(*log.borrow(), ["undo", "redo"]);
    // Leaving the buttons clears the hint.
    app.on_mouse_move(WIDTH - 5.0, HEIGHT - 5.0);
    assert_eq!(look.hints.borrow().last(), Some(&None));
}

/// A stand-in panel.
struct Panel(Rect);

impl Widget for Panel {
    fn type_name(&self) -> &'static str {
        "Panel"
    }
    fn bounds(&self) -> Rect {
        self.0
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.0 = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &[]
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        unreachable!("a panel has no children")
    }
    fn layout(&mut self, available: Size) -> Size {
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn splitter(memory: &Rc<Remembering>) -> BrowserSplitter {
    let panel = || Box::new(Panel(Rect::default())) as Box<dyn Widget>;
    let color = agg_gui::Color::rgba(0.5, 0.5, 0.5, 1.0);
    let memory = Rc::clone(memory) as Rc<dyn BrowserMemory>;
    BrowserSplitter::new(panel(), panel(), color, memory, "Library Splitter")
}

#[test]
fn the_splitter_drags_the_trees_width_and_remembers_it() {
    agg_gui::set_device_scale(1.0);
    let memory = Rc::new(Remembering::default());
    let mut app = App::new(Box::new(splitter(&memory)));
    app.layout(Size::new(WIDTH, HEIGHT));
    assert_eq!(property(&app, "Library Splitter", "distance"), "254");

    let bar = Point::new(254.0 + SPLITTER_WIDTH / 2.0, HEIGHT / 2.0);
    app.on_mouse_move(bar.x, bar.y);
    app.on_mouse_down(bar.x, bar.y, MouseButton::Left, Modifiers::default());
    app.on_mouse_move(bar.x - 100.0, bar.y);
    app.layout(Size::new(WIDTH, HEIGHT));
    assert_eq!(property(&app, "Library Splitter", "distance"), "154");
    assert_eq!(
        *memory.width.borrow(),
        None,
        "remembered when the drag ends"
    );
    app.on_mouse_up(
        bar.x - 100.0,
        bar.y,
        MouseButton::Left,
        Modifiers::default(),
    );
    assert_eq!(*memory.width.borrow(), Some(154.0));

    // Never narrower than the minimum.
    app.on_mouse_down(157.0, bar.y, MouseButton::Left, Modifiers::default());
    app.on_mouse_move(-500.0, bar.y);
    app.on_mouse_up(-500.0, bar.y, MouseButton::Left, Modifiers::default());
    assert_eq!(*memory.width.borrow(), Some(20.0));

    // A new splitter starts where the user left it.
    let app = App::new(Box::new(splitter(&memory)));
    assert_eq!(property(&app, "Library Splitter", "distance"), "20");
}
