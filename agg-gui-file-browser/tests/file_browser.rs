//! The composed `FileBrowser` over a `MemoryFolder`, through agg-gui's `App` with real pointer
//! and key events: the tree form lays out its bar, tree, splitter and list under the names
//! MatterCAD's Library uses, in its order; a tree row moves the list and the breadcrumbs; the
//! search box keeps its text and the keys when a search's results rebuild the header; the drag's
//! picture is drawn by the browser, from the global overlay, while a row is dragged from the list
//! onto the tree; the list-only form has its toolbar and no tree, and its rows open; and an app's
//! dressed built-in keeps its tooltip in the menu.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{App, Framebuffer, GfxCtx, Key, Modifiers, MouseButton, Point, Rect, Size, Widget};
use agg_gui_file_browser::{
    run_ready_local_tasks, BrowserAction, BrowserActions, BrowserCommand, BrowserConfig,
    BrowserLayout, BrowserOptions, BrowserRef, BrowserState, BuiltinAction, Builtins, FileBrowser,
    ViewMode,
};
use agg_gui_files::{tr, DynamicFolder, MemoryFolder};

const WIDTH: f64 = 700.0;
const HEIGHT: f64 = 400.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// Home holding Projects (holding Brackets), gear.stl and bolt.stl.
fn home() -> MemoryFolder {
    let home = MemoryFolder::new("Home");
    let projects = home.add_folder("Projects").expect("free");
    projects.add_folder("Brackets").expect("free");
    home.add_file("gear.stl", b"solid gear".to_vec());
    home.add_file("bolt.stl", b"solid bolt".to_vec());
    home
}

struct Browser {
    app: App,
    state: Rc<RefCell<BrowserState>>,
}

/// The browser at Home with `config`, in rows, laid out and painted once.
fn browser(config: BrowserConfig) -> Browser {
    agg_gui::set_device_scale(1.0);
    let state = BrowserState::new(Box::new(home()), config);
    state.borrow_mut().set_view_mode(ViewMode::Rows);
    let app = App::new(Box::new(FileBrowser::new(&state, font())));
    let mut b = Browser { app, state };
    frame(&mut b.app);
    b
}

fn list_only() -> BrowserConfig {
    BrowserConfig::builder()
        .options(BrowserOptions {
            layout: BrowserLayout::ListOnly,
            ..BrowserOptions::default()
        })
        .build()
}

