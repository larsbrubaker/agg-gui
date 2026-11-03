//! The Save dialog on `SheetDialogs` over folders in memory, through agg-gui's `App` with real
//! pointer and key events: a name typed and saved answers the folder and the file name (the
//! app's extension added); a name already there asks to replace it, and Cancel keeps the dialog
//! open; Create Folder makes a folder and opens it; read-only folders are not offered; the
//! dialog opens where it was last left; a click on a file fills in its name and a double click
//! saves over it, while the name field keeps the keys (Backspace edits the name, never removes the
//! file); the name field has the keyboard as the dialog opens; the app's own question is asked
//! before a save; F5 lists a folder changed behind the dialog's back, whatever has the keyboard;
//! and a folder something still holds saves nothing and says so.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{App, Key, Modifiers, MouseButton, Point, Rect, Size, Stack, Widget};
use agg_gui_file_browser::{
    append_extension, dialog_options, run_ready_local_tasks, BrowserConfig, BrowserDialogs,
    BrowserMemory, LocationFuture, OpenFolder, ReplaceQuestion, SavePick, SaveRequest,
    SheetDialogs,
};
use agg_gui_files::{DynamicFolder, DynamicFolderLink, Folder, FolderLink, Item, MemoryFolder};

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
    let root_sample = sample.clone();
    let memory = memory.map(|m| m as Rc<dyn BrowserMemory>);
    fixture_over(move || root_sample.root(), memory)
}

/// The dialogs browsing the root `root` makes, remembering through `memory` when given.
fn fixture_over(
    root: impl Fn() -> Box<dyn Folder> + 'static,
    memory: Option<Rc<dyn BrowserMemory>>,
) -> Fixture {
    agg_gui::set_device_scale(1.0);
    let mut dialogs = SheetDialogs::new()
        .with_browse_root(root)
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
    open_request(
        f,
        SaveRequest::new(name).with_namer(append_extension(".txt")),
    );
}

