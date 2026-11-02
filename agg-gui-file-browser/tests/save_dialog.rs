//! The Save dialog on `SheetDialogs` over folders in memory, through agg-gui's `App` with real
//! pointer and key events: a name typed and saved answers the folder and the file name (the
//! app's extension added); a name already there asks to replace it, and Cancel keeps the dialog
//! open; Create Folder makes a folder and opens it; read-only folders are not offered; and the
//! dialog opens where it was last left.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{App, Modifiers, MouseButton, Point, Rect, Size, Stack, Widget};
use agg_gui_file_browser::{
    append_extension, dialog_options, run_ready_local_tasks, BrowserConfig, BrowserDialogs,
    BrowserMemory, LocationFuture, OpenFolder, SavePick, SaveRequest, SheetDialogs,
};
use agg_gui_files::{DynamicFolder, DynamicFolderLink, Folder, MemoryFolder};

const WIDTH: f64 = 800.0;
const HEIGHT: f64 = 600.0;
const DIALOG: &str = "Save As Window";

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// The top: Designs (writable, holding report.txt) and Examples (read-only).
#[derive(Clone)]
struct Sample {
    designs: MemoryFolder,
    examples: MemoryFolder,
}

impl Sample {
    fn new() -> Self {
        let designs = MemoryFolder::new("Designs");
        designs.add_file("report.txt", b"r".to_vec());
        let examples = MemoryFolder::new("Examples");
        Self { designs, examples }
    }