/// A layout and a paint, as a frame does (the global overlays included).
fn frame(app: &mut App) {
    app.layout(Size::new(WIDTH, HEIGHT));
    let mut fb = Framebuffer::new(WIDTH as u32, HEIGHT as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
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

fn rect(app: &App, id: &str) -> Rect {
    find(app.root(), id, Point::new(0.0, 0.0))
        .unwrap_or_else(|| panic!("no widget named {id}"))
        .1
}

fn has(app: &App, id: &str) -> bool {
    find(app.root(), id, Point::new(0.0, 0.0)).is_some()
}

fn centre(app: &App, id: &str) -> Point {
    let r = rect(app, id);
    Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0)
}

/// The browser's own property `key`.
fn browser_property(app: &App, key: &str) -> String {
    app.root()
        .properties()
        .into_iter()
        .find(|(k, _)| *k == key)
        .unwrap_or_else(|| panic!("the browser has no {key}"))
        .1
}

fn click(app: &mut App, id: &str) {
    let at = centre(app, id);
    let y = HEIGHT - at.y;
    app.on_mouse_move(at.x, y);
    app.on_mouse_down(at.x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(at.x, y, MouseButton::Left, Modifiers::default());
    frame(app);
}

fn move_to(app: &mut App, at: Point) {
    app.on_mouse_move(at.x, HEIGHT - at.y);
    frame(app);
}

#[test]
fn the_tree_form_lays_out_its_bar_tree_and_list_under_the_librarys_names() {
    let b = browser(BrowserConfig::default());
    let bar = [
        "FolderBreadCrumbWidget",
        "Library Search Panel",
        "Library Undo Button",
        "Library Redo Button",
        "Library View Options Button",
        "Print Library View Options",
        "Print Library Overflow Menu",
    ];
    let rects: Vec<Rect> = bar.iter().map(|id| rect(&b.app, id)).collect();
    for (pair, ids) in rects.windows(2).zip(bar.windows(2)) {
        assert!(
            pair[0].x + pair[0].width <= pair[1].x + 0.5,
            "{} sits left of {}",
            ids[0],
            ids[1]
        );
    }
    // The search box is 200 wide inside its margins; the overflow button ends at the bar's
    // right padding.
    assert_eq!(rects[1].width, 215.0);
    let overflow = rects[6];
    assert_eq!(overflow.x + overflow.width, WIDTH - 3.0);
    assert!(has(&b.app, "Library Up Button"));
    assert!(has(&b.app, "Bread Crumb Button Home"));
    assert!(!has(&b.app, "Folders Toolbar"), "the tree form has one bar");
    // Under the bar: the tree, the splitter's bar, the list.
    let (tree, list) = (
        rect(&b.app, "Library TreeView"),
        rect(&b.app, "LibraryView"),
    );
    let splitter = rect(&b.app, "Library Splitter");
    assert_eq!(splitter.width, WIDTH);
    assert!(tree.x + tree.width < list.x);
    assert!(
        tree.y + tree.height <= rects[0].y,
        "the tree is under the bar"
    );
    assert!(has(&b.app, "Projects Tree Node"));
    assert!(has(&b.app, "Row Item gear.stl"));
    assert!(has(&b.app, "Projects Row Item Collection"));
    assert_eq!(browser_property(&b.app, "item_count"), "2");
}

#[test]
fn a_tree_row_moves_the_list_and_the_breadcrumbs() {
    let mut b = browser(BrowserConfig::default());
    click(&mut b.app, "Projects Tree Node");
    assert_eq!(browser_property(&b.app, "path"), "Home > Projects");
    assert_eq!(
        browser_property(&b.app, "bread_crumb_trail"),
        "Home/Projects"
    );
    assert!(has(&b.app, "Bread Crumb Button Projects"));
    assert!(has(&b.app, "Brackets Row Item Collection"));
    assert!(!has(&b.app, "Row Item gear.stl"));
    // The breadcrumb takes it back, and the mouse's back button goes up.
    click(&mut b.app, "Bread Crumb Button Home");
    assert_eq!(browser_property(&b.app, "path"), "Home");
    assert!(!has(&b.app, "Bread Crumb Button Projects"));
    click(&mut b.app, "Projects Row Item Collection");
    let at = centre(&b.app, "LibraryView");
    b.app.on_mouse_down(
        at.x,
        HEIGHT - at.y,
        MouseButton::Other(3),
        Modifiers::default(),
    );
    frame(&mut b.app);
    assert_eq!(browser_property(&b.app, "path"), "Home");
}

#[test]
fn the_search_box_keeps_its_text_and_the_keys_when_the_results_rebuild_the_header() {
    let mut b = browser(BrowserConfig::default());
    click(&mut b.app, "Library Search Input");
    for c in "gea".chars() {
        b.app.on_text_input(&c.to_string());
    }
    b.app.on_key_down(Key::Enter, Modifiers::default());
    for _ in 0..1000 {
        run_ready_local_tasks();
    }
    frame(&mut b.app);
    assert_eq!(b.state.borrow().depth(), 2, "the results show below Home");
    assert_eq!(browser_property(&b.app, "search_query"), "gea");
    assert!(has(&b.app, "Row Item gear.stl"));
    assert!(!has(&b.app, "Row Item bolt.stl"));
    // The header was built again for the results' folder, with the same box in it, still
    // taking the keys.
    assert_eq!(
        browser_property(&b.app, "bread_crumb_trail")
            .split('/')
            .count(),
        2
    );
    b.app.on_text_input("r");
    frame(&mut b.app);
    let search = find(b.app.root(), "Library Search Panel", Point::new(0.0, 0.0))
        .expect("the box")
        .0;
    let text = search
        .as_any()
        .and_then(|any| any.downcast_ref::<agg_gui_file_browser::search::search_box::SearchBox>())
        .map(|s| s.text());
    assert_eq!(text.as_deref(), Some("gear"));
}

#[test]
fn the_browser_draws_the_drag_picture_while_a_row_goes_from_the_list_to_the_tree() {
    let mut b = browser(BrowserConfig::default());
    let (start, end) = (
        centre(&b.app, "Row Item gear.stl"),
        centre(&b.app, "Projects Tree Node"),
    );
    b.app.on_mouse_move(start.x, HEIGHT - start.y);
    b.app.on_mouse_down(
        start.x,
        HEIGHT - start.y,
        MouseButton::Left,
        Modifiers::default(),
    );
    frame(&mut b.app);
    for step in 1..=4 {
        let t = f64::from(step) / 4.0;
        let at = Point::new(
            start.x + (end.x - start.x) * t,
            start.y + (end.y - start.y) * t,
        );
        move_to(&mut b.app, at);
    }
    let at = Point::new(end.x + 1.0, end.y);
    move_to(&mut b.app, at);
    assert_eq!(browser_property(&b.app, "dragging"), "true");
    // Painted by the browser's overlay (the list in a composed browser leaves it), over the tree.
    let expected = format!("{},{},64,64", at.x + 16.0, at.y - 16.0 - 64.0);
    assert_eq!(browser_property(&b.app, "drag_picture"), expected);
    assert_eq!(browser_property(&b.app, "drag_picture_count"), "1");
    b.app
        .on_mouse_up(at.x, HEIGHT - at.y, MouseButton::Left, Modifiers::default());
    for _ in 0..20 {
        run_ready_local_tasks();
    }
    frame(&mut b.app);
    assert_eq!(browser_property(&b.app, "drag_picture"), "");
    assert!(!has(&b.app, "Row Item gear.stl"), "moved into Projects");
    click(&mut b.app, "Projects Tree Node");
    assert!(has(&b.app, "Row Item gear.stl"));
}

#[test]
fn the_list_only_form_has_its_toolbar_and_no_tree_and_its_rows_open() {
    let mut b = browser(list_only());
    assert!(!has(&b.app, "Library TreeView"));
    assert!(!has(&b.app, "Library Splitter"));
    assert!(!has(&b.app, "Library Undo Button"));
    let (bar, toolbar) = (
        rect(&b.app, "FolderBreadCrumbWidget"),
        rect(&b.app, "Folders Toolbar"),
    );
    assert!(
        toolbar.y + toolbar.height <= bar.y + 0.5,
        "the toolbar is under the bar"
    );
    let search = rect(&b.app, "Library Search Panel");
    assert!(
        search.width > WIDTH / 2.0,
        "the search box takes the toolbar's room"
    );
    for id in ["Library View Options Button", "Print Library View Options"] {
        assert!(rect(&b.app, id).x > search.x + search.width);
    }
    assert!(has(&b.app, "Print Library Overflow Menu"));
    let list = rect(&b.app, "LibraryView");
    assert_eq!(list.x, 0.0);
    assert!(list.y + list.height <= toolbar.y + 0.5);
    // A double click on a folder row opens it.
    let at = centre(&b.app, "Projects Row Item Collection");
    let y = HEIGHT - at.y;
    b.app.on_mouse_move(at.x, y);
    for clicks in 1..=2 {
        b.app
            .on_mouse_down_clicks(at.x, y, MouseButton::Left, Modifiers::default(), clicks);
        b.app
            .on_mouse_up(at.x, y, MouseButton::Left, Modifiers::default());
        frame(&mut b.app);
    }
    frame(&mut b.app);
    assert_eq!(browser_property(&b.app, "path"), "Home > Projects");
    assert!(has(&b.app, "Brackets Row Item Collection"));
}

/// An app placing the built-ins with Add dressed in its own tooltip.
struct DressedAdd;

impl BrowserActions for DressedAdd {
    fn menu(&self, _state: &BrowserRef, builtins: &Builtins) -> Vec<BrowserAction> {
        let mut rows = builtins.default_menu();
        for row in &mut rows {
            if matches!(row, BrowserAction::Builtin(BuiltinAction::Add)) {
                let add = BrowserCommand::builtin(BuiltinAction::Add)
                    .with_tooltip("Add an .stl file to the folder");
                *row = BrowserAction::Command(add);
            }
        }
        rows
    }
}

#[test]
fn a_built_in_the_app_dresses_keeps_its_tooltip_and_still_runs_as_the_browsers() {
    let config = BrowserConfig::builder()
        .actions(Rc::new(DressedAdd))
        .build();
    let mut b = browser(config);
    click(&mut b.app, "Print Library Overflow Menu");
    let overflow = find(
        b.app.root(),
        "Print Library Overflow Menu",
        Point::new(0.0, 0.0),
    )
    .expect("the button")
    .0
    .as_any()
    .and_then(|a| a.downcast_ref::<agg_gui_file_browser::OverflowButton>())
    .expect("an overflow button");
    let rows = overflow.menu_rows();
    assert_eq!(rows.first().map(|r| r.0.as_str()), Some(tr("Add").as_str()));
    let add = overflow
        .menu_entries()
        .iter()
        .find_map(|e| match e {
            agg_gui::widgets::menu::MenuEntry::Item(item) if item.label == tr("Add") => {
                Some(item.clone())
            }
            _ => None,
        })
        .expect("Add's row");
    assert_eq!(
        add.tooltip.as_deref(),
        Some("Add an .stl file to the folder")
    );
    // The browser's own Add: enabled by its rules (no dialogs to pick files with, so greyed).
    assert!(!add.enabled);
}

#[test]
fn a_folders_header_markdown_shows_above_the_list_and_goes_with_the_folder() {
    for config in [BrowserConfig::default(), list_only()] {
        agg_gui::set_device_scale(1.0);
        let root = DynamicFolder::new("Home").with_header_markdown("Parts **you** made");
        let state = BrowserState::new(Box::new(root), config);
        let mut app = App::new(Box::new(FileBrowser::new(&state, font())));
        frame(&mut app);
        let (header, list) = (
            rect(&app, "Library Header Markdown"),
            rect(&app, "LibraryView"),
        );
        assert!(header.height > 0.0);
        assert!(
            list.y + list.height <= header.y + 0.5,
            "the list is under it"
        );
        let list_only = !has(&app, "Library TreeView");
        // The list-only form also says it along its bottom.
        assert_eq!(has(&app, "Provider Message"), list_only);
        state
            .borrow_mut()
            .push_container(Box::new(MemoryFolder::new("Plain")));
        frame(&mut app);
        assert!(!has(&app, "Library Header Markdown"));
        assert!(!has(&app, "Provider Message"));
    }
}
