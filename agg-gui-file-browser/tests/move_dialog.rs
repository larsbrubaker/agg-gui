//! The Move dialog on `SheetDialogs` with a move root, over folders on disk, through agg-gui's
//! `App` with real pointer events: the browser's Move opens it, the user opens a folder in the
//! dialog's own browser and presses Move, the item moves there and Undo puts it back; a folder
//! can't go inside itself, nor items into the folder they are in (the dialog says why and its
//! button is off); where the app's rule copies, the dialog says Copy and the original stays; and
//! a folder something still holds moves nothing and the dialog stays in it.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{App, Modifiers, MouseButton, Point, Rect, Size, Stack, Widget};
use agg_gui_file_browser::{
    run_builtin, run_ready_local_tasks, BrowserConfig, BrowserState, BuiltinAction, FileBrowser,
    PlaceRules, SheetDialogs, ViewMode,
};
use agg_gui_files::disk::{AllFiles, DiskFolder};
use agg_gui_files::{Folder, PlaceId};

const WIDTH: f64 = 800.0;
const HEIGHT: f64 = 600.0;
const DIALOG: &str = "Move Item Window";

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// A fresh scratch folder holding Projects (holding Brackets and bolt.stl), Parts and gear.stl.
fn workshop(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "agg-gui-file-browser-move-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Projects").join("Brackets")).expect("made");
    std::fs::create_dir_all(dir.join("Parts")).expect("made");
    std::fs::write(dir.join("gear.stl"), b"solid gear").expect("written");
    std::fs::write(dir.join("Projects").join("bolt.stl"), b"solid bolt").expect("written");
    dir
}

fn disk(dir: &Path) -> Box<dyn Folder> {
    Box::new(DiskFolder::new(dir, Rc::new(AllFiles)))
}

struct Fixture {
    app: App,
    state: Rc<RefCell<BrowserState>>,
}

/// The tree-and-list browser over `dir` with Move dialogs browsing `dir` too, on a sheet over it.
fn fixture(dir: &Path, places: Option<Rc<dyn PlaceRules>>) -> Fixture {
    agg_gui::set_device_scale(1.0);
    let root = dir.to_path_buf();
    let dialogs = SheetDialogs::new().with_browse_root(move || disk(&root));
    let mut builder = BrowserConfig::builder().dialogs(Rc::new(dialogs.clone()));
    if let Some(places) = places {
        builder = builder.places(places);
    }
    let state = BrowserState::new(disk(dir), builder.build());
    state.borrow_mut().set_view_mode(ViewMode::Rows);
    let root = Stack::new()
        .add(Box::new(FileBrowser::new(&state, font())))
        .add(Box::new(dialogs.sheet(font())));
    let mut app = App::new(Box::new(root));
    layout(&mut app);
    Fixture { app, state }
}

fn layout(app: &mut App) {
    app.layout(Size::new(WIDTH, HEIGHT));
    app.layout(Size::new(WIDTH, HEIGHT));
}

/// The widget named `id` under `widget` and its rectangle in the root's coordinates (Y up).
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

/// The widget named `id` inside the dialog, with the dialog's origin.
fn in_dialog<'a>(app: &'a App, id: &str) -> Option<(&'a dyn Widget, Rect)> {
    let (dialog, rect) = find(app.root(), DIALOG, Point::new(0.0, 0.0))?;
    let b = dialog.bounds();
    let origin = Point::new(rect.x - b.x, rect.y - b.y);
    find(dialog, id, origin)
}

fn dialog_property(app: &App, key: &str) -> String {
    let (dialog, _) = find(app.root(), DIALOG, Point::new(0.0, 0.0)).expect("the dialog shows");
    dialog
        .properties()
        .into_iter()
        .find(|(k, _)| *k == key)
        .unwrap_or_else(|| panic!("the dialog has no {key}"))
        .1
}

fn press_at(app: &mut App, r: Rect, clicks: u32) {
    let at = Point::new(r.x + r.width / 2.0, HEIGHT - (r.y + r.height / 2.0));
    app.on_mouse_move(at.x, at.y);
    app.on_mouse_down_clicks(at.x, at.y, MouseButton::Left, Modifiers::default(), clicks);
    app.on_mouse_up(at.x, at.y, MouseButton::Left, Modifiers::default());
    layout(app);
}

