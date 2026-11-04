//! What the dialog controller tests (`dialog_controllers.rs`) drive: a sample folder tree
//! (`Sample`), an app's own dialog page (`AppPage`) and its Save As page over a
//! `SaveController` (`save_page`), the answers a Save hands back, and agg-gui's `App` laid out
//! and driven with real pointer and key events (`click`, `type_into`), the page's reported
//! properties read back (`property`).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{
    App, Button, DrawCtx, Event, EventResult, Modifiers, MouseButton, Point, Rect, Size, Stack,
    TextField, Widget,
};
use agg_gui_file_browser::{
    dialog_options, run_ready_local_tasks, BrowserConfig, BrowserPage, BrowserState, SaveAnswers,
    SaveController, SavePick,
};
use agg_gui_files::{DynamicFolder, DynamicFolderLink, Folder, MemoryFolder};

pub const WIDTH: f64 = 800.0;
pub const HEIGHT: f64 = 600.0;
pub const FIELD: &str = "Design Name Edit Field";
pub const ACCEPT: &str = "Accept Button";
pub const CANCEL: &str = "Cancel Button";
pub const CREATE: &str = "Create Folder Button";
pub const PAGE: &str = "App Dialog Page";

pub fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// The top: Designs (holding report.txt and gear.stl), Drafts and Cloud, all writable.
#[derive(Clone)]
pub struct Sample {
    pub designs: MemoryFolder,
    pub drafts: MemoryFolder,
    pub cloud: MemoryFolder,
}

impl Sample {
    pub fn new() -> Self {
        let designs = MemoryFolder::new("Designs");
        designs.add_file("report.txt", b"r".to_vec());
        designs.add_file("gear.stl", b"g".to_vec());
        Self {
            designs,
            drafts: MemoryFolder::new("Drafts"),
            cloud: MemoryFolder::new("Cloud"),
        }
    }

    pub fn root(&self) -> Box<dyn Folder> {
        let mut root = DynamicFolder::new("Top");
        root.data.child_containers = [&self.designs, &self.drafts, &self.cloud]
            .into_iter()
            .map(|folder| {
                let folder = folder.clone();
                let name = folder.name().to_string();
                Rc::new(DynamicFolderLink::new(
                    name,
                    Rc::new(move || Box::new(folder.clone()) as Box<dyn Folder>),
                )) as Rc<dyn agg_gui_files::FolderLink>
            })
            .collect();
        Box::new(root)
    }

    pub fn browser(&self) -> Rc<RefCell<BrowserState>> {
        agg_gui::set_device_scale(1.0);
        let config = BrowserConfig::builder().options(dialog_options()).build();
        BrowserState::new(self.root(), config)
    }
}

/// An app's own dialog page: its widgets laid out top to bottom (the browser page filling what
/// is left), the controller's `update` run before the browser lays out, and the properties the
/// app reports.
pub struct AppPage {
    pub update: Box<dyn Fn()>,
    pub report: Box<dyn Fn() -> Vec<(&'static str, String)>>,
    pub bounds: Rect,
    /// The browser page first, then the rest.
    pub children: Vec<Box<dyn Widget>>,
}

impl Widget for AppPage {
    fn type_name(&self) -> &'static str {
        "AppPage"
    }
    fn id(&self) -> Option<&str> {
        Some(PAGE)
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
        (self.update)();
        let mut top = available.height;
        for child in self.children.iter_mut().skip(1) {
            let size = child.layout(Size::new(available.width, 40.0));
            top -= 40.0;
            child.set_bounds(Rect::new(0.0, top, size.width.max(200.0), 40.0));
        }
        let room = Size::new(available.width, top.max(0.0));
        self.children[0].layout(room);
        self.children[0].set_bounds(Rect::new(0.0, 0.0, room.width, room.height));
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        (self.report)()
    }
}

