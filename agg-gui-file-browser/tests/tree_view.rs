//! The folder tree (`BrowserTree`) over a `MemoryFolder`, driven through agg-gui's `App` with real
//! pointer events: the arrow opens and folds a folder, a press on a row's name shows the folder in
//! the browser and opens it, the browser's folder is drawn selected with its parents opened, a
//! folder added to the showing folder or renamed by a write shows in the tree, the quick look
//! leaves an empty folder without an arrow, two folders of one name in different places stay two
//! rows, and a right press opens the tree's menu for the folder, asked of the app with that
//! folder as the selection in the folder above it.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{App, Modifiers, MouseButton, Point, Rect, Size, Widget};
use agg_gui_file_browser::{
    BrowserAction, BrowserActions, BrowserCommand, BrowserConfig, BrowserRef, BrowserState,
    BrowserTree, Builtins, SheetDialogs,
};
use agg_gui_files::{Folder, MemoryFolder};

const WIDTH: f64 = 300.0;
const HEIGHT: f64 = 300.0;
/// Where a root row's arrow is, from the row's left: the tree's margin and half the arrow's box.
const ARROW_X: f64 = 15.0 + 11.0;
/// Where a root row's name is, from the row's left: past the arrow and the icon.
const NAME_X: f64 = 15.0 + 22.0 + 20.0 + 10.0;
/// The indent of each level.
const INDENT: f64 = 16.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// Home holding Projects (holding Brackets) and the empty Notes; and the Projects handle.
fn home() -> (MemoryFolder, MemoryFolder) {
    let home = MemoryFolder::new("Home");
    let projects = home.add_folder("Projects").expect("Projects is free");
    projects.add_folder("Brackets").expect("Brackets is free");
    home.add_folder("Notes").expect("Notes is free");
    (home, projects)
}

/// An app showing the tree of a browser at `root` with `config`, laid out.
fn tree_app(root: Box<dyn Folder>, config: BrowserConfig) -> (App, Rc<RefCell<BrowserState>>) {
    agg_gui::set_device_scale(1.0);
    let state = BrowserState::new(root, config);
    let mut app = App::new(Box::new(BrowserTree::for_state(&state, font())));
    app.layout(Size::new(WIDTH, HEIGHT));
    (app, state)
}

fn tree(app: &App) -> &BrowserTree {
    app.root()
        .as_any()
        .and_then(|any| any.downcast_ref::<BrowserTree>())
        .expect("the root is the tree")
}

/// The tree's rows, indented by depth.
fn rows(app: &App) -> String {
    property_of(app.root(), "rows")
}

fn property_of(widget: &dyn Widget, key: &str) -> String {
    widget
        .properties()
        .into_iter()
        .find(|(k, _)| *k == key)
        .unwrap_or_else(|| panic!("no {key}"))
        .1
}

/// The `n`th row widget (root coordinates, Y up, as the tree is the root).
fn nth_row(app: &App, n: usize) -> (&dyn Widget, Rect) {
    let row = app.root().children()[n].as_ref();
    (row, row.bounds())
}

/// The first row named `id`, and its index.
fn row_named(app: &App, id: &str) -> usize {
    app.root()
        .children()
        .iter()
        .position(|row| row.id() == Some(id))
        .unwrap_or_else(|| panic!("no row {id}: {}", rows(app)))
}

fn row_property(app: &App, n: usize, key: &str) -> String {
    property_of(nth_row(app, n).0, key)
}

/// A press and release at `x` from row `n`'s left, at its middle, then a layout.
fn press_row(app: &mut App, n: usize, x: f64, button: MouseButton) {
    let depth = rows(app)
        .split('|')
        .nth(n)
        .map_or(0, |r| (r.len() - r.trim_start().len()) / 2);
    let rect = nth_row(app, n).1;
    let at = Point::new(
        rect.x + x + depth as f64 * INDENT,
        rect.y + rect.height / 2.0,
    );
    let y = HEIGHT - at.y;
    app.on_mouse_move(at.x, y);
    app.on_mouse_down_clicks(at.x, y, button, Modifiers::default(), 1);
    app.on_mouse_up(at.x, y, button, Modifiers::default());
    app.layout(Size::new(WIDTH, HEIGHT));
}

