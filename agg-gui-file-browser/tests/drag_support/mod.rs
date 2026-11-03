//! What the drag-and-drop test binaries share (`drag_drop.rs`, `drag_drop_hooks.rs`): an app
//! showing a browser's folder tree beside its list with the app's own view to their right, the
//! pointer's presses, drags and releases over it, scratch folders on disk and in two places, and
//! an app view (`View`) that takes the items and notes what it heard. Each binary uses a part of
//! it, so what one leaves unused is no fault.

#![allow(dead_code, unused_imports)]

pub use std::cell::RefCell;
pub use std::path::{Path, PathBuf};
pub use std::rc::Rc;
pub use std::sync::Arc;

pub use agg_gui::text::Font;
pub use agg_gui::{
    App, DrawCtx, Event, EventResult, Framebuffer, GfxCtx, Key, Modifiers, MouseButton, Point,
    Rect, Size, Widget,
};
pub use agg_gui_file_browser::{
    drop_into, run_ready_local_tasks, BrowserConfig, BrowserList, BrowserOptions, BrowserState,
    BrowserTree, CopyCx, CopyTakeover, DropKey, DropTargetKind, Entry, ItemPictures, MoveCx,
    MoveTakeover, OutsideDrops, OutsideOver, OverlayFrame, PictureCx, PictureState, PlaceRules,
    ViewMode, SPRING_DELAY,
};
pub use agg_gui_files::disk::{AllFiles, DiskFolder};
pub use agg_gui_files::Item;
pub use agg_gui_files::{
    DynamicFolder, DynamicFolderLink, Folder, FolderLink, MemoryFolder, PlaceId,
};

pub const WIDTH: f64 = 600.0;
pub const HEIGHT: f64 = 400.0;
/// The tree's width, left of the list.
pub const TREE: f64 = 180.0;
/// The room right of the list: the app's own view, outside the browser.
pub const VIEW: f64 = 100.0;

