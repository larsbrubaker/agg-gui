//! The Open dialog on `SheetDialogs` over folders in memory, through agg-gui's `App` with real
//! pointer events: it lists every folder (read-only ones too) and only the files the request's
//! filter keeps; a double click on a file opens it; Open is enabled only with a file (or one
//! folder, which it opens) selected; one file at a time unless the request allows several; and
//! Cancel answers nothing and the dialog opens where it was left.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{App, Modifiers, MouseButton, Point, Rect, Size, Stack, Widget};
use agg_gui_file_browser::{
    dialog_options, extension_filter, run_ready_local_tasks, BrowserConfig, BrowserDialogs,
    BrowserMemory, DialogKind, LocationFuture, OpenFolder, OpenPick, OpenRequest, SheetDialogs,
};
use agg_gui_files::{DynamicFolder, DynamicFolderLink, Folder, MemoryFolder};

const WIDTH: f64 = 800.0;
const HEIGHT: f64 = 600.0;
const DIALOG: &str = "Open Window";

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// The top: Designs (holding a.stl, b.stl and notes.txt) and Examples (read-only, holding
/// cube.stl).
fn root() -> Box<dyn Folder> {
    let designs = MemoryFolder::new("Designs");
    designs.add_file("a.stl", b"a".to_vec());
    designs.add_file("b.stl", b"b".to_vec());
    designs.add_file("notes.txt", b"n".to_vec());
    let examples = MemoryFolder::new("Examples");
    examples.add_file("cube.stl", b"c".to_vec());
    let mut root = DynamicFolder::new("Top");
    root.data.child_containers = vec![
        Rc::new(DynamicFolderLink::new(
            "Designs",
            Rc::new(move || Box::new(designs.clone()) as Box<dyn Folder>),
        )),
        Rc::new(
            DynamicFolderLink::new(
                "Examples",
                Rc::new(move || Box::new(examples.clone()) as Box<dyn Folder>),
            )
            .with_read_only(true),
        ),
    ];
    Box::new(root)
}

/// Remembers each key's path by folder names and finds it again from a new root.
#[derive(Default)]
struct Remembered {
    saved: RefCell<HashMap<String, Vec<String>>>,
}

