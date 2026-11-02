//! The list views' finer parts: the default pictures keep a bounded number and make an evicted
//! one again; the app's overlay on a picture gets its own presses, the row's hover and its
//! tooltip; rows and tiles report where their names and pictures are; a drag that ends outside
//! the list gives the keys back; and a search's results list under headers that roll up.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{
    App, DrawCtx, Event, EventResult, Framebuffer, GfxCtx, Modifiers, MouseButton, Point, Rect,
    Size, Widget,
};
use agg_gui_file_browser::{
    run_ready_local_tasks, BrowserConfig, BrowserList, BrowserState, BrowserTile, Entry,
    ItemPictures, LocalSpawner, OwnPictures, PictureCx, PictureState, ViewMode,
};
use agg_gui_files::{
    Folder, Item, MemoryFolder, Picture, PictureFuture, SearchResultsFolder, Spawner, Thumbnail,
};

const WIDTH: f64 = 400.0;
const HEIGHT: f64 = 300.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// An item whose own picture is a solid red square, counting how often it is asked to make one.
struct Drawing {
    name: String,
    made: Rc<Cell<u32>>,
}

impl Item for Drawing {
    fn id(&self) -> &str {
        &self.name
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn is_protected(&self) -> bool {
        false
    }
    fn is_visible(&self) -> bool {
        true
    }
    fn as_thumbnail(&self) -> Option<&dyn Thumbnail> {
        Some(self)
    }
}

impl Thumbnail for Drawing {
    fn get_thumbnail(&self, _width: i32, _height: i32) -> PictureFuture {
        self.made.set(self.made.get() + 1);
        let red = [255u8, 0, 0, 255].repeat(16);
        Box::pin(async move { Ok(Picture::new(4, 4, red)) })
    }
}

fn drawing(name: &str, made: &Rc<Cell<u32>>) -> Entry {
    let item: Rc<dyn Item> = Rc::new(Drawing {
        name: name.to_string(),
        made: Rc::clone(made),
    });
    Entry::new(item, vec!["Drawings".to_string()])
}

fn is_ready(state: &PictureState) -> bool {
    matches!(state, PictureState::Ready(_))
}

#[test]
fn a_picture_evicted_from_the_kept_ones_is_made_again_when_asked_for() {
    agg_gui::set_device_scale(1.0);
    let pictures = OwnPictures::with_capacity(1);
    let spawner: Rc<dyn Spawner> = Rc::new(LocalSpawner::new(Rc::new(|m: &str| panic!("{m}"))));
    let arrived: Rc<dyn Fn()> = Rc::new(|| {});
    let cx = PictureCx::new(&spawner, &arrived);
    let (bolt_made, washer_made) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let (bolt, washer) = (drawing("bolt", &bolt_made), drawing("washer", &washer_made));
    assert!(!is_ready(&pictures.picture(&cx, &bolt, 50)), "on its way");
    run_ready_local_tasks();
    assert!(is_ready(&pictures.picture(&cx, &bolt, 50)));
    assert!(
        is_ready(&pictures.picture(&cx, &bolt, 50)),
        "kept, not made again"
    );
    assert_eq!(bolt_made.get(), 1);
    pictures.picture(&cx, &washer, 50);
    run_ready_local_tasks();
    assert!(is_ready(&pictures.picture(&cx, &washer, 50)));
    assert_eq!(
        pictures.kept(),
        1,
        "the bolt's picture left for the washer's"
    );
    assert!(!is_ready(&pictures.picture(&cx, &bolt, 50)), "made again");
    run_ready_local_tasks();
    assert!(
        is_ready(&pictures.picture(&cx, &bolt, 50)),
        "not left showing nothing"
    );
    assert_eq!(bolt_made.get(), 2);
}

/// What an overlay heard.
#[derive(Default)]
struct Heard {
    enters: Cell<u32>,
    presses: Cell<u32>,
    releases: Cell<u32>,
}

/// An overlay whose own shape is the picture's top-right 16-unit square.
struct Marker {
    heard: Rc<Heard>,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Marker {
    fn type_name(&self) -> &'static str {
        "Marker"
    }
    fn id(&self) -> Option<&str> {
        Some("Marker")
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
        available
    }
    fn hit_test(&self, p: Point) -> bool {
        let (w, h) = (self.bounds.width, self.bounds.height);
        p.x >= w - 16.0 && p.x <= w && p.y >= h - 16.0 && p.y <= h
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseEnter => self.heard.enters.set(self.heard.enters.get() + 1),
            Event::MouseDown { .. } => {
                self.heard.presses.set(self.heard.presses.get() + 1);
                return EventResult::Consumed;
            }
            Event::MouseUp { .. } => self.heard.releases.set(self.heard.releases.get() + 1),
            _ => {}
        }
        EventResult::Ignored
    }
    fn tooltip_text(&self) -> Option<&str> {
        Some("Marked")
    }
}