    fn root(&self) -> Box<dyn Folder> {
        let (designs, examples) = (self.designs.clone(), self.examples.clone());
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
}

/// Remembers each key's path by folder names and finds it again from a new root.
struct Remembered {
    sample: Sample,
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
        let root = self.sample.root();
        Some(Box::pin(async move {
            let mut opened = Vec::new();
            let mut at = root;
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

struct Fixture {
    app: App,
    dialogs: SheetDialogs,
    picked: Rc<RefCell<Option<Option<SavePick>>>>,
}

fn fixture(sample: &Sample, memory: Option<Rc<Remembered>>) -> Fixture {
    agg_gui::set_device_scale(1.0);
    let root_sample = sample.clone();
    let mut dialogs = SheetDialogs::new()
        .with_browse_root(move || root_sample.root())
        .with_save_location_key("SaveAsLastBrowsedPath");
    if let Some(memory) = memory {
        dialogs = dialogs.with_browse_config(move || {
            BrowserConfig::builder()
                .memory(memory.clone())
                .options(dialog_options())
                .build()
        });
    }
    let root = Stack::new().add(Box::new(dialogs.sheet(font())));
    let mut app = App::new(Box::new(root));
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

/// Asks to save `name`, with ".txt" added.
fn open_save(f: &mut Fixture, name: &str) {
    let picked = Rc::clone(&f.picked);
    f.dialogs.pick_save_target(
        SaveRequest::new(name).with_namer(append_extension(".txt")),
        Box::new(move |pick| *picked.borrow_mut() = Some(pick)),
    );
    layout(&mut f.app);
    for _ in 0..20 {
        run_ready_local_tasks();
    }
    layout(&mut f.app);
    assert!(
        in_dialog(&f.app, "Save Name Field").is_some(),
        "the dialog shows"
    );
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

fn dialog_property(app: &App, key: &str) -> String {
    let (dialog, _) = find(app.root(), DIALOG, Point::new(0.0, 0.0)).expect("the dialog shows");
    dialog
        .properties()
        .into_iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
        .unwrap_or_default()
}

fn click(f: &mut Fixture, id: &str, clicks: u32) {
    let (_, r) = in_dialog(&f.app, id).unwrap_or_else(|| panic!("no {id} in the dialog"));
    let at = Point::new(r.x + r.width / 2.0, HEIGHT - (r.y + r.height / 2.0));
    f.app.on_mouse_move(at.x, at.y);
    for n in 1..=clicks {
        f.app
            .on_mouse_down_clicks(at.x, at.y, MouseButton::Left, Modifiers::default(), n);
        f.app
            .on_mouse_up(at.x, at.y, MouseButton::Left, Modifiers::default());
        layout(&mut f.app);
    }
}

fn type_into(f: &mut Fixture, id: &str, text: &str) {
    click(f, id, 1);
    f.app.on_text_input(text);
    layout(&mut f.app);
}

fn picked(f: &Fixture) -> (String, String, bool) {
    let pick = f.picked.borrow_mut().take().expect("answered");
    let pick = pick.expect("not cancelled");
    (
        pick.folder.name().to_string(),
        pick.file_name,
        pick.replaces,
    )
}

#[test]
fn a_typed_name_saves_into_the_folder_with_the_extension_added() {
    let sample = Sample::new();
    let mut f = fixture(&sample, None);
    open_save(&mut f, "");
    assert!(
        in_dialog(&f.app, "Examples Row Item Collection").is_none(),
        "read-only hidden"
    );
    assert_eq!(
        dialog_property(&f.app, "can_save"),
        "false",
        "the top takes no file"
    );
    click(&mut f, "Designs Row Item Collection", 2);
    assert_eq!(dialog_property(&f.app, "can_save"), "true");
    type_into(&mut f, "Save Name Field", "notes");
    click(&mut f, "Browser Dialog Confirm Button", 1);
    assert_eq!(picked(&f), ("Designs".into(), "notes.txt".into(), false));
    assert!(in_dialog(&f.app, "Save Name Field").is_none(), "closed");
}

#[test]
fn a_name_already_there_asks_to_replace_and_cancel_keeps_the_dialog() {
    let sample = Sample::new();
    let mut f = fixture(&sample, None);
    open_save(&mut f, "report");
    click(&mut f, "Designs Row Item Collection", 2);
    click(&mut f, "Browser Dialog Confirm Button", 1);
    assert_eq!(dialog_property(&f.app, "question"), "replace report.txt");
    let (note, _) = in_dialog(&f.app, "Save As Note").expect("the note");
    let text = note
        .properties()
        .into_iter()
        .find(|(k, _)| *k == "text")
        .map(|(_, v)| v)
        .unwrap_or_default();
    assert!(text.contains("\"report.txt\" already exists."), "{text}");
    click(&mut f, "Browser Dialog Cancel Button", 1);
    assert!(f.picked.borrow().is_none(), "not answered");
    assert_eq!(dialog_property(&f.app, "question"), "normal");
    assert!(in_dialog(&f.app, "Save Name Field").is_some(), "still open");
    click(&mut f, "Browser Dialog Confirm Button", 1);
    click(&mut f, "Browser Dialog Confirm Button", 1);
    assert_eq!(picked(&f), ("Designs".into(), "report.txt".into(), true));
}

#[test]
fn create_folder_makes_a_folder_and_opens_it() {
    let sample = Sample::new();
    let mut f = fixture(&sample, None);
    open_save(&mut f, "notes");
    click(&mut f, "Designs Row Item Collection", 2);
    click(&mut f, "Create Folder In Button", 1);
    type_into(&mut f, "Save As Folder Name Field", "Drafts");
    click(&mut f, "Browser Dialog Confirm Button", 1);
    let names: Vec<String> = sample
        .designs
        .child_containers()
        .iter()
        .map(|l| l.name().to_string())
        .collect();
    assert_eq!(names, ["Drafts"]);
    assert_eq!(
        dialog_property(&f.app, "bread_crumb_trail"),
        "Top/Designs/Drafts"
    );
    click(&mut f, "Browser Dialog Confirm Button", 1);
    assert_eq!(picked(&f), ("Drafts".into(), "notes.txt".into(), false));
}

#[test]
fn the_dialog_opens_where_it_was_left() {
    let sample = Sample::new();
    let memory = Rc::new(Remembered {
        sample: sample.clone(),
        saved: RefCell::default(),
    });
    let mut f = fixture(&sample, Some(Rc::clone(&memory)));
    open_save(&mut f, "notes");
    click(&mut f, "Designs Row Item Collection", 2);
    click(&mut f, "Browser Dialog Cancel Button", 1);
    assert!(matches!(*f.picked.borrow(), Some(None)), "cancelled");
    assert_eq!(
        memory.saved.borrow().get("SaveAsLastBrowsedPath"),
        Some(&vec!["Designs".to_string()])
    );
    open_save(&mut f, "notes");
    assert_eq!(dialog_property(&f.app, "bread_crumb_trail"), "Top/Designs");
}
