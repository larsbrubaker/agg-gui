//! The browser's own actions run from its menus, driven through agg-gui's `App` with real events
//! on `MemoryFolder`s and a scratch `DiskFolder`: Create Folder... asks a name on the default
//! sheet and makes the folder (undone by Undo), Rename from the list and from the tree asks over
//! the current name (undone by Undo), Remove asks first and sends a folder on disk's items to the
//! trash (put back by Undo), the Delete key runs Remove, a read-only folder greys every write, an
//! app's ordered list shows with its icons, shortcuts and submenu as given, the overflow button
//! opens the folder's whole list, and Open File Location shows where the app's locate hook says
//! an item lives.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::icon_image::IconImage;
use agg_gui::text::Font;
use agg_gui::widgets::menu::MenuEntry;
use agg_gui::{App, Color, Key, Modifiers, MouseButton, Point, Rect, Size, Stack, Widget};
use agg_gui_file_browser::{
    BrowserAction, BrowserActions, BrowserCommand, BrowserConfig, BrowserList, BrowserLook,
    BrowserNames, BrowserRef, BrowserState, BrowserStyle, BrowserTree, BuiltinAction, Builtins,
    ItemLocation, OverflowButton, SheetDialogs,
};
use agg_gui_files::disk::{set_trash_directory, AllFiles, DiskFolder, DiskRules};
use agg_gui_files::{Folder, Item, MemoryFolder};

const WIDTH: f64 = 400.0;
const HEIGHT: f64 = 300.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// A fresh, empty folder under the system's temp directory for one test.
fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "agg-gui-file-browser-actions-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("the scratch folder is made");
    path
}

/// An app showing `view` with the dialogs' sheet over it.
fn app_with_sheet(view: Box<dyn Widget>, dialogs: &SheetDialogs) -> App {
    agg_gui::set_device_scale(1.0);
    let root = Stack::new().add(view).add(Box::new(dialogs.sheet(font())));
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(WIDTH, HEIGHT));
    app
}