/// No pictures, a `Marker` over every item.
struct Marked {
    heard: Rc<Heard>,
}

impl ItemPictures for Marked {
    fn picture(&self, _cx: &PictureCx<'_>, _entry: &Entry, _side: i32) -> PictureState {
        PictureState::Default
    }
    fn tile_overlay(&self, entry: &Entry) -> Option<Box<dyn Widget>> {
        (!entry.is_folder()).then(|| {
            Box::new(Marker {
                heard: Rc::clone(&self.heard),
                bounds: Rect::default(),
                children: Vec::new(),
            }) as Box<dyn Widget>
        })
    }
}

fn home() -> MemoryFolder {
    let home = MemoryFolder::new("Home");
    home.add_folder("Notes").expect("Notes is free");
    home.add_file("gear b.stl", b"b".to_vec());
    home.add_file("gear a.stl", b"a".to_vec());
    home.add_file("bracket.stl", b"c".to_vec());
    home
}

fn list_app(
    root: Box<dyn Folder>,
    config: BrowserConfig,
    mode: ViewMode,
) -> (App, Rc<RefCell<BrowserState>>) {
    agg_gui::set_device_scale(1.0);
    let state = BrowserState::new(root, config);
    state.borrow_mut().set_view_mode(mode);
    let mut app = App::new(Box::new(BrowserList::new(&state, font())));
    app.layout(Size::new(WIDTH, HEIGHT));
    (app, state)
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

fn widget<'a>(app: &'a App, id: &str) -> (&'a dyn Widget, Rect) {
    find(app.root(), id, Point::new(0.0, 0.0)).unwrap_or_else(|| panic!("no widget named {id}"))
}

fn property(app: &App, id: &str, key: &str) -> String {
    widget(app, id)
        .0
        .properties()
        .into_iter()
        .find(|(k, _)| *k == key)
        .unwrap_or_else(|| panic!("{id} has no {key}"))
        .1
}

fn press(app: &mut App, at: Point) {
    app.on_mouse_move(at.x, HEIGHT - at.y);
    app.on_mouse_down(at.x, HEIGHT - at.y, MouseButton::Left, Modifiers::default());
}

fn release(app: &mut App, at: Point) {
    app.on_mouse_up(at.x, HEIGHT - at.y, MouseButton::Left, Modifiers::default());
    app.layout(Size::new(WIDTH, HEIGHT));
}

fn selected(state: &Rc<RefCell<BrowserState>>) -> Vec<String> {
    let s = state.borrow();
    s.selected_entries()
        .iter()
        .map(|e| e.name().to_string())
        .collect()
}