/// The app's Save As page over `c`: its name field (focus id 77, its own hint), Save, Cancel
/// and Create Folder.
pub fn save_page(c: &SaveController) -> AppPage {
    let (editing, entering, accepting, enabled, cancelling, creating) = (
        c.clone(),
        c.clone(),
        c.clone(),
        c.clone(),
        c.clone(),
        c.clone(),
    );
    let mut field = TextField::new(font())
        .with_text_cell(c.name_cell())
        .on_change(move |_| editing.edited())
        .on_enter(move |_| entering.accept())
        .with_name(FIELD);
    if let Some(hint) = c.placeholder() {
        field = field.with_placeholder(hint.to_string());
    }
    if let Some(id) = c.request().focus_id {
        field = field.with_focus_id(id);
    }
    let accept = Button::new("Save", font())
        .on_click(move || accepting.accept())
        .with_enabled_fn(move || enabled.accept_enabled())
        .with_name(ACCEPT);
    let cancel = Button::new("Cancel", font())
        .on_click(move || cancelling.cancel())
        .with_name(CANCEL);
    let create = Button::new("Create Folder", font())
        .on_click(move || creating.create_folder())
        .with_name(CREATE);
    let (updating, reporting) = (c.clone(), c.clone());
    AppPage {
        update: Box::new(move || updating.update()),
        report: Box::new(move || {
            vec![
                ("design_name", reporting.name()),
                (
                    "design_name_hint",
                    reporting.placeholder().unwrap_or_default().to_string(),
                ),
                ("accept_enabled", reporting.accept_enabled().to_string()),
            ]
        }),
        bounds: Rect::default(),
        children: vec![
            Box::new(BrowserPage::new(c.state(), font())),
            Box::new(field),
            Box::new(accept),
            Box::new(cancel),
            Box::new(create),
        ],
    }
}

pub type Picked = Rc<RefCell<Option<Option<SavePick>>>>;

pub fn save_answers() -> (SaveAnswers, Picked) {
    let picked: Picked = Rc::new(RefCell::new(None));
    let (accepting, cancelling) = (Rc::clone(&picked), Rc::clone(&picked));
    let answers = SaveAnswers {
        accept: Rc::new(move |pick| *accepting.borrow_mut() = Some(Some(pick))),
        cancel: Rc::new(move || *cancelling.borrow_mut() = Some(None)),
    };
    (answers, picked)
}

pub fn app_over(page: AppPage) -> App {
    let mut app = App::new(Box::new(Stack::new().add(Box::new(page))));
    layout(&mut app);
    for _ in 0..20 {
        run_ready_local_tasks();
    }
    layout(&mut app);
    app
}

pub fn layout(app: &mut App) {
    app.layout(Size::new(WIDTH, HEIGHT));
    app.layout(Size::new(WIDTH, HEIGHT));
}

pub fn find<'a>(widget: &'a dyn Widget, id: &str, origin: Point) -> Option<(&'a dyn Widget, Rect)> {
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

pub fn property(app: &App, key: &str) -> String {
    let (page, _) = find(app.root(), PAGE, Point::new(0.0, 0.0)).expect("the page shows");
    page.properties()
        .into_iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
        .unwrap_or_default()
}

pub fn click(app: &mut App, id: &str, clicks: u32) {
    let (_, r) =
        find(app.root(), id, Point::new(0.0, 0.0)).unwrap_or_else(|| panic!("no {id} shows"));
    let at = Point::new(r.x + r.width / 2.0, HEIGHT - (r.y + r.height / 2.0));
    app.on_mouse_move(at.x, at.y);
    for n in 1..=clicks {
        app.on_mouse_down_clicks(at.x, at.y, MouseButton::Left, Modifiers::default(), n);
        app.on_mouse_up(at.x, at.y, MouseButton::Left, Modifiers::default());
        layout(app);
    }
}

pub fn type_into(app: &mut App, id: &str, text: &str) {
    click(app, id, 1);
    app.on_text_input(text);
    layout(app);
}

pub fn trail(c: &SaveController) -> String {
    c.state().borrow().path_names().join("/")
}

pub fn pick_of(picked: &Picked) -> (String, String, bool) {
    let pick = picked.borrow_mut().take().expect("answered");
    let pick = pick.expect("not cancelled");
    (
        pick.folder.name().to_string(),
        pick.file_name,
        pick.replaces,
    )
}
