//! The list views (`BrowserList`) over a `MemoryFolder`, driven through agg-gui's `App` with real
//! pointer events: rows and tiles carry their names and sizes, clicks select (Shift ranges,
//! Control toggles), the keys and a rubber band move and widen the selection, a double click
//! activates through the app's hook or opens a folder, the menu button and a right press open the
//! app's menu, a picture arriving later redraws its tile, and a folder listing only a message
//! shows it in place of its rows.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::icon_image::IconImage;
use agg_gui::text::Font;
use agg_gui::{App, Framebuffer, GfxCtx, Modifiers, MouseButton, Point, Rect, Size, Widget};
use agg_gui_file_browser::{
    Activation, BrowserActions, BrowserConfig, BrowserList, BrowserRef, BrowserState, Entry,
    ItemPictures, PictureCx, PictureState, SheetDialogs, SignInOffer, ViewMode,
};
use agg_gui_files::{Folder, FolderLink, Item, ListingFailure, MemoryFolder, MessageItem};

const WIDTH: f64 = 400.0;
const HEIGHT: f64 = 300.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// Home holding the folders Projects (holding Brackets) and Notes, and the files b.txt, a.txt,
/// c.txt.
fn home() -> MemoryFolder {
    let home = MemoryFolder::new("Home");
    let projects = home.add_folder("Projects").expect("Projects is free");
    projects.add_folder("Brackets").expect("Brackets is free");
    home.add_folder("Notes").expect("Notes is free");
    home.add_file("b.txt", b"b".to_vec());
    home.add_file("a.txt", b"a".to_vec());
    home.add_file("c.txt", b"c".to_vec());
    home
}

/// An app showing the list of a browser at `root` with `config`, in `mode`, laid out.
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

fn rect_of(app: &App, id: &str) -> Rect {
    find(app.root(), id, Point::new(0.0, 0.0))
        .unwrap_or_else(|| panic!("no widget named {id}"))
        .1
}

fn property(app: &App, id: &str, key: &str) -> String {
    let (widget, _) =
        find(app.root(), id, Point::new(0.0, 0.0)).unwrap_or_else(|| panic!("no {id}"));
    widget
        .properties()
        .into_iter()
        .find(|(k, _)| *k == key)
        .unwrap_or_else(|| panic!("{id} has no {key}"))
        .1
}

/// The names of the rows or tiles, in widget order.
fn shown(widget: &dyn Widget, kind: &str, out: &mut Vec<String>) {
    if widget.type_name() == kind {
        out.push(widget.id().unwrap_or_default().to_string());
    }
    for child in widget.children() {
        shown(child.as_ref(), kind, out);
    }
}

fn shown_of(app: &App, kind: &str) -> Vec<String> {
    let mut out = Vec::new();
    shown(app.root(), kind, &mut out);
    out
}

fn list(app: &App) -> &BrowserList {
    app.root()
        .as_any()
        .and_then(|any| any.downcast_ref::<BrowserList>())
        .expect("the root is the list")
}

/// A press and release at `at` (root coordinates, Y up) with `mods`, the press the `clicks`th of
/// a multiple click, then a layout, as a frame would.
fn click_at(app: &mut App, at: Point, button: MouseButton, mods: Modifiers, clicks: u32) {
    let y = HEIGHT - at.y;
    app.on_mouse_move(at.x, y);
    app.on_mouse_down_clicks(at.x, y, button, mods, clicks);
    app.on_mouse_up(at.x, y, button, mods);
    app.layout(Size::new(WIDTH, HEIGHT));
}

fn centre(r: Rect) -> Point {
    Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0)
}

fn click_row(app: &mut App, id: &str, mods: Modifiers) {
    let at = centre(rect_of(app, id));
    click_at(app, at, MouseButton::Left, mods, 1);
}

