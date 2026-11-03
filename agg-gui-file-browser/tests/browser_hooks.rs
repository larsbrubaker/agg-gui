//! The hooks through which an app's own browser becomes a `FileBrowser` and its dialogs'
//! `BrowserPage`: a child kept across rebuilds and the painted rectangle, the look's list banner,
//! header markdown widget, view mode names and page bar, and the pictures' turn before each
//! layout. Driven through agg-gui's `App`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::widgets::Label;
use agg_gui::{
    App, DrawCtx, Event, EventResult, Framebuffer, GfxCtx, Insets, Point, Rect, Size, SizedBox,
    Widget,
};
use agg_gui_file_browser::{
    BrowserConfig, BrowserLook, BrowserNames, BrowserPage, BrowserState, BrowserStyle, DefaultLook,
    Entry, FileBrowser, ItemPictures, PictureCx, PictureState, ViewMode,
};
use agg_gui_files::{DynamicFolder, DynamicFolderLink, Folder, FolderLink, MemoryFolder};

const WIDTH: f64 = 700.0;
const HEIGHT: f64 = 400.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// Home (its header markdown "Home's words") holding Projects and gear.stl.
fn home() -> DynamicFolder {
    let disk = MemoryFolder::new("Disk");
    let projects = disk.add_folder("Projects").expect("free");
    disk.add_file("gear.stl", b"g".to_vec());
    let mut home = DynamicFolder::new("Home").with_header_markdown("Home's words");
    home.data.child_containers = vec![Rc::new(DynamicFolderLink::new(
        "Projects",
        Rc::new(move || Box::new(projects.clone()) as Box<dyn Folder>),
    ))];
    home.data.items = disk.items();
    home
}

/// A look drawing the banner, header and page bar its app's way, counting banner asks.
struct AppLook {
    base: DefaultLook,
    banners: Rc<RefCell<Vec<bool>>>,
}

impl BrowserLook for AppLook {
    fn style(&self) -> BrowserStyle {
        self.base.style()
    }
    fn names(&self) -> &BrowserNames {
        self.base.names()
    }
    fn list_banner(
        &self,
        _state: &Rc<RefCell<BrowserState>>,
        shown_anew: bool,
        font: &Arc<Font>,
    ) -> Option<Box<dyn Widget>> {
        self.banners.borrow_mut().push(shown_anew);
        Some(Box::new(Named(
            "App Banner",
            Label::new("Waiting", Arc::clone(font)),
        )))
    }
    fn header_markdown(&self, text: &str, font: &Arc<Font>) -> Option<Box<dyn Widget>> {
        let label = Label::new(text, Arc::clone(font));
        Some(Box::new(Named("App Markdown", label)))
    }
    fn view_mode_name(&self, mode: ViewMode) -> String {
        format!("app-{}", mode.number())
    }
    fn page_nav_bar_padding(&self) -> Insets {
        Insets::all(2.0)
    }
    fn page_nav_bar_rule(&self) -> bool {
        false
    }
}

/// `widget` carrying the id `name`.
struct Named<W: Widget>(&'static str, W);

impl<W: Widget> Widget for Named<W> {
    fn type_name(&self) -> &'static str {
        "Named"
    }
    fn id(&self) -> Option<&str> {
        Some(self.0)
    }
    fn bounds(&self) -> Rect {
        self.1.bounds()
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.1.set_bounds(bounds);
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        self.1.children()
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        self.1.children_mut()
    }
    fn layout(&mut self, available: Size) -> Size {
        self.1.layout(available)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        self.1.paint(ctx);
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        self.1.on_event(event)
    }
}

/// Pictures counting their turns before each layout.
#[derive(Default)]
struct CountingPictures(Cell<u32>);

impl ItemPictures for CountingPictures {
    fn picture(&self, _cx: &PictureCx<'_>, _entry: &Entry, _side: i32) -> PictureState {
        PictureState::Default
    }
    fn before_layout(&self, state: &Rc<RefCell<BrowserState>>) {
        // The state is lent unborrowed: the hook may change it.
        state.borrow_mut().sync_content();
        self.0.set(self.0.get() + 1);
    }
}

fn app_look() -> (Rc<AppLook>, Rc<RefCell<Vec<bool>>>) {
    let banners = Rc::new(RefCell::new(Vec::new()));
    let look = Rc::new(AppLook {
        base: DefaultLook::default(),
        banners: Rc::clone(&banners),
    });
    (look, banners)
}

fn state_with(config: BrowserConfig) -> Rc<RefCell<BrowserState>> {
    agg_gui::set_device_scale(1.0);
    let state = BrowserState::new(Box::new(home()), config);
    state.borrow_mut().set_view_mode(ViewMode::Rows);
    state
}