#[test]
fn the_arrow_opens_and_folds_a_folder() {
    let (home, _) = home();
    let (mut app, state) = tree_app(Box::new(home), BrowserConfig::default());
    assert_eq!(rows(&app), "Notes|Projects");
    let projects = row_named(&app, "Projects Tree Node");
    press_row(&mut app, projects, ARROW_X, MouseButton::Left);
    assert_eq!(rows(&app), "Notes|Projects|  Brackets");
    assert_eq!(row_property(&app, projects, "expanded"), "true");
    // The arrow only opens: the browser stays where it was.
    assert_eq!(state.borrow().path_names(), ["Home"]);
    press_row(&mut app, projects, ARROW_X, MouseButton::Left);
    assert_eq!(rows(&app), "Notes|Projects");
    assert_eq!(row_property(&app, projects, "expanded"), "false");
}

#[test]
fn a_press_on_a_rows_name_shows_the_folder_in_the_browser_and_opens_it() {
    let (home, _) = home();
    let (mut app, state) = tree_app(Box::new(home), BrowserConfig::default());
    let projects = row_named(&app, "Projects Tree Node");
    press_row(&mut app, projects, NAME_X, MouseButton::Left);
    assert_eq!(state.borrow().path_names(), ["Home", "Projects"]);
    assert_eq!(rows(&app), "Notes|Projects|  Brackets");
    assert_eq!(row_property(&app, projects, "selected"), "true");

    // A folder one level in, from the tree: the path is the tree's.
    let brackets = row_named(&app, "Brackets Tree Node");
    press_row(&mut app, brackets, NAME_X, MouseButton::Left);
    assert_eq!(
        state.borrow().path_names(),
        ["Home", "Projects", "Brackets"]
    );
    // An open row stays open when pressed again: only the arrow folds.
    press_row(&mut app, projects, NAME_X, MouseButton::Left);
    assert_eq!(state.borrow().path_names(), ["Home", "Projects"]);
    assert_eq!(rows(&app), "Notes|Projects|  Brackets");
}

#[test]
fn the_browsers_folder_is_drawn_selected_with_its_parents_opened() {
    let (home, _) = home();
    let (mut app, state) = tree_app(Box::new(home), BrowserConfig::default());
    assert_eq!(rows(&app), "Notes|Projects");
    // The browser moves on its own (the list, a breadcrumb).
    let projects = state
        .borrow()
        .root_child_containers()
        .into_iter()
        .find(|link| link.name() == "Projects")
        .expect("Projects is in Home");
    state.borrow_mut().open(projects.as_ref());
    let brackets = state
        .borrow()
        .active_container()
        .expect("Projects shows")
        .child_containers()[0]
        .clone();
    state.borrow_mut().open(brackets.as_ref());
    app.layout(Size::new(WIDTH, HEIGHT));
    assert_eq!(rows(&app), "Notes|Projects|  Brackets");
    let brackets_row = row_named(&app, "Brackets Tree Node");
    assert_eq!(row_property(&app, brackets_row, "selected"), "true");
    let projects_row = row_named(&app, "Projects Tree Node");
    assert_eq!(row_property(&app, projects_row, "selected"), "false");
    // Back at the root, nothing is selected.
    state.borrow_mut().go_to(0);
    app.layout(Size::new(WIDTH, HEIGHT));
    for n in 0..app.root().children().len() {
        assert_eq!(row_property(&app, n, "selected"), "false");
    }
}

