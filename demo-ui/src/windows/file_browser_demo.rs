//! The File Browser demo: agg-gui-file-browser's `FileBrowser` (folder tree, list, search, view
//! and sort, Undo and Redo, the "File" menu and the Move dialog on `SheetDialogs`) over a sample
//! folder that lives in memory, so nothing a visitor does reaches real files: Remove, Move and
//! Rename change only the sample. The sample has a writable place ("My Designs", with folders and
//! parts) and a read-only one ("Examples", whose parts can be copied out but nothing put in), and
//! each part shows a picture made from its name (a coloured tile with the name's first letter's
//! shape stood in by a disc), so the list's icon views have something to show. The tree's rows
//! are folders through `DefaultLook`'s icons. "Save As..." opens the Save dialog and writes a
//! generated note into the folder and name picked (a click on a note fills its name in); "Open..."
//! opens the Open dialog on the sample's parts (`.stl`, several at once) and names what was
//! opened beside the buttons. Work runs on the browser's default `LocalSpawner`. Not an egui
//! demo: egui has no file browser.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::icon_image::IconImage;
use agg_gui::{
    Button, DrawCtx, Event, EventResult, FlexColumn, FlexRow, Font, Label, Rect, Size, Stack,
    Widget,
};
use agg_gui_file_browser::{
    extension_filter, BrowserConfig, BrowserDialogs, BrowserState, Entry, FileBrowser,
    ItemPictures, OpenPick, OpenRequest, PictureCx, PictureState, SavePick, SaveRequest,
    SheetDialogs,
};
use agg_gui_files::{
    DynamicFolder, DynamicFolderLink, Folder, MemoryFolder, PlaceId, WritableFolder,
};

/// The demo window's title.
pub const TITLE: &str = "\u{F07C} File Browser";

/// The sample's two trees, shared by the browser and its Move dialog.
#[derive(Clone)]
struct Sample {
    designs: MemoryFolder,
    examples: MemoryFolder,
}

impl Sample {
    fn new() -> Self {
        let place = PlaceId::new("sample");
        let designs = MemoryFolder::new("My Designs").with_place(place.clone());
        if let Ok(brackets) = designs.add_folder("Brackets") {
            brackets.add_file("corner bracket.stl", b"solid corner".to_vec());
            brackets.add_file("shelf bracket.stl", b"solid shelf".to_vec());
        }
        if let Ok(gears) = designs.add_folder("Gears") {
            gears.add_file("spur gear.stl", b"solid spur".to_vec());
        }
        let _ = designs.add_folder("Projects");
        designs.add_file("phone stand.stl", b"solid stand".to_vec());
        designs.add_file("cable clip.stl", b"solid clip".to_vec());
        let examples = MemoryFolder::new("Examples").with_place(place);
        examples.add_file("calibration cube.stl", b"solid cube".to_vec());
        examples.add_file("vase.stl", b"solid vase".to_vec());
        examples.add_file("whistle.stl", b"solid whistle".to_vec());
        Self { designs, examples }
    }

    /// The top folder: My Designs (writable) and Examples (read-only), both drawn as folders.
    fn root(&self) -> Box<dyn Folder> {
        let mut root = DynamicFolder::new("Sample");
        let designs = self.designs.clone();
        let examples = self.examples.clone();
        let designs_link = DynamicFolderLink::new(
            "My Designs",
            Rc::new(move || Box::new(designs.clone()) as Box<dyn Folder>),
        )
        .with_micro_icon("folder");
        let examples_link = DynamicFolderLink::new(
            "Examples",
            Rc::new(move || Box::new(ReadOnly(examples.clone())) as Box<dyn Folder>),
        )
        .with_read_only(true)
        .with_micro_icon("folder");
        root.data.child_containers = vec![Rc::new(designs_link), Rc::new(examples_link)];
        root.data.is_protected = true;
        Box::new(root)
    }
}

/// A memory folder shown read-only: it lists, but takes and gives up nothing.
struct ReadOnly(MemoryFolder);

