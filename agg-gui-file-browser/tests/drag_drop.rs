//! Dragging the browser's items onto its folders, through agg-gui's `App` with real pointer
//! events over a folder tree beside the list: a row dropped on a folder row, a folder tile or a
//! tree row moves on disk and the move can be undone; with the copy modifier the drop copies; a
//! drop between two places copies; a read-only folder is never highlighted and takes nothing; and
//! a copy is offered to the app first (`take_over_copy`), which may run it or hand it back. A
//! drag held over a folder opens it after the spring delay (on agg-gui's virtual clock): a tree
//! row in place, a folder in the list in the list, where a drop on the empty space lands in it.
//! The drag's picture follows the pointer, hides over the app's view when the app says so, and a
//! release there goes to the app's `OutsideDrops` hook (`cancel` when the drag comes back).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{
    App, DrawCtx, Event, EventResult, Framebuffer, GfxCtx, Modifiers, MouseButton, Point, Rect,
    Size, Widget,
};
use agg_gui_file_browser::{
    run_ready_local_tasks, BrowserConfig, BrowserList, BrowserState, BrowserTree, CopyCx,
    CopyTakeover, OutsideDrops, OutsideOver, PlaceRules, ViewMode, SPRING_DELAY,
};
use agg_gui_files::disk::{AllFiles, DiskFolder};
use agg_gui_files::Item;
use agg_gui_files::{DynamicFolder, DynamicFolderLink, Folder, FolderLink, MemoryFolder, PlaceId};

const WIDTH: f64 = 600.0;
const HEIGHT: f64 = 400.0;
/// The tree's width, left of the list.
const TREE: f64 = 180.0;
/// The room right of the list: the app's own view, outside the browser.
const VIEW: f64 = 100.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// The tree left of the list, as an app places them.
struct Split {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Split {
    fn type_name(&self) -> &'static str {
        "Split"
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
        let widths = [TREE, available.width - TREE - VIEW];
        let mut x = 0.0;
        for (child, width) in self.children.iter_mut().zip(widths) {
            child.layout(Size::new(width, available.height));
            child.set_bounds(Rect::new(x, 0.0, width, available.height));
            x += width;
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// An app showing the tree and the list of a browser at `root` with `config`, in `mode`.
fn browser_app(
    root: Box<dyn Folder>,
    config: BrowserConfig,
    mode: ViewMode,
) -> (App, Rc<RefCell<BrowserState>>) {
    agg_gui::set_device_scale(1.0);
    let state = BrowserState::new(root, config);
    state.borrow_mut().set_view_mode(mode);
    let split = Split {
        bounds: Rect::default(),
        children: vec![
            Box::new(BrowserTree::for_state(&state, font())),
            Box::new(BrowserList::new(&state, font())),
        ],
    };
    let mut app = App::new(Box::new(split));
    frame(&mut app);
    (app, state)
}

/// A layout and a paint, as a frame does: the folders record themselves as drop targets.
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

fn found<'a>(app: &'a App, id: &str) -> (&'a dyn Widget, Rect) {
    find(app.root(), id, Point::new(0.0, 0.0)).unwrap_or_else(|| panic!("no widget named {id}"))
}

fn centre(app: &App, id: &str) -> Point {
    let r = found(app, id).1;
    Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0)
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

/// The modifiers holding the copy modifier: Option on the Mac, Ctrl elsewhere.
fn copy_modifier() -> Modifiers {
    Modifiers {
        alt: cfg!(target_os = "macos"),
        ctrl: !cfg!(target_os = "macos"),
        ..Modifiers::default()
    }
}

/// Moves the pointer to `at` (root coordinates, Y up), then a frame.
fn move_to(app: &mut App, at: Point) {
    app.on_mouse_move(at.x, HEIGHT - at.y);
    frame(app);
}

/// Presses on the widget named `from`, drags in steps to the widget named `to`, and holds there
/// (a second move once the targets have painted). The button stays down.
fn drag_over(app: &mut App, from: &str, to: &str) -> Point {
    let (start, end) = (centre(app, from), centre(app, to));
    app.on_mouse_move(start.x, HEIGHT - start.y);
    app.on_mouse_down(
        start.x,
        HEIGHT - start.y,
        MouseButton::Left,
        Modifiers::default(),
    );
    frame(app);
    for step in 1..=4 {
        let t = f64::from(step) / 4.0;
        let at = Point::new(
            start.x + (end.x - start.x) * t,
            start.y + (end.y - start.y) * t,
        );
        move_to(app, at);
    }
    move_to(app, Point::new(end.x + 1.0, end.y));
    Point::new(end.x + 1.0, end.y)
}

/// Releases at `at` with `modifiers`, then runs the copies started and a frame.
fn release(app: &mut App, at: Point, modifiers: Modifiers) {
    app.on_mouse_up(at.x, HEIGHT - at.y, MouseButton::Left, modifiers);
    for _ in 0..20 {
        run_ready_local_tasks();
    }
    frame(app);
}

/// A fresh scratch folder holding the folder Parts and the file gear.stl.
fn workshop(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "agg-gui-file-browser-drag-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Parts")).expect("made");
    std::fs::write(dir.join("gear.stl"), b"solid gear").expect("written");
    dir
}