/// Clicks the main browser's row `id` (the browser's own, under the sheet).
fn select_in_browser(f: &mut Fixture, id: &str) {
    let (_, r) = find(f.app.root(), id, Point::new(0.0, 0.0)).expect("the row shows");
    press_at(&mut f.app, r, 1);
}

/// Clicks the dialog's widget `id`, `clicks` times in a row.
fn click_in_dialog(f: &mut Fixture, id: &str, clicks: u32) {
    let (_, r) = in_dialog(&f.app, id).unwrap_or_else(|| panic!("no {id} in the dialog"));
    for n in 1..=clicks {
        press_at(&mut f.app, r, n);
    }
}

/// Runs Move on what the main browser has selected; the dialog shows.
fn open_move(f: &mut Fixture) {
    assert!(run_builtin(&f.state, BuiltinAction::Move), "Move runs");
    layout(&mut f.app);
    assert!(in_dialog(&f.app, "Browser Dialog Confirm Button").is_some());
}

fn settle(f: &mut Fixture) {
    for _ in 0..50 {
        run_ready_local_tasks();
    }
    layout(&mut f.app);
}

#[test]
fn the_dialog_moves_the_item_into_the_folder_opened_and_undo_puts_it_back() {
    let dir = workshop("moves");
    let mut f = fixture(&dir, None);
    select_in_browser(&mut f, "Row Item gear.stl");
    open_move(&mut f);
    click_in_dialog(&mut f, "Parts Row Item Collection", 2);
    assert_eq!(
        dialog_property(&f.app, "summary"),
        "Move gear.stl to Parts."
    );
    assert_eq!(dialog_property(&f.app, "accept_text"), "Move");
    click_in_dialog(&mut f, "Browser Dialog Confirm Button", 1);
    settle(&mut f);
    assert!(
        in_dialog(&f.app, "Browser Dialog Confirm Button").is_none(),
        "closed"
    );
    assert!(dir.join("Parts").join("gear.stl").exists());
    assert!(!dir.join("gear.stl").exists());
    assert!(BrowserState::undo(&f.state));
    settle(&mut f);
    assert!(dir.join("gear.stl").exists(), "undo puts it back");
    assert!(!dir.join("Parts").join("gear.stl").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_selected_folder_row_is_the_destination() {
    let dir = workshop("row");
    let mut f = fixture(&dir, None);
    select_in_browser(&mut f, "Row Item gear.stl");
    open_move(&mut f);
    click_in_dialog(&mut f, "Projects Row Item Collection", 1);
    assert_eq!(
        dialog_property(&f.app, "summary"),
        "Move gear.stl to Projects."
    );
    click_in_dialog(&mut f, "Browser Dialog Confirm Button", 1);
    settle(&mut f);
    assert!(dir.join("Projects").join("gear.stl").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_folder_cant_go_inside_itself_nor_items_where_they_already_are() {
    let dir = workshop("refused");
    let mut f = fixture(&dir, None);
    select_in_browser(&mut f, "Projects Row Item Collection");
    open_move(&mut f);
    click_in_dialog(&mut f, "Projects Row Item Collection", 1);
    let itself = "Projects can't go inside itself. Choose another folder.";
    assert_eq!(dialog_property(&f.app, "summary"), itself);
    assert_eq!(dialog_property(&f.app, "accept_enabled"), "false");
    // Inside it, a folder there is inside it too.
    click_in_dialog(&mut f, "Projects Row Item Collection", 2);
    click_in_dialog(&mut f, "Brackets Row Item Collection", 1);
    assert_eq!(dialog_property(&f.app, "summary"), itself);
    // A press on the disabled button does nothing.
    click_in_dialog(&mut f, "Browser Dialog Confirm Button", 1);
    assert!(in_dialog(&f.app, "Browser Dialog Confirm Button").is_some());
    click_in_dialog(&mut f, "Browser Dialog Cancel Button", 1);
    assert!(in_dialog(&f.app, "Browser Dialog Confirm Button").is_none());
    assert!(
        dir.join("Projects").join("Brackets").is_dir(),
        "nothing moved"
    );

    // bolt.stl, from inside Projects, into Projects.
    let mut f = fixture(&dir, None);
    let (_, r) = find(f.app.root(), "Projects Tree Node", Point::new(0.0, 0.0)).expect("row");
    press_at(&mut f.app, r, 1);
    select_in_browser(&mut f, "Row Item bolt.stl");
    open_move(&mut f);
    click_in_dialog(&mut f, "Projects Row Item Collection", 1);
    assert_eq!(
        dialog_property(&f.app, "summary"),
        "bolt.stl is already in Projects."
    );
    assert_eq!(dialog_property(&f.app, "accept_enabled"), "false");
    let _ = std::fs::remove_dir_all(&dir);
}

/// An app whose every drop copies.
struct AlwaysCopies;

impl PlaceRules for AlwaysCopies {
    fn drop_copies(&self, _: Option<&dyn Folder>, _: Option<PlaceId>, _: bool) -> bool {
        true
    }
}

#[test]
fn where_the_rule_copies_the_dialog_says_copy_and_the_original_stays() {
    let dir = workshop("copies");
    let mut f = fixture(&dir, Some(Rc::new(AlwaysCopies)));
    select_in_browser(&mut f, "Row Item gear.stl");
    open_move(&mut f);
    click_in_dialog(&mut f, "Parts Row Item Collection", 1);
    let home = dir.file_name().and_then(|n| n.to_str()).expect("a name");
    assert_eq!(
        dialog_property(&f.app, "summary"),
        format!("Copy gear.stl to Parts. The original stays in {home}.")
    );
    assert_eq!(dialog_property(&f.app, "accept_text"), "Copy");
    click_in_dialog(&mut f, "Browser Dialog Confirm Button", 1);
    settle(&mut f);
    assert!(dir.join("Parts").join("gear.stl").exists(), "copied");
    assert!(dir.join("gear.stl").exists(), "the original stays");
    let _ = std::fs::remove_dir_all(&dir);
}

/// An app's memory: each key's open path by folder names, found again from a new root on disk.
struct Remembered {
    dir: PathBuf,
    saved: RefCell<std::collections::HashMap<String, Vec<String>>>,
}

impl agg_gui_file_browser::BrowserMemory for Remembered {
    fn remember_location(&self, key: &str, path: &[agg_gui_file_browser::OpenFolder]) {
        let names = path[1..]
            .iter()
            .map(|f| f.borrow().name().to_string())
            .collect();
        self.saved.borrow_mut().insert(key.to_string(), names);
    }
    fn last_location(&self, key: &str) -> Option<agg_gui_file_browser::LocationFuture> {
        let names = self.saved.borrow().get(key).cloned()?;
        let dir = self.dir.clone();
        Some(Box::pin(async move {
            let mut opened: Vec<Box<dyn Folder>> = Vec::new();
            let mut at = disk(&dir);
            for name in names {
                at.load();
                let link = at
                    .child_containers()
                    .into_iter()
                    .find(|l| l.name() == name)?;
                opened.push(link.get_container().ok()?);
                at = link.get_container().ok()?;
            }
            Some(opened)
        }))
    }
}

#[test]
fn the_dialog_browses_with_the_apps_config_and_opens_where_it_was_left() {
    let dir = workshop("remembers");
    agg_gui::set_device_scale(1.0);
    let memory = Rc::new(Remembered {
        dir: dir.clone(),
        saved: RefCell::default(),
    });
    let names = agg_gui_file_browser::BrowserNames {
        page_nav_bar: "App Page Bar".into(),
        ..agg_gui_file_browser::BrowserNames::default()
    };
    let look: Rc<dyn agg_gui_file_browser::BrowserLook> =
        Rc::new(agg_gui_file_browser::DefaultLook::with_names(names));
    let (root, config_memory) = (dir.clone(), Rc::clone(&memory));
    let dialogs = SheetDialogs::new()
        .with_browse_root(move || disk(&root))
        .with_browse_config(move || {
            BrowserConfig::builder()
                .look(Rc::clone(&look))
                .memory(config_memory.clone())
                .options(agg_gui_file_browser::dialog_options())
                .build()
        })
        .with_move_location_key("MoveItemLastBrowsedPath");
    let state = BrowserState::new(
        disk(&dir),
        BrowserConfig::builder()
            .dialogs(Rc::new(dialogs.clone()))
            .build(),
    );
    state.borrow_mut().set_view_mode(ViewMode::Rows);
    let root = Stack::new()
        .add(Box::new(FileBrowser::new(&state, font())))
        .add(Box::new(dialogs.sheet(font())));
    let mut f = Fixture {
        app: App::new(Box::new(root)),
        state,
    };
    layout(&mut f.app);
    select_in_browser(&mut f, "Row Item gear.stl");
    open_move(&mut f);
    assert!(
        in_dialog(&f.app, "App Page Bar").is_some(),
        "the app's look"
    );
    click_in_dialog(&mut f, "Projects Row Item Collection", 2);
    assert_eq!(
        memory.saved.borrow().get("MoveItemLastBrowsedPath"),
        Some(&vec!["Projects".to_string()])
    );
    click_in_dialog(&mut f, "Browser Dialog Cancel Button", 1);
    // Opened again: where it was left.
    open_move(&mut f);
    settle(&mut f);
    let trail = dialog_property(&f.app, "bread_crumb_trail");
    assert!(trail.ends_with("/Projects"), "{trail}");
    // Accepting there moves into Projects and remembers nothing new.
    click_in_dialog(&mut f, "Browser Dialog Confirm Button", 1);
    settle(&mut f);
    assert!(dir.join("Projects").join("gear.stl").exists());
    assert_eq!(
        memory.saved.borrow().get("MoveItemLastBrowsedPath"),
        Some(&vec!["Projects".to_string()])
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Remembers nothing, but holds on to every folder it is handed: something else holding the
/// folder the dialog would hand over.
#[derive(Default)]
struct Holding {
    held: RefCell<Vec<agg_gui_file_browser::OpenFolder>>,
}

impl agg_gui_file_browser::BrowserMemory for Holding {
    fn remember_location(&self, _key: &str, path: &[agg_gui_file_browser::OpenFolder]) {
        self.held.borrow_mut().extend(path.iter().cloned());
    }
}

#[test]
fn a_folder_something_still_holds_moves_nothing_and_the_dialog_stays_in_it() {
    let dir = workshop("held");
    agg_gui::set_device_scale(1.0);
    let holding = Rc::new(Holding::default());
    let (root, memory) = (dir.clone(), Rc::clone(&holding));
    let dialogs = SheetDialogs::new()
        .with_browse_root(move || disk(&root))
        .with_browse_config(move || {
            BrowserConfig::builder()
                .memory(memory.clone())
                .options(agg_gui_file_browser::dialog_options())
                .build()
        })
        .with_move_location_key("MoveItemLastBrowsedPath");
    let state = BrowserState::new(
        disk(&dir),
        BrowserConfig::builder()
            .dialogs(Rc::new(dialogs.clone()))
            .build(),
    );
    state.borrow_mut().set_view_mode(ViewMode::Rows);
    let root = Stack::new()
        .add(Box::new(FileBrowser::new(&state, font())))
        .add(Box::new(dialogs.sheet(font())));
    let mut f = Fixture {
        app: App::new(Box::new(root)),
        state,
    };
    layout(&mut f.app);
    select_in_browser(&mut f, "Row Item gear.stl");
    open_move(&mut f);
    click_in_dialog(&mut f, "Parts Row Item Collection", 2);
    click_in_dialog(&mut f, "Browser Dialog Confirm Button", 1);
    settle(&mut f);
    assert!(dir.join("gear.stl").exists(), "nothing moved");
    let trail = dialog_property(&f.app, "bread_crumb_trail");
    assert!(
        trail.ends_with("/Parts"),
        "the dialog stays in the folder: {trail}"
    );
    holding.held.borrow_mut().clear();
    click_in_dialog(&mut f, "Browser Dialog Confirm Button", 1);
    settle(&mut f);
    assert!(
        dir.join("Parts").join("gear.stl").exists(),
        "moved once let go"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