fn frame(app: &mut App) {
    app.layout(Size::new(WIDTH, HEIGHT));
    let mut fb = Framebuffer::new(WIDTH as u32, HEIGHT as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
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

fn has_type(widget: &dyn Widget, type_name: &str) -> bool {
    widget.type_name() == type_name
        || widget
            .children()
            .iter()
            .any(|c| has_type(c.as_ref(), type_name))
}

fn rect(app: &App, id: &str) -> Rect {
    find(app.root(), id, Point::default())
        .unwrap_or_else(|| panic!("no widget named {id}"))
        .1
}

fn property(app: &App, key: &str) -> String {
    app.root()
        .properties()
        .into_iter()
        .find(|(k, _)| *k == key)
        .unwrap_or_else(|| panic!("no property {key}"))
        .1
}

fn open(state: &Rc<RefCell<BrowserState>>, name: &str) {
    let link = state
        .borrow()
        .active_container()
        .and_then(|c| c.child_containers().into_iter().find(|l| l.name() == name))
        .unwrap_or_else(|| panic!("{name} is listed"));
    state.borrow_mut().open(link.as_ref() as &dyn FolderLink);
}

/// A browser placed 50 right and 30 up inside its window.
struct Inset(Vec<Box<dyn Widget>>, Rect);

impl Widget for Inset {
    fn type_name(&self) -> &'static str {
        "Inset"
    }
    fn bounds(&self) -> Rect {
        self.1
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.1 = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.0
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.0
    }
    fn layout(&mut self, available: Size) -> Size {
        let room = Size::new(available.width - 50.0, available.height - 30.0);
        self.0[0].layout(room);
        self.0[0].set_bounds(Rect::new(50.0, 30.0, room.width, room.height));
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

#[test]
fn a_kept_child_outlives_rebuilds_at_zero_size_and_the_painted_rect_is_the_windows() {
    let state = state_with(BrowserConfig::default());
    let painted = Rc::new(Cell::new(Rect::default()));
    let host = Named("Menu Host", SizedBox::fixed(40.0, 40.0));
    let browser = FileBrowser::new(&state, font())
        .with_overlay_child(Box::new(host))
        .with_painted_rect(Rc::clone(&painted));
    let mut app = App::new(Box::new(Inset(vec![Box::new(browser)], Rect::default())));
    frame(&mut app);
    assert_eq!(rect(&app, "Menu Host"), Rect::new(50.0, 30.0, 0.0, 0.0));
    assert_eq!(painted.get(), Rect::new(50.0, 30.0, 650.0, 370.0));
    open(&state, "Projects");
    frame(&mut app);
    assert!(find(app.root(), "Menu Host", Point::default()).is_some());
    let children = &app.root().children()[0].children();
    assert_eq!(children.last().and_then(|c| c.id()), Some("Menu Host"));
}

#[test]
fn the_looks_banner_and_markdown_stand_over_the_list_and_its_view_names_are_reported() {
    let (look, banners) = app_look();
    let config = BrowserConfig::builder().look(look).build();
    let state = state_with(config);
    let mut app = App::new(Box::new(FileBrowser::new(&state, font())));
    frame(&mut app);
    let (header, banner) = (rect(&app, "App Markdown"), rect(&app, "App Banner"));
    let list = rect(
        &app,
        &state.borrow().config().look.names().list_area.clone(),
    );
    assert!(
        header.y >= banner.y + banner.height - 0.5,
        "{header:?} {banner:?}"
    );
    assert!(
        banner.y >= list.y + list.height - 0.5,
        "{banner:?} {list:?}"
    );
    assert_eq!(*banners.borrow(), [true]);
    assert_eq!(property(&app, "view_mode"), "app-0");
    open(&state, "Projects");
    frame(&mut app);
    assert!(find(app.root(), "App Markdown", Point::default()).is_none());
    assert_eq!(
        *banners.borrow(),
        [true, true],
        "asked again for the new folder"
    );
}

#[test]
fn the_page_bar_takes_the_looks_padding_and_rule() {
    let (look, _) = app_look();
    let config = BrowserConfig::builder().look(look).build();
    let state = state_with(config);
    let name = state.borrow().config().look.names().page_nav_bar.clone();
    let mut app = App::new(Box::new(BrowserPage::new(&state, font())));
    frame(&mut app);
    assert!(
        find(app.root(), &name, Point::default()).is_some(),
        "the bar keeps its name"
    );
    assert!(!has_type(app.root(), "OverflowBar"), "no rule");
    let default_state = state_with(BrowserConfig::default());
    let mut default_app = App::new(Box::new(BrowserPage::new(&default_state, font())));
    frame(&mut default_app);
    assert!(
        has_type(default_app.root(), "OverflowBar"),
        "the default draws its rule"
    );
    // Padding 2 top and bottom against 1 top and bottom plus the rule's pixel: the app's bar is
    // one unit taller than the default's.
    let tall = |app: &App| rect(app, &name).height;
    assert_eq!(tall(&app), tall(&default_app) + 1.0);
}

#[test]
fn the_pictures_get_their_turn_before_each_layout() {
    let pictures = Rc::new(CountingPictures::default());
    let config = BrowserConfig::builder().pictures(pictures.clone()).build();
    let state = state_with(config);
    let mut app = App::new(Box::new(FileBrowser::new(&state, font())));
    frame(&mut app);
    let first = pictures.0.get();
    assert!(first >= 1);
    frame(&mut app);
    assert!(pictures.0.get() > first);
}

/// The trait's own defaults hold: a page bar with the default look's padding and rule.
#[test]
fn the_default_look_keeps_the_page_bar_as_it_was() {
    let look = DefaultLook::default();
    assert_eq!(look.page_nav_bar_padding(), Insets::symmetric(3.0, 1.0));
    assert!(look.page_nav_bar_rule());
    assert_eq!(look.view_mode_name(ViewMode::Icons70), "Icons70");
    assert!(look.header_markdown("x", &font()).is_none());
}