fn disk(dir: &Path) -> Box<dyn Folder> {
    Box::new(DiskFolder::new(dir, Rc::new(AllFiles)))
}

/// Drags gear.stl's row onto `target` in `mode` and checks the move on disk and its undo.
fn moves_onto(label: &str, mode: ViewMode, target: &str) {
    let dir = workshop(label);
    let (mut app, state) = browser_app(disk(&dir), BrowserConfig::default(), mode);
    let at = drag_over(&mut app, "Row Item gear.stl", target);
    assert_eq!(property(&app, target, "drop_label"), "Move to Parts");
    assert_eq!(property(&app, "Row Item gear.stl", "lifted"), "true");
    release(&mut app, at, Modifiers::default());
    assert!(dir.join("Parts/gear.stl").exists() && !dir.join("gear.stl").exists());
    // The list shows the folder without it.
    assert!(find(app.root(), "Row Item gear.stl", Point::new(0.0, 0.0)).is_none());
    let history = state.borrow().history();
    assert_eq!(
        history.undo_description().as_deref(),
        Some("Moved gear.stl to Parts")
    );
    assert!(BrowserState::undo(&state));
    assert!(dir.join("gear.stl").exists() && !dir.join("Parts/gear.stl").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_row_dropped_on_a_folder_row_moves_and_can_be_undone() {
    moves_onto("row", ViewMode::Rows, "Parts Row Item Collection");
}

#[test]
fn a_row_dropped_on_a_folder_tile_moves_and_can_be_undone() {
    moves_onto("tile", ViewMode::Icons, "Parts Row Item Collection");
}

#[test]
fn a_row_dropped_on_a_tree_row_moves_and_can_be_undone() {
    moves_onto("tree", ViewMode::Rows, "Parts Tree Node");
}

#[test]
fn the_copy_modifier_copies() {
    let dir = workshop("copy");
    let (mut app, state) = browser_app(disk(&dir), BrowserConfig::default(), ViewMode::Rows);
    let at = drag_over(&mut app, "Row Item gear.stl", "Parts Row Item Collection");
    release(&mut app, at, copy_modifier());
    assert!(dir.join("Parts/gear.stl").exists() && dir.join("gear.stl").exists());
    let history = state.borrow().history();
    assert!(history.can_undo(), "the copy is one step");
    assert!(BrowserState::undo(&state));
    assert!(!dir.join("Parts/gear.stl").exists() && dir.join("gear.stl").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Home listing Desk (in place "desk", holding gear.stl) and Shelf (in place "shelf"), each a
/// root row, and the browser showing Desk.
fn two_places(
    config: BrowserConfig,
    shelf_read_only: bool,
) -> (App, Rc<RefCell<BrowserState>>, MemoryFolder, MemoryFolder) {
    let desk = MemoryFolder::new("Desk").with_place(PlaceId::new("desk"));
    desk.add_file("gear.stl", b"solid".to_vec());
    let shelf = MemoryFolder::new("Shelf").with_place(PlaceId::new("shelf"));
    let shelf_link: Rc<dyn FolderLink> = match shelf_read_only {
        false => shelf.link(),
        true => {
            let handle = shelf.clone();
            Rc::new(
                DynamicFolderLink::new(
                    "Shelf",
                    Rc::new(move || Box::new(handle.clone()) as Box<dyn Folder>),
                )
                .with_read_only(true)
                .with_local_listing(),
            )
        }
    };
    let mut home = DynamicFolder::new("Home");
    home.data.child_containers = vec![desk.link(), shelf_link];
    let (mut app, state) = browser_app(Box::new(home), config, ViewMode::Rows);
    let desk_link = state.borrow().root_child_containers()[0].clone();
    state.borrow_mut().open(desk_link.as_ref());
    frame(&mut app);
    (app, state, desk, shelf)
}

fn names(folder: &MemoryFolder) -> Vec<String> {
    let mut folder = folder.clone();
    folder.load();
    folder
        .items()
        .iter()
        .map(|i| i.name().to_string())
        .collect()
}

#[test]
fn a_drop_between_two_places_copies() {
    let (mut app, _state, desk, shelf) = two_places(BrowserConfig::default(), false);
    let at = drag_over(&mut app, "Row Item gear.stl", "Shelf Tree Node");
    assert_eq!(
        property(&app, "Shelf Tree Node", "drop_label"),
        "Copy to Shelf"
    );
    release(&mut app, at, Modifiers::default());
    assert_eq!(names(&desk), ["gear.stl"]);
    assert_eq!(names(&shelf), ["gear.stl"]);
}

#[test]
fn a_read_only_folder_is_never_highlighted_and_takes_nothing() {
    let (mut app, _state, desk, shelf) = two_places(BrowserConfig::default(), true);
    let at = drag_over(&mut app, "Row Item gear.stl", "Shelf Tree Node");
    assert_eq!(
        property(&app, "Shelf Tree Node", "drop_highlighted"),
        "false"
    );
    assert_eq!(property(&app, "Shelf Tree Node", "drop_label"), "");
    release(&mut app, at, Modifiers::default());
    assert_eq!(names(&desk), ["gear.stl"]);
    assert!(names(&shelf).is_empty());
}

/// Rules that take every copy (`take`) or hand it back, noting what they were handed.
struct Offered {
    take: bool,
    seen: RefCell<Vec<String>>,
}

impl PlaceRules for Offered {
    fn take_over_copy(&self, cx: CopyCx) -> CopyTakeover {
        let source = cx
            .source
            .as_ref()
            .map_or(String::new(), |s| s.borrow().name().to_string());
        self.seen.borrow_mut().push(format!(
            "{} item(s) from {source} to {}",
            cx.items.len(),
            cx.target_name
        ));
        match self.take {
            true => {
                (cx.report)("copying the app's way");
                CopyTakeover::Taken
            }
            false => CopyTakeover::Declined(cx),
        }
    }
}

#[test]
fn a_copy_taken_over_is_the_apps_and_one_handed_back_runs_here() {
    for take in [true, false] {
        let rules = Rc::new(Offered {
            take,
            seen: RefCell::new(Vec::new()),
        });
        let config = BrowserConfig::builder().places(rules.clone()).build();
        let (mut app, state, _desk, shelf) = two_places(config, false);
        let at = drag_over(&mut app, "Row Item gear.stl", "Shelf Tree Node");
        release(&mut app, at, Modifiers::default());
        assert_eq!(*rules.seen.borrow(), ["1 item(s) from Desk to Shelf"]);
        match take {
            true => {
                assert!(names(&shelf).is_empty(), "the app runs it");
                assert_eq!(state.borrow().status(), "copying the app's way");
            }
            false => assert_eq!(names(&shelf), ["gear.stl"]),
        }
    }
}

/// The workshop with a folder Inner inside Parts.
fn nested_workshop(label: &str) -> PathBuf {
    let dir = workshop(label);
    std::fs::create_dir_all(dir.join("Parts/Inner")).expect("made");
    dir
}

#[test]
fn a_drag_held_over_a_closed_tree_row_opens_it_after_the_delay() {
    let _clock = agg_gui::clock::scoped_virtual(None);
    let dir = nested_workshop("spring-tree");
    let (mut app, _state) = browser_app(disk(&dir), BrowserConfig::default(), ViewMode::Rows);
    drag_over(&mut app, "Row Item gear.stl", "Parts Tree Node");
    assert_eq!(property(&app, "Parts Tree Node", "expanded"), "false");
    agg_gui::clock::advance(SPRING_DELAY / 2);
    frame(&mut app);
    assert_eq!(
        property(&app, "Parts Tree Node", "expanded"),
        "false",
        "not yet"
    );
    agg_gui::clock::advance(SPRING_DELAY / 2);
    // The paint that finds the wait over opens the row; the next frame shows it.
    frame(&mut app);
    frame(&mut app);
    assert_eq!(property(&app, "Parts Tree Node", "expanded"), "true");

    // The folder it opened takes the drop.
    let at = centre(&app, "Inner Tree Node");
    move_to(&mut app, at);
    move_to(&mut app, Point::new(at.x + 1.0, at.y));
    release(&mut app, Point::new(at.x + 1.0, at.y), Modifiers::default());
    assert!(dir.join("Parts/Inner/gear.stl").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_drag_held_over_a_folder_in_the_list_opens_it_there_and_drops_into_it() {
    let _clock = agg_gui::clock::scoped_virtual(None);
    let dir = workshop("spring-list");
    let (mut app, state) = browser_app(disk(&dir), BrowserConfig::default(), ViewMode::Rows);
    drag_over(&mut app, "Row Item gear.stl", "Parts Row Item Collection");
    agg_gui::clock::advance(SPRING_DELAY);
    frame(&mut app);
    frame(&mut app);
    assert_eq!(
        state.borrow().path_names().last().map(String::as_str),
        Some("Parts")
    );

    // On the empty space of the folder it opened, the drop lands there.
    let empty = Point::new(TREE + 150.0, 20.0);
    move_to(&mut app, empty);
    move_to(&mut app, Point::new(empty.x + 1.0, empty.y));
    release(
        &mut app,
        Point::new(empty.x + 1.0, empty.y),
        Modifiers::default(),
    );
    assert!(dir.join("Parts/gear.stl").exists() && !dir.join("gear.stl").exists());
    assert!(find(app.root(), "Row Item gear.stl", Point::new(0.0, 0.0)).is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The app's view right of the browser: it takes the items and shows them itself.
#[derive(Default)]
struct View {
    calls: RefCell<Vec<String>>,
}

impl OutsideDrops for View {
    fn over(&self, items: &[Rc<dyn Item>], _window_at: Point) -> OutsideOver {
        self.calls
            .borrow_mut()
            .push(format!("over {}", items.len()));
        OutsideOver::Target {
            hides_picture: true,
        }
    }
    fn release(&self, items: &[Rc<dyn Item>], _window_at: Point) -> bool {
        let names: Vec<&str> = items.iter().map(|i| i.name()).collect();
        self.calls
            .borrow_mut()
            .push(format!("release {}", names.join(",")));
        true
    }
    fn cancel(&self) {
        self.calls.borrow_mut().push("cancel".to_string());
    }
}

/// The list's property `key`.
fn list_property(app: &App, key: &str) -> String {
    property(app, "LibraryView", key)
}

#[test]
fn the_picture_follows_hides_over_the_apps_view_and_a_release_there_is_the_apps() {
    let dir = workshop("outside");
    let view = Rc::new(View::default());
    let config = BrowserConfig::builder().drops(view.clone()).build();
    let (mut app, _state) = browser_app(disk(&dir), config, ViewMode::Rows);
    let at = drag_over(&mut app, "Row Item gear.stl", "Parts Row Item Collection");
    let expected = format!("{},{},64,64", at.x + 16.0, at.y - 16.0 - 64.0);
    assert_eq!(list_property(&app, "drag_picture"), expected);
    assert_eq!(list_property(&app, "drag_picture_count"), "1");
    assert!(view.calls.borrow().is_empty(), "inside the browser");

    // Over the app's view, which shows the items itself.
    let outside = Point::new(WIDTH - VIEW / 2.0, HEIGHT / 2.0);
    move_to(&mut app, outside);
    assert_eq!(list_property(&app, "drag_picture"), "");
    // Back inside: the view puts itself back and the picture returns.
    let inside = centre(&app, "Parts Row Item Collection");
    move_to(&mut app, inside);
    assert_ne!(list_property(&app, "drag_picture"), "");
    move_to(&mut app, outside);
    release(&mut app, outside, Modifiers::default());
    assert_eq!(
        *view.calls.borrow(),
        ["over 1", "cancel", "over 1", "release gear.stl"]
    );
    assert!(dir.join("gear.stl").exists(), "the browser moved nothing");
    assert_eq!(list_property(&app, "drag_picture"), "");
    let _ = std::fs::remove_dir_all(&dir);
}