#[test]
fn a_folder_added_to_the_showing_folder_or_renamed_by_a_write_shows_in_the_tree() {
    let (home, projects) = home();
    let (mut app, state) = tree_app(Box::new(home), BrowserConfig::default());
    let row = row_named(&app, "Projects Tree Node");
    press_row(&mut app, row, NAME_X, MouseButton::Left);
    assert_eq!(rows(&app), "Notes|Projects|  Brackets");

    // Added from outside the browser: the showing folder's content changes.
    projects.add_folder("Fixtures").expect("Fixtures is free");
    app.layout(Size::new(WIDTH, HEIGHT));
    assert_eq!(rows(&app), "Notes|Projects|  Brackets|  Fixtures");

    // Renamed by a write through the browser: the tree lists itself again.
    {
        let mut s = state.borrow_mut();
        let item = s
            .active_container()
            .expect("Projects shows")
            .child_containers()[0]
            .clone();
        let item: Rc<dyn agg_gui_files::Item> = item;
        let mut folder = s.container_at_mut(1).expect("Projects is open");
        let writable = folder
            .as_writable_mut()
            .expect("a memory folder is writable");
        writable.rename(&item, "Clamps").expect("Clamps is free");
        drop(folder);
        s.after_write(String::new());
    }
    app.layout(Size::new(WIDTH, HEIGHT));
    assert_eq!(rows(&app), "Notes|Projects|  Clamps|  Fixtures");
}

#[test]
fn the_quick_look_leaves_an_empty_folder_without_an_arrow() {
    let (home, _) = home();
    let (mut app, state) = tree_app(Box::new(home), BrowserConfig::default());
    let notes = row_named(&app, "Notes Tree Node");
    assert_eq!(row_property(&app, notes, "has_arrow"), "false");
    let projects = row_named(&app, "Projects Tree Node");
    assert_eq!(row_property(&app, projects, "has_arrow"), "true");
    // Where the arrow would be, a press shows the folder, as one anywhere else on the row does.
    press_row(&mut app, notes, ARROW_X, MouseButton::Left);
    assert_eq!(state.borrow().path_names(), ["Home", "Notes"]);
    assert_eq!(rows(&app), "Notes|Projects");
}

#[test]
fn two_folders_of_one_name_in_different_places_stay_two_rows() {
    let home = MemoryFolder::new("Home");
    for parent in ["A", "B"] {
        let parent = home.add_folder(parent).expect("free");
        parent
            .add_folder("Twin")
            .expect("free")
            .add_folder(parent_name(&parent))
            .expect("free");
    }
    let (mut app, state) = tree_app(Box::new(home), BrowserConfig::default());
    press_row(&mut app, 0, ARROW_X, MouseButton::Left);
    let b = row_named(&app, "B Tree Node");
    press_row(&mut app, b, ARROW_X, MouseButton::Left);
    assert_eq!(rows(&app), "A|  Twin|B|  Twin");

    // B's twin, from the tree: it, not A's, is drawn selected and opens to its own folder.
    press_row(&mut app, 3, NAME_X, MouseButton::Left);
    assert_eq!(state.borrow().path_names(), ["Home", "B", "Twin"]);
    assert_eq!(rows(&app), "A|  Twin|B|  Twin|    In B");
    assert_eq!(row_property(&app, 3, "selected"), "true");
    assert_eq!(row_property(&app, 1, "selected"), "false");
    assert_eq!(row_property(&app, 1, "expanded"), "false");
}

/// The folder each twin holds: "In A" or "In B", after its parent.
fn parent_name(parent: &MemoryFolder) -> &'static str {
    match parent.name() {
        "A" => "In A",
        _ => "In B",
    }
}

/// Actions whose menu offers the built-ins and one command, recording what each ask was handed
/// and what the browser's selection was when the command ran.
#[derive(Default)]
struct Menus {
    asked: RefCell<Vec<(Vec<String>, Vec<String>)>>,
    ran_with: Rc<RefCell<Vec<String>>>,
}