#[test]
fn the_overlay_takes_presses_on_its_own_shape_and_hears_the_tiles_hover() {
    let heard = Rc::new(Heard::default());
    let pictures = Rc::new(Marked {
        heard: Rc::clone(&heard),
    });
    let config = BrowserConfig::builder().pictures(pictures).build();
    let (mut app, state) = list_app(Box::new(home()), config, ViewMode::Icons);
    let (_, tile) = widget(&app, "Row Item gear a.stl");
    // The picture is the tile's top 100 inside its 1-unit border.
    let corner = Point::new(tile.x + 101.0 - 8.0, tile.y + tile.height - 1.0 - 8.0);
    app.on_mouse_move(corner.x, HEIGHT - corner.y);
    assert!(
        heard.enters.get() >= 1,
        "the tile's hover reaches its overlay"
    );
    let tooltip = widget(&app, "Row Item gear a.stl").0.tooltip_text();
    assert_eq!(
        tooltip,
        Some("Marked"),
        "over its shape, the overlay's tooltip"
    );
    press(&mut app, corner);
    release(&mut app, corner);
    assert_eq!((heard.presses.get(), heard.releases.get()), (1, 1));
    assert!(
        selected(&state).is_empty(),
        "a press the overlay took is not the tile's"
    );
    let middle = Point::new(tile.x + 30.0, tile.y + 40.0);
    press(&mut app, middle);
    release(&mut app, middle);
    assert_eq!(
        heard.presses.get(),
        1,
        "a press off its shape is the tile's"
    );
    assert_eq!(selected(&state), ["gear a.stl"]);
    // A tile rebuilt under the still pointer tells its new overlay it is hovered.
    let enters = heard.enters.get();
    state.borrow_mut().select_all();
    app.layout(Size::new(WIDTH, HEIGHT));
    assert!(heard.enters.get() > enters);
}

#[test]
fn rows_and_tiles_say_where_their_names_and_pictures_are() {
    let (app, _) = list_app(Box::new(home()), BrowserConfig::default(), ViewMode::Rows);
    let right: f64 = property(&app, "Row Item bracket.stl", "name_right")
        .parse()
        .expect("a number");
    assert!(right > 66.0 && right < 200.0, "after the picture: {right}");
    assert_eq!(property(&app, "Row Item bracket.stl", "thumbnail"), "item");
    assert_eq!(
        property(&app, "Notes Row Item Collection", "thumbnail"),
        "folder"
    );
    assert_eq!(
        property(&app, "Row Item bracket.stl", "thumbnail_center"),
        "",
        "a glyph"
    );
    let (app, _) = list_app(Box::new(home()), BrowserConfig::default(), ViewMode::Icons);
    let (tile, _) = widget(&app, "Row Item gear a.stl");
    let tile = tile
        .as_any()
        .and_then(|any| any.downcast_ref::<BrowserTile>())
        .expect("a tile");
    let band = tile.label_band().expect("a 100 tile names itself");
    assert_eq!((band.x, band.width), (1.0, 100.0));
    assert!(band.y >= 0.0 && band.y + band.height <= tile.bounds().height - 101.0);
    assert_eq!(tile.name_max_width(), 96.0);
    let (text, x, baseline) = tile.label_text_origin().expect("a name");
    assert_eq!(text, "gear a.stl");
    assert!(x > 1.0 && baseline >= band.y);
    assert!(tile.label_printed_height() > 0.0);
}

/// Pictures that are all a solid red square.
struct Red;

impl ItemPictures for Red {
    fn picture(&self, _cx: &PictureCx<'_>, _entry: &Entry, _side: i32) -> PictureState {
        let red = Arc::new([255u8, 0, 0, 255].repeat(4));
        let image = agg_gui::icon_image::IconImage::from_rgba(red, 2, 2, Size::new(2.0, 2.0));
        image.map_or(PictureState::Default, PictureState::Ready)
    }
}

#[test]
fn a_pictures_centre_is_reported_for_tests_of_its_colour() {
    let config = BrowserConfig::builder().pictures(Rc::new(Red)).build();
    let (app, _) = list_app(Box::new(home()), config, ViewMode::Rows);
    assert_eq!(
        property(&app, "Row Item bracket.stl", "thumbnail"),
        "picture"
    );
    assert_eq!(
        property(&app, "Row Item bracket.stl", "thumbnail_center"),
        "255,0,0,255"
    );
}