/// A browser at `root` whose dialogs are a `SheetDialogs`, with `actions` when given; its list in
/// an app with the sheet over it.
fn list_app(
    root: Box<dyn Folder>,
    actions: Option<Rc<dyn BrowserActions>>,
) -> (App, Rc<RefCell<BrowserState>>) {
    let dialogs = SheetDialogs::new();
    let mut builder = BrowserConfig::builder().dialogs(Rc::new(dialogs.clone()));
    if let Some(actions) = actions {
        builder = builder.actions(actions);
    }
    let state = BrowserState::new(root, builder.build());
    let app = app_with_sheet(Box::new(BrowserList::new(&state, font())), &dialogs);
    (app, state)
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

/// The widget named `id` and its rectangle in the root's coordinates (Y up).
fn named<'a>(app: &'a App, id: &str) -> Option<(&'a dyn Widget, Rect)> {
    find_path(app.root(), &|w| w.id() == Some(id), Point::new(0.0, 0.0))
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

fn list(app: &App) -> &BrowserList {
    of_type(app, "BrowserList")
}

fn centre(r: Rect) -> Point {
    Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0)
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

/// Picks the open menu's row `title` (of `rows`, the menu's rows) with the keys, as a user
/// stepping down its enabled rows would.
fn pick(app: &mut App, rows: &[(String, bool)], title: &str) {
    let at = rows
        .iter()
        .filter(|(t, enabled)| *enabled && !t.is_empty())
        .position(|(t, _)| t == title)
        .unwrap_or_else(|| panic!("{title} is not an enabled row of {rows:?}"));
    // The pointer away from the menu, so no row is hovered and Down starts from the top.
    app.on_mouse_move(2.0, 2.0);
    for _ in 0..=at {
        key(app, Key::ArrowDown);
    }
    key(app, Key::Enter);
}

/// Types `text` into the sheet's focused name field and accepts it with Enter.
fn answer_name(app: &mut App, text: &str) {
    assert!(
        named(app, "Browser Dialog Name Field").is_some(),
        "the name sheet shows"
    );
    app.on_text_input(text);
    app.layout(Size::new(WIDTH, HEIGHT));
    key(app, Key::Enter);
}

/// Presses the sheet's confirming button.
fn press_confirm(app: &mut App) {
    let (_, rect) = named(app, "Browser Dialog Confirm Button").expect("the question shows");
    click_at(app, centre(rect), MouseButton::Left);
}

/// The text of every label in the app (the sheet's title, message and button labels).
fn label_texts(app: &App) -> Vec<String> {
    fn walk(widget: &dyn Widget, out: &mut Vec<String>) {
        if widget.type_name() == "Label" {
            if let Some((_, text)) = widget.properties().into_iter().find(|(k, _)| *k == "text") {
                out.push(text);
            }
        }
        for child in widget.children() {
            walk(child.as_ref(), out);
        }
    }
    let mut out = Vec::new();
    walk(app.root(), &mut out);
    out
}

fn names(state: &Rc<RefCell<BrowserState>>) -> Vec<String> {
    state
        .borrow()
        .entries()
        .iter()
        .map(|e| e.name().to_string())
        .collect()
}

/// A right press on the row of `name` (selecting it), opening the selection's menu.
fn row_menu(app: &mut App, name: &str) -> Vec<(String, bool)> {
    let (_, rect) = named(app, &format!("Row Item {name}")).expect("the row shows");
    click_at(app, centre(rect), MouseButton::Right);
    list(app).menu_rows()
}

/// A right press on the list's empty space, opening the folder's menu.
fn folder_menu(app: &mut App) -> Vec<(String, bool)> {
    click_at(app, Point::new(200.0, 5.0), MouseButton::Right);
    list(app).menu_rows()
}

#[test]
fn create_folder_asks_a_name_on_the_sheet_and_undo_removes_the_folder() {
    let dir = scratch("create");
    let folder = DiskFolder::new(&dir, Rc::new(AllFiles));
    let (mut app, state) = list_app(Box::new(folder), None);
    let rows = folder_menu(&mut app);
    assert_eq!(
        rows,
        [
            ("Add".to_string(), false),
            ("Create Folder...".to_string(), true)
        ],
        "no file picker: Add is greyed"
    );
    pick(&mut app, &rows, "Create Folder...");
    answer_name(&mut app, "Brackets");
    assert!(dir.join("Brackets").is_dir());
    assert_eq!(names(&state), ["Brackets"]);
    assert!(
        named(&app, "Browser Dialog Name Field").is_none(),
        "the sheet closed"
    );

    assert!(BrowserState::undo(&state));
    assert!(!dir.join("Brackets").exists());
    assert_eq!(names(&state), Vec::<String>::new());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_name_creates_nothing_and_cancel_closes_the_sheet() {
    let home = MemoryFolder::new("Home");
    let (mut app, state) = list_app(Box::new(home), None);
    let rows = folder_menu(&mut app);
    pick(&mut app, &rows, "Create Folder...");
    key(&mut app, Key::Enter);
    assert!(
        named(&app, "Browser Dialog Name Field").is_some(),
        "the sheet stays"
    );
    let (_, cancel) = named(&app, "Browser Dialog Cancel Button").expect("Cancel shows");
    click_at(&mut app, centre(cancel), MouseButton::Left);
    assert!(named(&app, "Browser Dialog Name Field").is_none());
    assert_eq!(names(&state), Vec::<String>::new());
}

#[test]
fn rename_from_the_list_asks_over_the_current_name_and_undo_puts_it_back() {
    let dir = scratch("rename-list");
    std::fs::write(dir.join("gear.stl"), b"solid gear").expect("written");
    let folder = DiskFolder::new(&dir, Rc::new(AllFiles));
    let (mut app, state) = list_app(Box::new(folder), None);
    let rows = row_menu(&mut app, "gear.stl");
    pick(&mut app, &rows, "Rename");
    // The field starts with the name, all of it selected: typing replaces it.
    answer_name(&mut app, "cog.stl");
    assert!(dir.join("cog.stl").exists() && !dir.join("gear.stl").exists());
    assert_eq!(names(&state), ["cog.stl"]);

    assert!(BrowserState::undo(&state));
    assert!(dir.join("gear.stl").exists());
    assert_eq!(names(&state), ["gear.stl"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rename_from_the_tree_renames_the_folder_shows_it_by_its_new_name_and_undoes() {
    let dir = scratch("rename-tree");
    std::fs::create_dir_all(dir.join("Projects")).expect("made");
    let dialogs = SheetDialogs::new();
    let config = BrowserConfig::builder()
        .dialogs(Rc::new(dialogs.clone()))
        .build();
    let state = BrowserState::new(Box::new(DiskFolder::new(&dir, Rc::new(AllFiles))), config);
    let mut app = app_with_sheet(Box::new(BrowserTree::for_state(&state, font())), &dialogs);
    let (_, rect) = named(&app, "Projects Tree Node").expect("the tree row shows");
    let at = Point::new(rect.x + 80.0, rect.y + rect.height / 2.0);
    click_at(&mut app, at, MouseButton::Right);
    assert_eq!(state.borrow().path_names().len(), 2, "the folder shows");
    let tree: &BrowserTree = of_type(&app, "TreeView");
    let rows = tree.menu_rows();
    pick(&mut app, &rows, "Rename");
    answer_name(&mut app, "Fixtures");
    assert!(dir.join("Fixtures").is_dir() && !dir.join("Projects").exists());
    let path = state.borrow().path_names();
    assert_eq!(
        path.last().map(String::as_str),
        Some("Fixtures"),
        "{path:?}"
    );
    assert!(named(&app, "Fixtures Tree Node").is_some());

    assert!(BrowserState::undo(&state));
    assert!(dir.join("Projects").is_dir());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn remove_asks_first_sends_the_items_to_the_trash_and_undo_puts_them_back() {
    let dir = scratch("remove");
    let trash = scratch("remove-trash");
    set_trash_directory(Some(trash.clone()));
    std::fs::write(dir.join("gear.stl"), b"solid gear").expect("written");
    let folder = DiskFolder::new(&dir, Rc::new(AllFiles));
    let (mut app, state) = list_app(Box::new(folder), None);
    let rows = row_menu(&mut app, "gear.stl");
    pick(&mut app, &rows, "Remove");
    // Nothing goes before the answer, and the question says where the items go.
    assert!(dir.join("gear.stl").exists());
    let (title, button) = match cfg!(windows) {
        true => ("Move to Recycle Bin?", "Move to Recycle Bin"),
        false => ("Move to Trash?", "Move to Trash"),
    };
    let texts = label_texts(&app);
    assert!(texts.iter().any(|t| t == title), "{texts:?}");
    assert!(texts.iter().any(|t| t == button), "{texts:?}");
    press_confirm(&mut app);
    assert!(!dir.join("gear.stl").exists());
    assert_eq!(names(&state), Vec::<String>::new());

    assert!(BrowserState::undo(&state));
    assert!(dir.join("gear.stl").exists());
    assert_eq!(names(&state), ["gear.stl"]);
    set_trash_directory(None);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&trash);
}

#[test]
fn the_delete_key_runs_remove_with_its_question() {
    let home = MemoryFolder::new("Home");
    home.add_file("a.txt", b"a".to_vec());
    home.add_file("b.txt", b"b".to_vec());
    let (mut app, state) = list_app(Box::new(home), None);
    let (_, rect) = named(&app, "Row Item a.txt").expect("the row shows");
    click_at(&mut app, centre(rect), MouseButton::Left);
    key(&mut app, Key::Delete);
    assert_eq!(names(&state), ["a.txt", "b.txt"], "asked first");
    press_confirm(&mut app);
    assert_eq!(names(&state), ["b.txt"]);
}

/// Rules for a folder on disk that takes no changes.
struct ReadOnly;

impl DiskRules for ReadOnly {
    fn lists_file(&self, _name: &str) -> bool {
        true
    }
    fn read_only(&self) -> bool {
        true
    }
}

#[test]
fn a_read_only_folder_greys_every_write() {
    let dir = scratch("read-only");
    std::fs::write(dir.join("gear.stl"), b"solid gear").expect("written");
    let dialogs = SheetDialogs::new();
    let config = BrowserConfig::builder()
        .dialogs(Rc::new(dialogs.clone()))
        .build();
    let state = BrowserState::new(Box::new(DiskFolder::new(&dir, Rc::new(ReadOnly))), config);
    let mut app = app_with_sheet(Box::new(OverflowButton::new(&state, font())), &dialogs);
    state.borrow_mut().select_all();
    click_at(&mut app, Point::new(16.0, 16.0), MouseButton::Left);
    let button: &OverflowButton = of_type(&app, "OverflowButton");
    assert!(button.menu_is_open());
    assert!(
        button.menu_rows().iter().all(|(_, enabled)| !enabled),
        "{:?}",
        button.menu_rows()
    );
    // The selection's menu, which shows only what can run, has nothing: it doesn't open.
    let (mut list_app, _) = list_app(Box::new(DiskFolder::new(&dir, Rc::new(ReadOnly))), None);
    row_menu(&mut list_app, "gear.stl");
    assert!(!list(&list_app).menu_is_open());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A look whose icon keys resolve to a 16-pixel square (any key it is asked).
struct KeyedLook {
    names: BrowserNames,
}

impl BrowserLook for KeyedLook {
    fn style(&self) -> BrowserStyle {
        BrowserStyle::default()
    }
    fn icon_for_key(&self, _key: &str, logical: Size, _tint: Color) -> Option<IconImage> {
        // A 16-unit side: no overflow.
        let side = logical.width as u32;
        IconImage::from_rgba(
            Arc::new(vec![255; (side * side * 4) as usize]),
            side,
            side,
            logical,
        )
    }
    fn names(&self) -> &BrowserNames {
        &self.names
    }
}

/// An app's ordered list: its own Open (an icon and a shortcut), the built-ins, and an Export
/// submenu of two formats; Empty Trash only when it can run (never, here).
struct AppMenu;

impl BrowserActions for AppMenu {
    fn menu(&self, _state: &BrowserRef, builtins: &Builtins) -> Vec<BrowserAction> {
        let mut rows = vec![
            BrowserAction::Command(
                BrowserCommand::new("Open", || {})
                    .with_icon("open.svg")
                    .with_shortcut("Ctrl+O")
                    .with_scope(agg_gui_file_browser::ActionScope::Selection),
            ),
            BrowserAction::Separator,
            BrowserAction::Command(BrowserCommand::submenu(
                "Export",
                vec![
                    BrowserAction::Command(BrowserCommand::new("STL", || {})),
                    BrowserAction::Command(BrowserCommand::new("3MF", || {})),
                ],
            )),
            BrowserAction::Command(
                BrowserCommand::new("Empty Trash", || {})
                    .enabled(false)
                    .only_when_enabled(),
            ),
            BrowserAction::Separator,
        ];
        rows.extend(
            builtins
                .offered()
                .iter()
                .map(|&a| BrowserAction::Builtin(a)),
        );
        rows
    }
}

#[test]
fn an_apps_ordered_list_shows_with_its_icons_shortcuts_and_submenu() {
    let home = MemoryFolder::new("Home");
    home.add_file("a.txt", b"a".to_vec());
    let dialogs = SheetDialogs::new();
    let config = BrowserConfig::builder()
        .dialogs(Rc::new(dialogs.clone()))
        .actions(Rc::new(AppMenu))
        .look(Rc::new(KeyedLook {
            names: BrowserNames::default(),
        }))
        .build();
    let state = BrowserState::new(Box::new(home), config);
    let mut app = app_with_sheet(Box::new(BrowserList::new(&state, font())), &dialogs);
    let (_, rect) = named(&app, "Row Item a.txt").expect("the row shows");
    click_at(&mut app, centre(rect), MouseButton::Right);
    let rows = list(&app).menu_rows();
    assert_eq!(
        rows.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
        ["Open", "", "Export", "", "Rename", "Remove"]
    );
    let entries = list(&app).menu_entries().to_vec();
    let MenuEntry::Item(open) = &entries[0] else {
        panic!("Open is a row");
    };
    assert_eq!(open.shortcut.as_deref(), Some("Ctrl+O"));
    assert!(open.icon_image.is_some(), "the look's icon for open.svg");
    let MenuEntry::Item(export) = &entries[2] else {
        panic!("Export is a row");
    };
    let formats: Vec<&str> = export
        .submenu
        .iter()
        .filter_map(|e| match e {
            MenuEntry::Item(item) => Some(item.label.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(formats, ["STL", "3MF"]);
    let MenuEntry::Item(rename) = &entries[4] else {
        panic!("Rename is a row");
    };
    assert!(
        rename.icon_image.is_some(),
        "the built-in's icon key goes to the look"
    );
    // The list's own menu: no selection rows; the built-ins adding to the folder and the
    // unscoped Export.
    key(&mut app, Key::Escape);
    click_at(&mut app, Point::new(200.0, 5.0), MouseButton::Right);
    let rows = list(&app).menu_rows();
    assert_eq!(
        rows.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
        ["Export", "", "Add", "Create Folder..."]
    );
}

#[test]
fn the_overflow_button_opens_the_folders_whole_list() {
    let home = MemoryFolder::new("Home");
    home.add_file("a.txt", b"a".to_vec());
    let dialogs = SheetDialogs::new();
    let config = BrowserConfig::builder()
        .dialogs(Rc::new(dialogs.clone()))
        .build();
    let state = BrowserState::new(Box::new(home), config);
    let mut app = app_with_sheet(Box::new(OverflowButton::new(&state, font())), &dialogs);
    assert!(named(&app, &BrowserNames::default().overflow_menu).is_some());
    click_at(&mut app, Point::new(16.0, 16.0), MouseButton::Left);
    let button: &OverflowButton = of_type(&app, "OverflowButton");
    assert_eq!(
        button.menu_rows(),
        [
            ("Add".to_string(), false),
            ("Create Folder...".to_string(), true),
            (String::new(), false),
            ("Open File Location".to_string(), false),
            (String::new(), false),
            ("Rename".to_string(), false),
            ("Move".to_string(), false),
            ("Remove".to_string(), false),
        ],
        "every row, greyed when it can't run (nothing is selected)"
    );
    let rows = button.menu_rows();
    pick(&mut app, &rows, "Create Folder...");
    answer_name(&mut app, "Notes");
    assert_eq!(names(&state), ["Notes", "a.txt"]);
}

/// Locates every item in the folder `Archive` below the open one.
struct Locating;

impl BrowserActions for Locating {
    fn can_locate(&self, _state: &BrowserRef, _item: &dyn Item) -> bool {
        true
    }
    fn locate(&self, state: &BrowserRef, _item: &Rc<dyn Item>) -> Option<ItemLocation> {
        let live = state.live()?;
        let s = live.borrow();
        let archive = s
            .active_container()?
            .child_containers()
            .into_iter()
            .find(|l| l.name() == "Archive")?;
        let folder = archive.get_container().ok()?;
        Some(ItemLocation {
            base: s.depth() - 1,
            folders: Some(vec![folder]),
        })
    }
}

#[test]
fn open_file_location_shows_where_the_locate_hook_says_the_item_lives() {
    let home = MemoryFolder::new("Home");
    home.add_folder("Archive").expect("free");
    home.add_file("a.txt", b"a".to_vec());
    let (mut app, state) = list_app(Box::new(home), Some(Rc::new(Locating)));
    let rows = row_menu(&mut app, "a.txt");
    assert!(
        rows.contains(&("Open File Location".to_string(), true)),
        "{rows:?}"
    );
    pick(&mut app, &rows, "Open File Location");
    assert_eq!(state.borrow().path_names(), ["Home", "Archive"]);
}

#[test]
fn a_built_in_the_app_places_runs_through_run_builtin_only_when_it_can() {
    let home = MemoryFolder::new("Home");
    home.add_file("a.txt", b"a".to_vec());
    let state = BrowserState::new(Box::new(home), BrowserConfig::default());
    state.borrow_mut().select_all();
    // No dialogs: Remove can't ask, so it can't run.
    assert!(!agg_gui_file_browser::run_builtin(
        &state,
        BuiltinAction::Remove
    ));
    assert_eq!(names(&state), ["a.txt"]);
}