pub fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// The tree left of the list, as an app places them.
pub struct Split {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Split {
    fn type_name(&self) -> &'static str {
        "Split"
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
        let widths = [TREE, available.width - TREE - VIEW];
        let mut x = 0.0;
        for (child, width) in self.children.iter_mut().zip(widths) {
            child.layout(Size::new(width, available.height));
            child.set_bounds(Rect::new(x, 0.0, width, available.height));
            x += width;
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// An app showing the tree and the list of a browser at `root` with `config`, in `mode`.
pub fn browser_app(
    root: Box<dyn Folder>,
    config: BrowserConfig,
    mode: ViewMode,
) -> (App, Rc<RefCell<BrowserState>>) {
    agg_gui::set_device_scale(1.0);
    let state = BrowserState::new(root, config);
    state.borrow_mut().set_view_mode(mode);
    let split = Split {
        bounds: Rect::default(),
        children: vec![
            Box::new(BrowserTree::for_state(&state, font())),
            Box::new(BrowserList::new(&state, font())),
        ],
    };
    let mut app = App::new(Box::new(split));
    frame(&mut app);
    (app, state)
}

/// A layout and a paint, as a frame does: the folders record themselves as drop targets.
pub fn frame(app: &mut App) {
    app.layout(Size::new(WIDTH, HEIGHT));
    let mut fb = Framebuffer::new(WIDTH as u32, HEIGHT as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

/// The widget named `id` and its rectangle in the root's coordinates (Y up).
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

pub fn found<'a>(app: &'a App, id: &str) -> (&'a dyn Widget, Rect) {
    find(app.root(), id, Point::new(0.0, 0.0)).unwrap_or_else(|| panic!("no widget named {id}"))
}

pub fn centre(app: &App, id: &str) -> Point {
    let r = found(app, id).1;
    Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0)
}

pub fn property(app: &App, id: &str, key: &str) -> String {
    found(app, id)
        .0
        .properties()
        .into_iter()
        .find(|(k, _)| *k == key)
        .unwrap_or_else(|| panic!("{id} has no {key}"))
        .1
}

/// The modifiers holding the copy modifier: Option on the Mac, Ctrl elsewhere.
pub fn copy_modifier() -> Modifiers {
    Modifiers {
        alt: cfg!(target_os = "macos"),
        ctrl: !cfg!(target_os = "macos"),
        ..Modifiers::default()
    }
}

/// Moves the pointer to `at` (root coordinates, Y up), then a frame.
pub fn move_to(app: &mut App, at: Point) {
    app.on_mouse_move(at.x, HEIGHT - at.y);
    frame(app);
}

/// Presses on the widget named `from`, drags in steps to the widget named `to`, and holds there
/// (a second move once the targets have painted). The button stays down.
pub fn drag_over(app: &mut App, from: &str, to: &str) -> Point {
    let (start, end) = (centre(app, from), centre(app, to));
    app.on_mouse_move(start.x, HEIGHT - start.y);
    app.on_mouse_down(
        start.x,
        HEIGHT - start.y,
        MouseButton::Left,
        Modifiers::default(),
    );
    frame(app);
    for step in 1..=4 {
        let t = f64::from(step) / 4.0;
        let at = Point::new(
            start.x + (end.x - start.x) * t,
            start.y + (end.y - start.y) * t,
        );
        move_to(app, at);
    }
    move_to(app, Point::new(end.x + 1.0, end.y));
    Point::new(end.x + 1.0, end.y)
}

/// Releases at `at` with `modifiers`, then runs the copies started and a frame.
pub fn release(app: &mut App, at: Point, modifiers: Modifiers) {
    app.on_mouse_up(at.x, HEIGHT - at.y, MouseButton::Left, modifiers);
    for _ in 0..20 {
        run_ready_local_tasks();
    }
    frame(app);
}

/// A fresh scratch folder holding the folder Parts and the file gear.stl.
pub fn workshop(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "agg-gui-file-browser-drag-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Parts")).expect("made");
    std::fs::write(dir.join("gear.stl"), b"solid gear").expect("written");
    dir
}

pub fn disk(dir: &Path) -> Box<dyn Folder> {
    Box::new(DiskFolder::new(dir, Rc::new(AllFiles)))
}

/// Home listing Desk (in place "desk", holding gear.stl) and Shelf (in place "shelf"), each a
/// root row, and the browser showing Desk.
pub fn two_places(
    config: BrowserConfig,
    shelf_read_only: bool,
) -> (App, Rc<RefCell<BrowserState>>, MemoryFolder, MemoryFolder) {
    let desk = MemoryFolder::new("Desk").with_place(PlaceId::new("desk"));
    desk.add_file("gear.stl", b"solid".to_vec());
    let shelf = MemoryFolder::new("Shelf").with_place(PlaceId::new("shelf"));
    let shelf_link: Rc<dyn FolderLink> = match shelf_read_only {
        false => shelf.link(),
        true => {
            let handle = shelf.clone();
            Rc::new(
                DynamicFolderLink::new(
                    "Shelf",
                    Rc::new(move || Box::new(handle.clone()) as Box<dyn Folder>),
                )
                .with_read_only(true)
                .with_local_listing(),
            )
        }
    };
    let mut home = DynamicFolder::new("Home");
    home.data.child_containers = vec![desk.link(), shelf_link];
    let (mut app, state) = browser_app(Box::new(home), config, ViewMode::Rows);
    let desk_link = state.borrow().root_child_containers()[0].clone();
    state.borrow_mut().open(desk_link.as_ref());
    frame(&mut app);
    (app, state, desk, shelf)
}

pub fn names(folder: &MemoryFolder) -> Vec<String> {
    let mut folder = folder.clone();
    folder.load();
    folder
        .items()
        .iter()
        .map(|i| i.name().to_string())
        .collect()
}

/// The app's view right of the browser: it takes the items and shows them itself.
#[derive(Default)]
pub struct View {
    pub calls: RefCell<Vec<String>>,
}

impl OutsideDrops for View {
    fn over(&self, items: &[Rc<dyn Item>], _window_at: Point) -> OutsideOver {
        self.calls
            .borrow_mut()
            .push(format!("over {}", items.len()));
        OutsideOver::Target {
            hides_picture: true,
        }
    }
    fn release(&self, items: &[Rc<dyn Item>], _window_at: Point) -> bool {
        let names: Vec<&str> = items.iter().map(|i| i.name()).collect();
        self.calls
            .borrow_mut()
            .push(format!("release {}", names.join(",")));
        true
    }
    fn cancel(&self) {
        self.calls.borrow_mut().push("cancel".to_string());
    }
}

/// The list's property `key`.
pub fn list_property(app: &App, key: &str) -> String {
    property(app, "LibraryView", key)
}