fn paint(app: &mut App) {
    let mut fb = Framebuffer::new(WIDTH as u32, HEIGHT as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

#[test]
fn a_drag_ending_outside_the_list_gives_the_keys_back_and_one_inside_keeps_them() {
    let (mut app, state) = list_app(Box::new(home()), BrowserConfig::default(), ViewMode::Rows);
    paint(&mut app);
    let (_, row) = widget(&app, "Row Item bracket.stl");
    let at = Point::new(row.x + 30.0, row.y + 25.0);
    press(&mut app, at);
    app.layout(Size::new(WIDTH, HEIGHT));
    assert_eq!(app.focused_widget_type_name(), Some("BrowserList"));
    let inside = Point::new(at.x + 100.0, at.y);
    app.on_mouse_move(inside.x, HEIGHT - inside.y);
    release(&mut app, inside);
    assert_eq!(app.focused_widget_type_name(), Some("BrowserList"));
    assert_eq!(selected(&state), ["bracket.stl"], "a drag never clicks");
    press(&mut app, at);
    let outside = Point::new(WIDTH + 200.0, at.y);
    app.on_mouse_move(outside.x, HEIGHT - outside.y);
    release(&mut app, outside);
    assert_eq!(
        app.focused_widget_type_name(),
        None,
        "the keys are given back"
    );
}

/// `root`'s search for `query`, run to its end.
fn finished_search(root: &dyn Folder, query: &str) -> Rc<SearchResultsFolder> {
    let spawner = Rc::new(LocalSpawner::new(Rc::new(|m: &str| panic!("fault: {m}"))));
    let mut results = SearchResultsFolder::new(root, query, spawner);
    results.start_search();
    for _ in 0..1000 {
        if results.is_finished() {
            break;
        }
        run_ready_local_tasks();
    }
    assert!(results.is_finished(), "the search ends");
    Rc::new(results)
}

fn kinds(widget: &dyn Widget, out: &mut Vec<String>) {
    if matches!(
        widget.type_name(),
        "BrowserRow" | "BrowserTile" | "SectionHeaderRow"
    ) {
        out.push(widget.id().unwrap_or_default().to_string());
    }
    for child in widget.children() {
        kinds(child.as_ref(), out);
    }
}

#[test]
fn a_searchs_results_list_under_a_header_that_rolls_its_section_up() {
    for mode in [ViewMode::Rows, ViewMode::Icons] {
        let root = home();
        let (mut app, state) = list_app(Box::new(root.clone()), BrowserConfig::default(), mode);
        state
            .borrow_mut()
            .push_search_results(finished_search(&root, "gear"));
        app.layout(Size::new(WIDTH, HEIGHT));
        let section = state
            .borrow()
            .list_slots()
            .into_iter()
            .find_map(|slot| match slot {
                agg_gui_file_browser::ListSlot::Section(h) => Some(h.name),
                _ => None,
            })
            .expect("a section");
        let header = format!("Search Section {section}");
        let mut shown = Vec::new();
        kinds(app.root(), &mut shown);
        assert_eq!(
            shown,
            [
                header.clone(),
                "Row Item gear a.stl".into(),
                "Row Item gear b.stl".into()
            ]
        );
        let (_, rect) = widget(&app, &header);
        assert_eq!(rect.width, WIDTH, "across the whole list");
        assert_eq!(
            property(&app, &format!("{header} Toggle"), "checked"),
            "true"
        );
        let middle = Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        press(&mut app, middle);
        release(&mut app, middle);
        let mut shown = Vec::new();
        kinds(app.root(), &mut shown);
        assert_eq!(shown, std::slice::from_ref(&header), "rolled up");
        assert_eq!(
            property(&app, &format!("{header} Toggle"), "checked"),
            "false"
        );
        // Open again for the next view (the rolled-up sections are the session's).
        let (_, rect) = widget(&app, &header);
        let middle = Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        press(&mut app, middle);
        release(&mut app, middle);
    }
}