fn selected(state: &Rc<RefCell<BrowserState>>) -> Vec<String> {
    let mut names: Vec<String> = state
        .borrow()
        .selected_entries()
        .iter()
        .map(|e| e.name().to_string())
        .collect();
    names.sort();
    names
}

#[test]
fn rows_list_folders_then_items_by_name_each_a_full_width_row() {
    let (app, _) = list_app(Box::new(home()), BrowserConfig::default(), ViewMode::Rows);
    assert_eq!(
        shown_of(&app, "BrowserRow"),
        [
            "Notes Row Item Collection",
            "Projects Row Item Collection",
            "Row Item a.txt",
            "Row Item b.txt",
            "Row Item c.txt"
        ]
    );
    let first = rect_of(&app, "Notes Row Item Collection");
    let second = rect_of(&app, "Projects Row Item Collection");
    assert_eq!((first.width, first.height), (WIDTH, 56.0));
    assert_eq!(first.y - second.y, 56.0, "each row below the one before");
    assert_eq!(first.y + first.height, HEIGHT, "the first row at the top");
    assert_eq!(property(&app, "Row Item a.txt", "name"), "a.txt");
    assert_eq!(
        property(&app, "Notes Row Item Collection", "picture"),
        "folder"
    );
    assert_eq!(property(&app, "Row Item a.txt", "picture"), "item");
}

#[test]
fn tiles_flow_folders_on_a_row_of_their_own_then_the_items() {
    let (app, _) = list_app(
        Box::new(home()),
        BrowserConfig::default(),
        ViewMode::Icons70,
    );
    assert_eq!(
        shown_of(&app, "BrowserTile"),
        [
            "Notes Row Item Collection",
            "Projects Row Item Collection",
            "Row Item a.txt",
            "Row Item b.txt",
            "Row Item c.txt"
        ]
    );
    let notes = rect_of(&app, "Notes Row Item Collection");
    let projects = rect_of(&app, "Projects Row Item Collection");
    let a = rect_of(&app, "Row Item a.txt");
    let b = rect_of(&app, "Row Item b.txt");
    assert_eq!(notes.width, 72.0, "a 70 picture and its border");
    assert_eq!(notes.y, projects.y, "the folders share a row");
    assert!(a.y < notes.y, "the items start a row below the folders");
    assert_eq!(a.y, b.y);
    assert!(a.x < b.x);
    assert_eq!(
        property(&app, "Row Item a.txt", "shown_name"),
        "",
        "70 is too small for a name"
    );
    let (app, _) = list_app(Box::new(home()), BrowserConfig::default(), ViewMode::Icons);
    assert_eq!(property(&app, "Row Item a.txt", "shown_name"), "a.txt");
    let tile = rect_of(&app, "Row Item a.txt");
    assert_eq!(tile.width, 102.0);
    assert!(tile.height > 102.0, "the name's line under the picture");
}

#[test]
fn clicks_select_one_row_shift_a_range_and_control_toggles() {
    let (mut app, state) = list_app(Box::new(home()), BrowserConfig::default(), ViewMode::Rows);
    click_row(&mut app, "Row Item a.txt", Modifiers::default());
    assert_eq!(selected(&state), ["a.txt"]);
    assert_eq!(property(&app, "Row Item a.txt", "selected"), "true");
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    click_row(&mut app, "Row Item c.txt", shift);
    assert_eq!(selected(&state), ["a.txt", "b.txt", "c.txt"]);
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    click_row(&mut app, "Row Item b.txt", ctrl);
    assert_eq!(selected(&state), ["a.txt", "c.txt"]);
    assert_eq!(property(&app, "Row Item b.txt", "selected"), "false");
    click_row(&mut app, "Notes Row Item Collection", ctrl);
    assert_eq!(selected(&state), ["Notes", "a.txt", "c.txt"]);
    // A plain click on empty space below the rows empties the selection.
    click_at(
        &mut app,
        Point::new(200.0, 10.0),
        MouseButton::Left,
        Modifiers::default(),
        1,
    );
    assert!(selected(&state).is_empty());
}