/// Asks `request`.
fn open_request(f: &mut Fixture, request: SaveRequest) {
    let picked = Rc::clone(&f.picked);
    f.dialogs.pick_save_target(
        request,
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

/// Replaces the text of the field `id` with `text`, as a user does: a click in the field, select
/// all, then typing. The click alone selects nothing in a field that already has the keyboard (the
/// Save name field has it from the start), so typing after it would add to the name.
fn type_into(f: &mut Fixture, id: &str, text: &str) {
    click(f, id, 1);
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    f.app.on_key_down(Key::Char('a'), ctrl);
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

/// The text of the dialog's note.
fn note(f: &Fixture) -> String {
    let (note, _) = in_dialog(&f.app, "Save As Note").expect("the note");
    note.properties()
        .into_iter()
        .find(|(k, _)| *k == "text")
        .map(|(_, v)| v)
        .unwrap_or_default()
}

#[test]
fn a_click_on_a_file_fills_in_its_name_and_a_double_click_saves_over_it() {
    let sample = Sample::new();
    let mut f = fixture(&sample, None);
    open_request(&mut f, SaveRequest::new("notes").with_extension(".txt"));
    click(&mut f, "Designs Row Item Collection", 2);
    click(&mut f, "Row Item report.txt", 1);
    assert_eq!(
        dialog_property(&f.app, "name"),
        "report",
        "the name as the user reads it"
    );
    type_into(&mut f, "Save Name Field", "x");
    click(&mut f, "Row Item report.txt", 2);
    assert_eq!(dialog_property(&f.app, "question"), "replace report.txt");
    assert!(
        note(&f).contains("\"report\" already exists."),
        "{}",
        note(&f)
    );
    assert!(f.picked.borrow().is_none(), "asked first");
    click(&mut f, "Browser Dialog Confirm Button", 1);
    assert_eq!(picked(&f), ("Designs".into(), "report.txt".into(), true));
}

#[test]
fn backspace_after_a_click_on_a_file_edits_the_name_and_never_removes_the_file() {
    let sample = Sample::new();
    let mut f = fixture(&sample, None);
    open_request(&mut f, SaveRequest::new("notes").with_extension(".txt"));
    click(&mut f, "Designs Row Item Collection", 2);
    click(&mut f, "Row Item report.txt", 1);
    assert_eq!(dialog_property(&f.app, "name"), "report");
    for key in [Key::Backspace, Key::Delete] {
        f.app.on_key_down(key, Modifiers::default());
        layout(&mut f.app);
        for _ in 0..20 {
            run_ready_local_tasks();
        }
        layout(&mut f.app);
    }
    assert_eq!(dialog_property(&f.app, "question"), "normal");
    assert!(f.picked.borrow().is_none(), "the save is still being asked");
    assert!(
        in_dialog(&f.app, "Save Name Field").is_some(),
        "the Save dialog still shows"
    );
    assert_ne!(
        dialog_property(&f.app, "name"),
        "report",
        "the name field took the keys"
    );
    let mut designs = sample.designs.clone();
    designs.load();
    assert_eq!(designs.items().len(), 1, "report.txt stays");
}

#[test]
fn the_name_field_has_the_keyboard_as_the_dialog_opens_and_typing_replaces_the_name() {
    let sample = Sample::new();
    let mut f = fixture(&sample, None);
    open_save(&mut f, "notes");
    assert!(f.app.focused_is_text_input(), "the name field has the keys");
    f.app.on_text_input("plan");
    layout(&mut f.app);
    assert_eq!(dialog_property(&f.app, "name"), "plan");
}

#[test]
fn the_apps_question_is_asked_before_a_save_and_a_refusal_only_goes_back() {
    let sample = Sample::new();
    let mut f = fixture(&sample, None);
    let asked = Rc::new(RefCell::new(Vec::<String>::new()));
    let heard = Rc::clone(&asked);
    let request = SaveRequest::new("open")
        .with_extension(".txt")
        .with_replace_question(Rc::new(move |target| {
            heard.borrow_mut().push(format!(
                "{} {} {}",
                target.folder.name(),
                target.file_name,
                target.exists
            ));
            match target.file_name {
                "open.txt" => Some(ReplaceQuestion::new(
                    "Confirm Save As",
                    "\"open\" is open in another tab.",
                    "Replace",
                )),
                "saving.txt" => Some(
                    ReplaceQuestion::refusal("Confirm Save As", "\"saving\" is being saved.")
                        .with_cancel_label("OK"),
                ),
                _ => None,
            }
        }));
    open_request(&mut f, request);
    click(&mut f, "Designs Row Item Collection", 2);
    // A refusal offers only its going-back button.
    type_into(&mut f, "Save Name Field", "saving");
    click(&mut f, "Browser Dialog Confirm Button", 1);
    assert_eq!(note(&f), "Confirm Save As: \"saving\" is being saved.");
    let (back, _) = in_dialog(&f.app, "Browser Dialog Cancel Button").expect("going back");
    assert!(
        back.properties().iter().any(|(_, v)| v == "OK"),
        "{:?}",
        back.properties()
    );
    assert!(in_dialog(&f.app, "Browser Dialog Confirm Button").is_none());
    click(&mut f, "Browser Dialog Cancel Button", 1);
    assert_eq!(dialog_property(&f.app, "question"), "normal");
    // A question about a file not there yet saves once agreed.
    type_into(&mut f, "Save Name Field", "open");
    click(&mut f, "Browser Dialog Confirm Button", 1);
    assert_eq!(
        note(&f),
        "Confirm Save As: \"open\" is open in another tab."
    );
    click(&mut f, "Browser Dialog Confirm Button", 1);
    assert_eq!(picked(&f), ("Designs".into(), "open.txt".into(), false));
    assert_eq!(
        *asked.borrow(),
        ["Designs saving.txt false", "Designs open.txt false"]
    );
}

#[test]
fn typing_a_new_name_leaves_the_question() {
    let sample = Sample::new();
    let mut f = fixture(&sample, None);
    open_save(&mut f, "report");
    click(&mut f, "Designs Row Item Collection", 2);
    click(&mut f, "Browser Dialog Confirm Button", 1);
    assert_eq!(dialog_property(&f.app, "question"), "replace report.txt");
    click(&mut f, "Save Name Field", 1);
    f.app.on_text_input("s");
    layout(&mut f.app);
    assert_eq!(dialog_property(&f.app, "question"), "normal");
    assert!(f.picked.borrow().is_none(), "nothing saved");
}

/// A folder listing a snapshot of `source`, taken at each load, that says nothing when `source`
/// changes: a folder another program writes into.
struct Snapshot {
    source: MemoryFolder,
    items: Vec<Rc<dyn Item>>,
}

impl Folder for Snapshot {
    fn id(&self) -> &str {
        "shared"
    }
    fn name(&self) -> &str {
        "Shared"
    }
    fn is_protected(&self) -> bool {
        true
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        Vec::new()
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        self.items.clone()
    }
    fn load(&mut self) {
        self.items = self.source.items();
    }
}

/// The Save dialog open in "Shared", a folder that says nothing when "drawing.txt" is written into
/// it behind the dialog's back (as it now is), and "old.txt" already there.
fn shared_folder_changed_behind_the_dialog() -> Fixture {
    let source = MemoryFolder::new("Shared");
    source.add_file("old.txt", b"o".to_vec());
    let behind = source.clone();
    let mut f = fixture_over(
        move || {
            let source = behind.clone();
            let mut root = DynamicFolder::new("Top");
            root.data.child_containers = vec![Rc::new(DynamicFolderLink::new(
                "Shared",
                Rc::new(move || {
                    Box::new(Snapshot {
                        source: source.clone(),
                        items: Vec::new(),
                    }) as Box<dyn Folder>
                }),
            ))];
            Box::new(root)
        },
        None,
    );
    open_save(&mut f, "notes");
    click(&mut f, "Shared Row Item Collection", 2);
    source.add_file("drawing.txt", b"d".to_vec());
    layout(&mut f.app);
    assert!(
        in_dialog(&f.app, "Row Item drawing.txt").is_none(),
        "not said"
    );
    f
}

/// F5 lists "drawing.txt".
fn f5_lists_the_new_file(f: &mut Fixture) {
    f.app.on_key_down(Key::F(5), Modifiers::default());
    layout(&mut f.app);
    assert!(
        in_dialog(&f.app, "Row Item drawing.txt").is_some(),
        "listed"
    );
}

#[test]
fn f5_lists_a_folder_changed_behind_the_dialogs_back() {
    let mut f = shared_folder_changed_behind_the_dialog();
    assert!(f.app.focused_is_text_input(), "the name field has the keys");
    f5_lists_the_new_file(&mut f);
}

#[test]
fn f5_lists_the_folder_after_a_click_on_a_file() {
    let mut f = shared_folder_changed_behind_the_dialog();
    click(&mut f, "Row Item old.txt", 1);
    assert!(
        f.app.focused_is_text_input(),
        "the click left the keys with the name field"
    );
    f5_lists_the_new_file(&mut f);
}

#[test]
fn f5_lists_the_folder_with_nothing_focused() {
    let mut f = shared_folder_changed_behind_the_dialog();
    // A click on the dialog's blank margin leaves nothing with the keyboard.
    let (_, r) = in_dialog(&f.app, DIALOG).expect("the dialog");
    let at = Point::new(r.x + 2.0, HEIGHT - (r.y + r.height - 2.0));
    f.app
        .on_mouse_down_clicks(at.x, at.y, MouseButton::Left, Modifiers::default(), 1);
    f.app
        .on_mouse_up(at.x, at.y, MouseButton::Left, Modifiers::default());
    layout(&mut f.app);
    assert!(
        f.app.focused_widget_type_name().is_none(),
        "nothing focused"
    );
    f5_lists_the_new_file(&mut f);
}

/// Remembers nothing, but holds on to every folder it is handed: something else holding the
/// folder the dialog would hand over.
#[derive(Default)]
struct Holding {
    held: RefCell<Vec<OpenFolder>>,
}

impl BrowserMemory for Holding {
    fn remember_location(&self, _key: &str, path: &[OpenFolder]) {
        self.held.borrow_mut().extend(path.iter().cloned());
    }
}

#[test]
fn a_folder_something_still_holds_saves_nothing_and_says_so() {
    let sample = Sample::new();
    let root_sample = sample.clone();
    let holding = Rc::new(Holding::default());
    let memory = Rc::clone(&holding) as Rc<dyn BrowserMemory>;
    let mut f = fixture_over(move || root_sample.root(), Some(memory));
    open_save(&mut f, "notes");
    click(&mut f, "Designs Row Item Collection", 2);
    click(&mut f, "Browser Dialog Confirm Button", 1);
    assert_eq!(
        note(&f),
        "The folder was busy, so nothing was saved. Try again."
    );
    assert!(f.picked.borrow().is_none(), "nothing saved");
    assert_eq!(
        dialog_property(&f.app, "bread_crumb_trail"),
        "Top/Designs",
        "the dialog stays in the folder"
    );
    holding.held.borrow_mut().clear();
    click(&mut f, "Browser Dialog Confirm Button", 1);
    assert_eq!(picked(&f), ("Designs".into(), "notes.txt".into(), false));
}