impl Folder for ReadOnly {
    fn id(&self) -> &str {
        self.0.id()
    }
    fn name(&self) -> &str {
        self.0.name()
    }
    fn is_protected(&self) -> bool {
        true
    }
    fn child_containers(&self) -> Vec<Rc<dyn agg_gui_files::FolderLink>> {
        self.0.child_containers()
    }
    fn items(&self) -> Vec<Rc<dyn agg_gui_files::Item>> {
        self.0.items()
    }
    fn place(&self) -> Option<PlaceId> {
        self.0.place()
    }
    fn location_key(&self) -> Option<String> {
        self.0.location_key()
    }
    fn load(&mut self) {
        self.0.load();
    }
}

/// Each part's picture: a tile in a colour picked from its name, with a lighter disc on it.
struct SamplePictures;

impl ItemPictures for SamplePictures {
    fn picture(&self, _cx: &PictureCx<'_>, entry: &Entry, side: i32) -> PictureState {
        if entry.is_folder() {
            return PictureState::Default;
        }
        match name_picture(entry.name(), side) {
            Some(image) => PictureState::Ready(image),
            None => PictureState::Default,
        }
    }
}

/// The picture of the part `name`, `side` units square.
fn name_picture(name: &str, side: i32) -> Option<IconImage> {
    let px = u32::try_from(side.clamp(8, 256)).ok()?;
    // A stable hue from the name's bytes.
    let hash = name.bytes().fold(2166136261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16777619)
    });
    let base = [
        (hash & 0x7f) as u8 + 64,
        ((hash >> 8) & 0x7f) as u8 + 64,
        ((hash >> 16) & 0x7f) as u8 + 64,
    ];
    let mut rgba = Vec::with_capacity((px * px * 4) as usize);
    let centre = f64::from(px) / 2.0;
    let radius = f64::from(px) * 0.3;
    for y in 0..px {
        for x in 0..px {
            let (dx, dy) = (f64::from(x) + 0.5 - centre, f64::from(y) + 0.5 - centre);
            let inside = dx * dx + dy * dy <= radius * radius;
            for channel in base {
                rgba.push(if inside {
                    channel.saturating_add(80)
                } else {
                    channel
                });
            }
            rgba.push(255);
        }
    }
    let logical = Size::new(f64::from(side), f64::from(side));
    IconImage::from_rgba(Arc::new(rgba), px, px, logical)
}

/// The demo's "Save As..." button's name.
pub const SAVE_AS_BUTTON: &str = "File Browser Demo Save As";
/// The demo's "Open..." button's name.
pub const OPEN_BUTTON: &str = "File Browser Demo Open";
/// The line naming what was opened.
pub const OPENED_LINE: &str = "File Browser Demo Opened";

/// What the Open dialog answered, as the demo's line says it.
fn opened_text(pick: &OpenPick) -> String {
    let names: Vec<&str> = pick.items.iter().map(|i| i.name()).collect();
    let folder = pick.folder.borrow().name().to_string();
    format!("Opened {} from {folder}", names.join(", "))
}

/// A label showing what its shared text says, built again when the text changes.
struct CellLabel {
    text: Rc<RefCell<String>>,
    shown: Option<String>,
    font: Arc<Font>,
    children: Vec<Box<dyn Widget>>,
    bounds: Rect,
}

impl CellLabel {
    fn new(text: Rc<RefCell<String>>, font: Arc<Font>) -> Self {
        Self {
            text,
            shown: None,
            font,
            children: Vec::new(),
            bounds: Rect::default(),
        }
    }
}

impl Widget for CellLabel {
    fn type_name(&self) -> &'static str {
        "CellLabel"
    }
    fn id(&self) -> Option<&str> {
        Some(OPENED_LINE)
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
        let text = self.text.borrow().clone();
        if self.shown.as_ref() != Some(&text) {
            let label = Label::new(text.clone(), Arc::clone(&self.font));
            self.children = vec![Box::new(label)];
            self.shown = Some(text);
        }
        let size = self.children[0].layout(available);
        self.children[0].set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
        size
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![("text", self.text.borrow().clone())]
    }
}

/// Writes a generated note into the folder the Save dialog picked (a memory folder), replacing a
/// file of that name when the user agreed.
fn save_note(pick: SavePick, count: u32) {
    let Some(folder) = pick
        .folder
        .as_any()
        .and_then(|a| a.downcast_ref::<MemoryFolder>())
        .cloned()
    else {
        return;
    };
    if pick.replaces {
        let old: Vec<_> = folder
            .items()
            .into_iter()
            .filter(|i| i.name() == pick.file_name)
            .collect();
        let mut writable = folder.clone();
        let _ = writable.remove(&old);
    }
    let text = format!("Note {count}, saved from the agg-gui File Browser demo.\n");
    folder.add_file(&pick.file_name, text.into_bytes());
}