/// Actions recording each activation by name.
#[derive(Default)]
struct Recording {
    activated: RefCell<Vec<String>>,
}

impl BrowserActions for Recording {
    fn activate(&self, _state: &BrowserRef, entry: &Entry, how: Activation) -> bool {
        assert_eq!(how, Activation::DoubleClick);
        self.activated.borrow_mut().push(entry.name().to_string());
        true
    }
}

#[test]
fn a_double_click_activates_an_item_through_the_hook_and_opens_a_folder() {
    let actions = Rc::new(Recording::default());
    let config = BrowserConfig::builder().actions(actions.clone()).build();
    let (mut app, state) = list_app(Box::new(home()), config, ViewMode::Rows);
    let at = centre(rect_of(&app, "Row Item b.txt"));
    click_at(&mut app, at, MouseButton::Left, Modifiers::default(), 1);
    assert!(
        actions.activated.borrow().is_empty(),
        "one click only selects"
    );
    click_at(&mut app, at, MouseButton::Left, Modifiers::default(), 2);
    assert_eq!(*actions.activated.borrow(), ["b.txt"]);
    let folder = centre(rect_of(&app, "Projects Row Item Collection"));
    click_at(&mut app, folder, MouseButton::Left, Modifiers::default(), 1);
    click_at(&mut app, folder, MouseButton::Left, Modifiers::default(), 2);
    assert_eq!(state.borrow().path_names(), ["Home", "Projects"]);
    assert_eq!(
        shown_of(&app, "BrowserRow"),
        ["Brackets Row Item Collection"]
    );
    assert_eq!(
        *actions.activated.borrow(),
        ["b.txt"],
        "a folder is not the app's"
    );
}

#[test]
fn the_menu_button_opens_the_selections_menu_and_empty_space_the_folders() {
    // Dialogs that confirm and ask names, so Rename, Remove and Create Folder... can run.
    let config = BrowserConfig::builder()
        .dialogs(Rc::new(SheetDialogs::new()))
        .build();
    let (mut app, state) = list_app(Box::new(home()), config, ViewMode::Rows);
    let row = rect_of(&app, "Row Item a.txt");
    // The row's box is inset 6 each side; the button is centred on the glyph at its right end.
    let button = Point::new(row.x + row.width - 6.0 - 16.0, row.y + 50.0 - 3.0 - 16.0);
    click_at(&mut app, button, MouseButton::Left, Modifiers::default(), 1);
    assert_eq!(selected(&state), ["a.txt"]);
    assert!(list(&app).menu_is_open());
    // The selection's menu holds what can run: no Move dialog and no way to locate an item
    // here, so Move and Open File Location are left out.
    let titles: Vec<(String, bool)> = list(&app).menu_rows();
    assert_eq!(
        titles,
        [("Rename".to_string(), true), ("Remove".to_string(), true),]
    );
    // A press outside the menu closes it.
    click_at(
        &mut app,
        Point::new(5.0, 5.0),
        MouseButton::Left,
        Modifiers::default(),
        1,
    );
    assert!(!list(&app).menu_is_open());
    click_at(
        &mut app,
        Point::new(200.0, 10.0),
        MouseButton::Right,
        Modifiers::default(),
        1,
    );
    assert_eq!(
        list(&app).menu_rows(),
        [
            ("Add".to_string(), false),
            ("Create Folder...".to_string(), true)
        ]
    );
}

#[test]
fn a_right_press_on_a_row_selects_it_and_opens_its_menu() {
    let config = BrowserConfig::builder()
        .dialogs(Rc::new(SheetDialogs::new()))
        .build();
    let (mut app, state) = list_app(Box::new(home()), config, ViewMode::Icons);
    let at = centre(rect_of(&app, "Row Item c.txt"));
    click_at(&mut app, at, MouseButton::Right, Modifiers::default(), 1);
    assert_eq!(selected(&state), ["c.txt"]);
    assert_eq!(
        list(&app).menu_rows().first().map(|r| r.0.as_str()),
        Some("Rename")
    );
}