impl BrowserMemory for Remembered {
    fn remember_location(&self, key: &str, path: &[OpenFolder]) {
        let names = path[1..]
            .iter()
            .map(|f| f.borrow().name().to_string())
            .collect();
        self.saved.borrow_mut().insert(key.to_string(), names);
    }
    fn last_location(&self, key: &str) -> Option<LocationFuture> {
        let names = self.saved.borrow().get(key).cloned()?;
        let mut at = root();
        Some(Box::pin(async move {
            let mut opened = Vec::new();
            for name in names {
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

type Picked = Rc<RefCell<Option<Option<OpenPick>>>>;

struct Fixture {
    app: App,
    dialogs: SheetDialogs,
    picked: Picked,
}

fn fixture(memory: Option<Rc<Remembered>>) -> Fixture {
    agg_gui::set_device_scale(1.0);
    let mut dialogs = SheetDialogs::new()
        .with_browse_root(root)
        .with_open_location_key("OpenLastBrowsedPath");
    if let Some(memory) = memory {
        dialogs = dialogs.with_browse_config(move || {
            BrowserConfig::builder()
                .memory(memory.clone())
                .options(dialog_options())
                .build()
        });
    }
    let stack = Stack::new().add(Box::new(dialogs.sheet(font())));
    let mut app = App::new(Box::new(stack));
    layout(&mut app);
    Fixture {
        app,
        dialogs,
        picked: Rc::new(RefCell::new(None)),
    }
}

fn layout(app: &mut App) {
    app.layout(Size::new(WIDTH, HEIGHT));
    app.layout(Size::new(WIDTH, HEIGHT));
}

/// Asks to open `.stl` files, several at once when `multiple`.
fn open(f: &mut Fixture, multiple: bool) {
    let picked = Rc::clone(&f.picked);
    let request = OpenRequest::new()
        .with_multiple(multiple)
        .with_filter(extension_filter(&[".stl"]));
    f.dialogs.pick_open_target(
        request,
        Box::new(move |pick| *picked.borrow_mut() = Some(pick)),
    );
    layout(&mut f.app);
    for _ in 0..20 {
        run_ready_local_tasks();
    }
    layout(&mut f.app);
    assert!(find(f.app.root(), DIALOG, Point::new(0.0, 0.0)).is_some());
}

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

fn in_dialog<'a>(app: &'a App, id: &str) -> Option<(&'a dyn Widget, Rect)> {
    let (dialog, rect) = find(app.root(), DIALOG, Point::new(0.0, 0.0))?;
    let b = dialog.bounds();
    find(dialog, id, Point::new(rect.x - b.x, rect.y - b.y))
}

fn property(app: &App, key: &str) -> String {
    let (dialog, _) = find(app.root(), DIALOG, Point::new(0.0, 0.0)).expect("the dialog shows");
    dialog
        .properties()
        .into_iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
        .unwrap_or_default()
}

fn click_with(f: &mut Fixture, id: &str, clicks: u32, modifiers: Modifiers) {
    let (_, r) = in_dialog(&f.app, id).unwrap_or_else(|| panic!("no {id} in the dialog"));
    let at = Point::new(r.x + r.width / 2.0, HEIGHT - (r.y + r.height / 2.0));
    f.app.on_mouse_move(at.x, at.y);
    for n in 1..=clicks {
        f.app
            .on_mouse_down_clicks(at.x, at.y, MouseButton::Left, modifiers, n);
        f.app.on_mouse_up(at.x, at.y, MouseButton::Left, modifiers);
        layout(&mut f.app);
    }
}

fn click(f: &mut Fixture, id: &str, clicks: u32) {
    click_with(f, id, clicks, Modifiers::default());
}

/// The folder and file names the dialog answered with.
fn picked(f: &Fixture) -> (String, Vec<String>) {
    let pick = f.picked.borrow_mut().take().expect("answered");
    let pick = pick.expect("not cancelled");
    let folder = pick.folder.borrow().name().to_string();
    (
        folder,
        pick.items.iter().map(|i| i.name().to_string()).collect(),
    )
}

#[test]
fn the_dialog_lists_folders_and_the_requested_files_and_a_double_click_opens_one() {
    let mut f = fixture(None);
    assert!(f.dialogs.offers(DialogKind::PickOpenTarget));
    open(&mut f, false);
    assert!(
        in_dialog(&f.app, "Examples Row Item Collection").is_some(),
        "a read-only folder is offered"
    );
    click(&mut f, "Designs Row Item Collection", 2);
    assert!(in_dialog(&f.app, "Row Item a.stl").is_some());
    assert!(
        in_dialog(&f.app, "Row Item notes.txt").is_none(),
        "the filter leaves it out"
    );
    click(&mut f, "Row Item b.stl", 2);
    assert_eq!(picked(&f), ("Designs".into(), vec!["b.stl".into()]));
    assert!(
        find(f.app.root(), DIALOG, Point::new(0.0, 0.0)).is_none(),
        "closed"
    );
}

#[test]
fn open_needs_a_file_selected_and_opens_a_selected_folder() {
    let mut f = fixture(None);
    open(&mut f, false);
    assert_eq!(property(&f.app, "accept_enabled"), "false");
    assert_eq!(property(&f.app, "summary"), "Choose a file to open.");
    click(&mut f, "Examples Row Item Collection", 1);
    assert_eq!(property(&f.app, "accept_enabled"), "true");
    click(&mut f, "Browser Dialog Confirm Button", 1);
    assert_eq!(property(&f.app, "bread_crumb_trail"), "Top/Examples");
    assert!(
        f.picked.borrow().is_none(),
        "a folder opens, nothing answered"
    );
    click(&mut f, "Row Item cube.stl", 1);
    assert_eq!(property(&f.app, "summary"), "cube.stl");
    click(&mut f, "Browser Dialog Confirm Button", 1);
    assert_eq!(picked(&f), ("Examples".into(), vec!["cube.stl".into()]));
}

fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        ..Modifiers::default()
    }
}

#[test]
fn one_file_at_a_time_unless_the_request_allows_several() {
    let mut f = fixture(None);
    open(&mut f, false);
    click(&mut f, "Designs Row Item Collection", 2);
    click(&mut f, "Row Item b.stl", 1);
    click_with(&mut f, "Row Item a.stl", 1, ctrl());
    assert_eq!(property(&f.app, "summary"), "a.stl");
    click(&mut f, "Browser Dialog Cancel Button", 1);
    assert!(matches!(*f.picked.borrow(), Some(None)), "cancelled");

    open(&mut f, true);
    assert_eq!(property(&f.app, "summary"), "Choose files to open.");
    click(&mut f, "Designs Row Item Collection", 2);
    click(&mut f, "Row Item b.stl", 1);
    click_with(&mut f, "Row Item a.stl", 1, ctrl());
    assert_eq!(property(&f.app, "summary"), "b.stl, a.stl");
    click(&mut f, "Browser Dialog Confirm Button", 1);
    assert_eq!(
        picked(&f),
        ("Designs".into(), vec!["b.stl".into(), "a.stl".into()])
    );
}

#[test]
fn cancel_answers_nothing_and_the_dialog_opens_where_it_was_left() {
    let memory = Rc::new(Remembered::default());
    let mut f = fixture(Some(Rc::clone(&memory)));
    open(&mut f, false);
    click(&mut f, "Examples Row Item Collection", 2);
    click(&mut f, "Browser Dialog Cancel Button", 1);
    assert!(matches!(*f.picked.borrow(), Some(None)), "cancelled");
    assert_eq!(
        memory.saved.borrow().get("OpenLastBrowsedPath"),
        Some(&vec!["Examples".to_string()])
    );
    open(&mut f, false);
    assert_eq!(property(&f.app, "bread_crumb_trail"), "Top/Examples");
}