impl BrowserActions for Menus {
    fn menu(&self, state: &BrowserRef, builtins: &Builtins) -> Vec<BrowserAction> {
        let selection = state
            .selection()
            .iter()
            .map(|e| e.name().to_string())
            .collect();
        self.asked
            .borrow_mut()
            .push((state.folder_path().to_vec(), selection));
        let live = state.live().expect("the browser is open");
        let ran_with = Rc::clone(&self.ran_with);
        let mut rows = builtins.default_menu();
        rows.push(BrowserAction::Command(BrowserCommand::new(
            "Show Info",
            move || {
                let names = live
                    .borrow()
                    .selected_entries()
                    .iter()
                    .map(|e| e.name().to_string())
                    .collect();
                *ran_with.borrow_mut() = names;
            },
        )));
        rows
    }
}

#[test]
fn a_right_press_opens_the_folders_menu_asked_with_it_as_the_selection() {
    let (home, _) = home();
    let actions = Rc::new(Menus::default());
    let config = BrowserConfig::builder()
        .actions(actions.clone())
        .dialogs(Rc::new(SheetDialogs::new()))
        .build();
    let (mut app, state) = tree_app(Box::new(home), config);
    let projects = row_named(&app, "Projects Tree Node");
    press_row(&mut app, projects, ARROW_X, MouseButton::Left);
    let brackets = row_named(&app, "Brackets Tree Node");
    press_row(&mut app, brackets, NAME_X, MouseButton::Right);

    // The folder shows, and its menu is open where it was pressed.
    assert_eq!(
        state.borrow().path_names(),
        ["Home", "Projects", "Brackets"]
    );
    assert!(tree(&app).menu_is_open());
    let menu = tree(&app).menu_rows();
    let titles: Vec<&str> = menu.iter().map(|(t, _)| t.as_str()).collect();
    assert!(titles.contains(&"Show Info"), "{titles:?}");
    assert!(titles.contains(&"Remove"), "{titles:?}");
    assert!(!titles.contains(&"Create Folder..."), "{titles:?}");
    // The menu holds what can run: Rename and Remove (the dialogs confirm and ask names), no
    // Move (no Move dialog) and no Open File Location (nothing locates).
    assert_eq!(titles, ["Rename", "Remove", "Show Info"]);
    assert!(menu
        .iter()
        .filter(|(t, _)| !t.is_empty())
        .all(|(_, enabled)| *enabled));
    // Asked for in the folder above, with the folder as the selection.
    assert_eq!(
        *actions.asked.borrow(),
        vec![(
            vec!["Home".to_string(), "Projects".to_string()],
            vec!["Brackets".to_string()]
        )]
    );
    assert!(state.borrow().temporary_selection().is_none());
}

#[test]
fn the_menus_command_runs_with_the_folder_as_the_selection() {
    let (home, _) = home();
    let actions = Rc::new(Menus::default());
    let config = BrowserConfig::builder().actions(actions.clone()).build();
    let (mut app, state) = tree_app(Box::new(home), config);
    let notes = row_named(&app, "Notes Tree Node");
    press_row(&mut app, notes, NAME_X, MouseButton::Right);
    let menu = tree(&app).menu_rows();
    let at = menu
        .iter()
        .position(|(t, _)| t == "Show Info")
        .expect("Show Info is a row");
    // The menu opened at the press; its rows hang below it. Find the row's place by walking
    // down from the press until a release there runs the command.
    let rect = nth_row(&app, notes).1;
    let press = Point::new(rect.x + NAME_X + 10.0, rect.y + rect.height / 2.0);
    let mut ran = false;
    for step in 0..((at + 2) * 40) {
        let y = HEIGHT - press.y + 4.0 + step as f64;
        app.on_mouse_move(press.x, y);
        app.on_mouse_down(press.x, y, MouseButton::Left, Modifiers::default());
        app.on_mouse_up(press.x, y, MouseButton::Left, Modifiers::default());
        app.layout(Size::new(WIDTH, HEIGHT));
        if !actions.ran_with.borrow().is_empty() {
            ran = true;
            break;
        }
        if !tree(&app).menu_is_open() {
            press_row(&mut app, notes, NAME_X, MouseButton::Right);
        }
    }
    assert!(ran, "the command ran");
    assert_eq!(*actions.ran_with.borrow(), ["Notes"]);
    assert!(state.borrow().temporary_selection().is_none());
}