/// Pictures that answer `Generating` until the test hands them a picture.
#[derive(Default)]
struct LaterPictures {
    ready: RefCell<Option<IconImage>>,
    arrived: RefCell<Option<Rc<dyn Fn()>>>,
}

impl ItemPictures for LaterPictures {
    fn picture(&self, cx: &PictureCx<'_>, entry: &Entry, _side: i32) -> PictureState {
        if entry.is_folder() {
            return PictureState::Default;
        }
        *self.arrived.borrow_mut() = Some(cx.arrived_callback());
        match self.ready.borrow().clone() {
            Some(image) => PictureState::Ready(image),
            None => PictureState::Generating,
        }
    }
}

/// The colour at `at` (root coordinates, Y up) once `app` has painted.
fn colour_at(app: &mut App, at: Point) -> [u8; 4] {
    let mut fb = Framebuffer::new(WIDTH as u32, HEIGHT as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        app.paint(&mut ctx);
    }
    let pixels = fb.pixels_flipped();
    let (x, y) = (at.x as usize, (HEIGHT - at.y) as usize);
    let i = (y * WIDTH as usize + x) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

#[test]
fn a_list_on_its_hosts_offset_keeps_it_and_never_resets_it() {
    let offset = Rc::new(Cell::new(10.0));
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    // Rows in a list far shorter than them, so the offset is never clamped away.
    state.borrow_mut().set_view_mode(ViewMode::Rows);
    let list = BrowserList::new(&state, font()).with_offset_cell(Rc::clone(&offset));
    let mut app = App::new(Box::new(list));
    app.layout(Size::new(WIDTH, 30.0));
    assert_eq!(
        offset.get(),
        10.0,
        "a new list takes the host's offset as it is"
    );
    // The host decides when new content goes back to the top.
    state.borrow_mut().go_to(0);
    app.layout(Size::new(WIDTH, 30.0));
    assert_eq!(offset.get(), 10.0);
}

/// The address of the widget named `id`: the same widget keeps it.
fn address_of(app: &App, id: &str) -> usize {
    let (widget, _) = find(app.root(), id, Point::new(0.0, 0.0)).expect("the widget");
    widget as *const dyn Widget as *const () as usize
}

#[test]
fn a_picture_arriving_later_redraws_its_tile() {
    let pictures = Rc::new(LaterPictures::default());
    let config = BrowserConfig::builder().pictures(pictures.clone()).build();
    let (mut app, _) = list_app(Box::new(home()), config, ViewMode::Icons);
    assert_eq!(property(&app, "Row Item a.txt", "picture"), "generating");
    let before = address_of(&app, "Row Item a.txt");
    let tile = rect_of(&app, "Row Item a.txt");
    // The picture's square is the tile's top 100 inside its border.
    let middle = Point::new(tile.x + 51.0, tile.y + tile.height - 51.0);
    let red = [255u8, 0, 0, 255].repeat(100 * 100);
    let image = IconImage::from_rgba(Arc::new(red), 100, 100, Size::new(100.0, 100.0));
    *pictures.ready.borrow_mut() = image;
    assert_ne!(colour_at(&mut app, middle), [255, 0, 0, 255]);
    let arrived = pictures
        .arrived
        .borrow()
        .clone()
        .expect("the hook was handed its callback");
    arrived();
    app.layout(Size::new(WIDTH, HEIGHT));
    assert_eq!(property(&app, "Row Item a.txt", "picture"), "picture");
    assert_eq!(
        address_of(&app, "Row Item a.txt"),
        before,
        "the tile is the same widget, so a press on it carries on"
    );
    assert_eq!(colour_at(&mut app, middle), [255, 0, 0, 255]);
}

/// The default look, but the folder message's button is the app's own (a `Label` named as asked,
/// enabled as asked, pressed through `press`).
struct OwnButtonLook(
    agg_gui_file_browser::DefaultLook,
    RefCell<Option<Rc<dyn Fn()>>>,
);

impl agg_gui_file_browser::BrowserLook for OwnButtonLook {
    fn style(&self) -> agg_gui_file_browser::BrowserStyle {
        self.0.style()
    }
    fn names(&self) -> &agg_gui_file_browser::BrowserNames {
        self.0.names()
    }
    fn message_button(
        &self,
        title: &str,
        name: &str,
        enabled: Rc<dyn Fn() -> bool>,
        press: Rc<dyn Fn()>,
    ) -> Option<Box<dyn Widget>> {
        *self.1.borrow_mut() = Some(press);
        let label = agg_gui::Label::new(format!("{title} {}", enabled()), font());
        Some(Box::new(agg_gui::widgets::named::Named::new(
            name.to_string(),
            Box::new(label),
        )))
    }
}

#[test]
fn a_look_makes_the_folder_messages_button_its_own() {
    let folder = message_folder(
        MessageItem::new("Sign in to see this folder"),
        Some(ListingFailure::SignIn),
    );
    let actions = Rc::new(SignsIn::default());
    let look = Rc::new(OwnButtonLook(
        agg_gui_file_browser::DefaultLook::default(),
        RefCell::new(None),
    ));
    let config = BrowserConfig::builder()
        .actions(actions.clone())
        .look(look.clone())
        .build();
    let (app, _) = list_app(folder, config, ViewMode::Icons);
    let (button, _) = find(
        app.root(),
        "Library Folder Sign In Button",
        Point::new(0.0, 0.0),
    )
    .expect("the look's button under the message's name");
    assert_eq!(button.type_name(), "Named");
    assert_eq!(
        button.children()[0]
            .properties()
            .into_iter()
            .find(|(k, _)| *k == "text")
            .map(|(_, v)| v)
            .as_deref(),
        Some("Sign In true")
    );
    (look.1.borrow().clone().expect("handed the press"))();
    assert_eq!(actions.presses.get(), 1);
}

/// A folder listing one message item, refused for `failure`.
struct MessageFolder {
    message: Rc<dyn Item>,
    failure: Option<ListingFailure>,
}

impl Folder for MessageFolder {
    fn id(&self) -> &str {
        "messages"
    }
    fn name(&self) -> &str {
        "Cloud"
    }
    fn is_protected(&self) -> bool {
        true
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        Vec::new()
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        vec![Rc::clone(&self.message)]
    }
    fn load(&mut self) {}
    fn listing_failure(&self) -> Option<ListingFailure> {
        self.failure
    }
}

fn message_folder(message: MessageItem, failure: Option<ListingFailure>) -> Box<dyn Folder> {
    Box::new(MessageFolder {
        message: Rc::new(message),
        failure,
    })
}

#[test]
fn a_loading_folder_shows_its_message_in_place_of_rows() {
    for mode in [ViewMode::Rows, ViewMode::Icons] {
        let folder = message_folder(MessageItem::loading(), None);
        let (app, _) = list_app(folder, BrowserConfig::default(), mode);
        let loading = MessageItem::loading().name().to_string();
        assert_eq!(property(&app, "Library Folder Message", "text"), loading);
        assert_eq!(property(&app, "Library Folder Message", "icon"), "false");
        assert!(shown_of(&app, "BrowserRow").is_empty());
        assert!(shown_of(&app, "BrowserTile").is_empty());
        let text = rect_of(&app, "Library Folder Message Text");
        assert!(
            text.y > 0.0 && text.y + text.height < HEIGHT,
            "centred in the list"
        );
    }
}

/// Actions offering a Sign In and counting its presses; `waiting` greys it.
#[derive(Default)]
struct SignsIn {
    presses: Rc<Cell<u32>>,
    waiting: Cell<bool>,
}

impl BrowserActions for SignsIn {
    fn sign_in(&self, _state: &BrowserRef, folder_text: &str) -> Option<SignInOffer> {
        let presses = Rc::clone(&self.presses);
        Some(SignInOffer {
            text: format!("{folder_text} (signed out)"),
            action: "Sign In".to_string(),
            enabled: !self.waiting.get(),
            press: Rc::new(move || presses.set(presses.get() + 1)),
        })
    }
}

#[test]
fn a_folder_refused_for_the_sign_in_shows_the_apps_sign_in_and_its_warning() {
    let folder = message_folder(
        MessageItem::new("Sign in to see this folder"),
        Some(ListingFailure::SignIn),
    );
    let actions = Rc::new(SignsIn::default());
    let config = BrowserConfig::builder().actions(actions.clone()).build();
    let (mut app, _) = list_app(folder, config, ViewMode::Icons);
    let message = "Library Folder Message";
    assert_eq!(
        property(&app, message, "text"),
        "Sign in to see this folder (signed out)"
    );
    assert_eq!(property(&app, message, "icon"), "true");
    assert_eq!(property(&app, message, "sign_in"), "Sign In");
    let button = centre(rect_of(&app, "Library Folder Sign In Button"));
    click_at(&mut app, button, MouseButton::Left, Modifiers::default(), 1);
    assert_eq!(actions.presses.get(), 1);
    // The button found by its name says whether it takes a press.
    let enabled = |app: &App| {
        let (button, _) = find(
            app.root(),
            "Library Folder Sign In Button",
            Point::new(0.0, 0.0),
        )
        .expect("the Sign In");
        button.is_enabled()
    };
    assert!(enabled(&app));
    actions.waiting.set(true);
    agg_gui::animation::request_layout();
    app.layout(Size::new(WIDTH, HEIGHT));
    assert!(
        !enabled(&app),
        "a Sign In waiting for its answer takes no press"
    );
    // Without the app's Sign In the message stands alone.
    let folder = message_folder(
        MessageItem::new("Sign in to see this folder"),
        Some(ListingFailure::SignIn),
    );
    let (app, _) = list_app(folder, BrowserConfig::default(), ViewMode::Icons);
    assert_eq!(property(&app, message, "sign_in"), "");
    assert!(find(
        app.root(),
        "Library Folder Sign In Button",
        Point::new(0.0, 0.0)
    )
    .is_none());
}

#[test]
fn the_keys_move_and_widen_the_selection_once_the_list_has_them() {
    let (mut app, state) = list_app(Box::new(home()), BrowserConfig::default(), ViewMode::Rows);
    click_row(&mut app, "Row Item a.txt", Modifiers::default());
    app.on_key_down(agg_gui::Key::ArrowDown, Modifiers::default());
    assert_eq!(selected(&state), ["b.txt"]);
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    app.on_key_down(agg_gui::Key::ArrowUp, shift);
    app.on_key_down(agg_gui::Key::ArrowUp, shift);
    assert_eq!(selected(&state), ["Projects", "a.txt", "b.txt"]);
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    app.on_key_down(agg_gui::Key::Char('a'), ctrl);
    assert_eq!(selected(&state).len(), 5);
    app.on_key_down(agg_gui::Key::Escape, Modifiers::default());
    assert!(selected(&state).is_empty());
}

#[test]
fn a_band_drawn_from_empty_space_selects_the_rows_it_touches() {
    let (mut app, state) = list_app(Box::new(home()), BrowserConfig::default(), ViewMode::Rows);
    let b = centre(rect_of(&app, "Row Item b.txt"));
    let (x, y) = (200.0, HEIGHT - 10.0);
    app.on_mouse_move(x, y);
    app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_move(x, HEIGHT - b.y);
    app.on_mouse_up(x, HEIGHT - b.y, MouseButton::Left, Modifiers::default());
    assert_eq!(selected(&state), ["b.txt", "c.txt"]);
}