/// The demo's content: a "Save As..." button over the browser on the sample, with the dialogs'
/// sheet over both.
pub fn file_browser_demo(font: Arc<Font>) -> Box<dyn Widget> {
    let sample = Sample::new();
    let move_sample = sample.clone();
    let dialogs = SheetDialogs::new().with_browse_root(move || move_sample.root());
    let config = BrowserConfig::builder()
        .dialogs(Rc::new(dialogs.clone()))
        .pictures(Rc::new(SamplePictures))
        .build();
    let state = BrowserState::new(sample.root(), config);
    let browser = FileBrowser::new(&state, Arc::clone(&font));
    let asking = dialogs.clone();
    let saves = Rc::new(std::cell::Cell::new(0u32));
    let save_as = Button::new("Save As...", Arc::clone(&font))
        .on_click(move || {
            let saves = Rc::clone(&saves);
            saves.set(saves.get() + 1);
            let request = SaveRequest::new(format!("note {}", saves.get())).with_extension(".txt");
            asking.pick_save_target(
                request,
                Box::new(move |pick| {
                    if let Some(pick) = pick {
                        save_note(pick, saves.get());
                    }
                }),
            );
        })
        .with_name(SAVE_AS_BUTTON);
    let opened = Rc::new(RefCell::new(String::new()));
    let (asking, showing) = (dialogs.clone(), Rc::clone(&opened));
    let open = Button::new("Open...", Arc::clone(&font))
        .on_click(move || {
            let showing = Rc::clone(&showing);
            let request = OpenRequest::new()
                .with_multiple(true)
                .with_filter(extension_filter(&[".stl"]));
            asking.pick_open_target(
                request,
                Box::new(move |pick| {
                    if let Some(pick) = pick {
                        *showing.borrow_mut() = opened_text(&pick);
                    }
                }),
            );
        })
        .with_name(OPEN_BUTTON);
    let buttons = FlexRow::new()
        .with_padding(4.0)
        .with_gap(8.0)
        .add(Box::new(save_as))
        .add(Box::new(open))
        .add(Box::new(CellLabel::new(opened, Arc::clone(&font))));
    let column = FlexColumn::new()
        .with_gap(4.0)
        .add(Box::new(buttons))
        .add_flex(Box::new(browser), 1.0);
    Box::new(
        Stack::new()
            .add(Box::new(column))
            .add(Box::new(dialogs.sheet(font))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use agg_gui::{App, Modifiers, MouseButton, Point, Rect};

    fn find<'a>(w: &'a dyn Widget, id: &str, origin: Point) -> Option<(&'a dyn Widget, Rect)> {
        let b = w.bounds();
        let here = Point::new(origin.x + b.x, origin.y + b.y);
        if w.id() == Some(id) {
            return Some((w, Rect::new(here.x, here.y, b.width, b.height)));
        }
        w.children().iter().find_map(|c| find(c.as_ref(), id, here))
    }

    fn property(app: &App, id: &str, key: &str) -> String {
        let (w, _) = find(app.root(), id, Point::new(0.0, 0.0)).expect("shows");
        w.properties()
            .into_iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v)
            .unwrap_or_default()
    }

    #[test]
    fn the_demo_lists_its_sample_with_folder_icons_and_opens_a_folder() {
        agg_gui::set_device_scale(1.0);
        let font = Arc::new(
            Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"),
        );
        let (w, h) = (720.0, 440.0);
        let mut app = App::new(file_browser_demo(font));
        app.layout(Size::new(w, h));
        app.layout(Size::new(w, h));
        for name in ["My Designs", "Examples"] {
            let row = format!("{name} Tree Node");
            assert_eq!(
                property(&app, &row, "has_icon"),
                "true",
                "{row} is a folder"
            );
            assert!(find(
                app.root(),
                &format!("{name} Row Item Collection"),
                Point::new(0.0, 0.0)
            )
            .is_some());
        }
        let (_, r) = find(app.root(), "My Designs Tree Node", Point::new(0.0, 0.0)).expect("row");
        let (x, y) = (r.x + r.width / 2.0, h - (r.y + r.height / 2.0));
        app.on_mouse_move(x, y);
        app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
        app.on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
        app.layout(Size::new(w, h));
        app.layout(Size::new(w, h));
        for id in [
            "Brackets Row Item Collection",
            "Row Item phone stand.stl",
            "Row Item cable clip.stl",
        ] {
            assert!(
                find(app.root(), id, Point::new(0.0, 0.0)).is_some(),
                "{id} listed"
            );
        }
        // Nested rows, whose links name no icon, are folders too.
        for name in ["Brackets", "Gears"] {
            let row = format!("{name} Tree Node");
            assert_eq!(
                property(&app, &row, "has_icon"),
                "true",
                "{row} is a folder"
            );
        }
    }

    #[test]
    fn save_as_writes_a_note_into_the_folder_picked() {
        agg_gui::set_device_scale(1.0);
        let font = Arc::new(
            Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"),
        );
        let (w, h) = (720.0, 440.0);
        let mut app = App::new(file_browser_demo(font));
        let click = |app: &mut App, id: &str, clicks: u32| {
            app.layout(Size::new(w, h));
            app.layout(Size::new(w, h));
            let found: Vec<Rect> = {
                let mut out = Vec::new();
                collect(app.root(), id, Point::new(0.0, 0.0), &mut out);
                out
            };
            let r = *found.last().unwrap_or_else(|| panic!("no {id}"));
            let (x, y) = (r.x + r.width / 2.0, h - (r.y + r.height / 2.0));
            app.on_mouse_move(x, y);
            for n in 1..=clicks {
                app.on_mouse_down_clicks(x, y, MouseButton::Left, Modifiers::default(), n);
                app.on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
                app.layout(Size::new(w, h));
            }
            app.layout(Size::new(w, h));
        };
        click(&mut app, SAVE_AS_BUTTON, 1);
        click(&mut app, "My Designs Row Item Collection", 2);
        click(&mut app, "Browser Dialog Confirm Button", 1);
        // The note lands in My Designs, which the browser behind lists once opened.
        click(&mut app, "My Designs Tree Node", 1);
        let mut out = Vec::new();
        collect(
            app.root(),
            "Row Item note 1.txt",
            Point::new(0.0, 0.0),
            &mut out,
        );
        assert!(!out.is_empty(), "the note is listed");
    }

    #[test]
    fn open_names_the_parts_picked() {
        agg_gui::set_device_scale(1.0);
        let font = Arc::new(
            Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"),
        );
        let (w, h) = (720.0, 440.0);
        let mut app = App::new(file_browser_demo(font));
        let click = |app: &mut App, id: &str, clicks: u32| {
            app.layout(Size::new(w, h));
            app.layout(Size::new(w, h));
            let mut found = Vec::new();
            collect(app.root(), id, Point::new(0.0, 0.0), &mut found);
            let r = *found.last().unwrap_or_else(|| panic!("no {id}"));
            let (x, y) = (r.x + r.width / 2.0, h - (r.y + r.height / 2.0));
            app.on_mouse_move(x, y);
            for n in 1..=clicks {
                app.on_mouse_down_clicks(x, y, MouseButton::Left, Modifiers::default(), n);
                app.on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
                app.layout(Size::new(w, h));
            }
            app.layout(Size::new(w, h));
        };
        click(&mut app, OPEN_BUTTON, 1);
        click(&mut app, "Examples Row Item Collection", 2);
        click(&mut app, "Row Item vase.stl", 2);
        assert_eq!(
            property(&app, OPENED_LINE, "text"),
            "Opened vase.stl from Examples"
        );
    }

    /// Every widget named `id`, in paint order (the dialog's last).
    fn collect(w: &dyn Widget, id: &str, origin: Point, out: &mut Vec<Rect>) {
        let b = w.bounds();
        let here = Point::new(origin.x + b.x, origin.y + b.y);
        if w.id() == Some(id) {
            out.push(Rect::new(here.x, here.y, b.width, b.height));
        }
        for c in w.children() {
            collect(c.as_ref(), id, here, out);
        }
    }
}
