//! A list whose folder holds more items than its pictures hook keeps still settles: every build
//! asks for the picture of every row in view (here the list is tall enough to show them all),
//! and none asked for by one build leaves before the next, so each picture is made once and
//! every tile shows it. A picture that lands while the rows are being
//! built (an inline spawner) is shown by the next layout, not lost.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Waker};

use agg_gui::text::Font;
use agg_gui::{App, Size, Widget};
use agg_gui_file_browser::{
    run_ready_local_tasks, BrowserConfig, BrowserList, BrowserState, OwnPictures, ViewMode,
};
use agg_gui_files::{
    Folder, FolderLink, Item, LocalTask, Picture, PictureFuture, Spawner, Thumbnail,
};

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// An item whose picture is a 4 x 4 square, counting how many times it is made.
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
        let green = [0u8, 255, 0, 255].repeat(16);
        Box::pin(async move { Ok(Picture::new(4, 4, green)) })
    }
}

/// A folder of `count` drawings sharing one counter of pictures made.
struct Drawings {
    count: usize,
    made: Rc<Cell<u32>>,
}

impl Folder for Drawings {
    fn id(&self) -> &str {
        "drawings"
    }
    fn name(&self) -> &str {
        "Drawings"
    }
    fn is_protected(&self) -> bool {
        true
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        Vec::new()
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        (0..self.count)
            .map(|i| {
                Rc::new(Drawing {
                    name: format!("part {i:02}"),
                    made: Rc::clone(&self.made),
                }) as Rc<dyn Item>
            })
            .collect()
    }
    fn load(&mut self) {}
}

/// Runs each task to its end the moment it is spawned (the pictures here are ready at once).
struct InlineSpawner;

impl Spawner for InlineSpawner {
    fn spawn_task(&self, mut task: LocalTask) {
        let mut cx = Context::from_waker(Waker::noop());
        while task.as_mut().poll(&mut cx).is_pending() {}
    }
    fn report_fault(&self, error: Arc<dyn std::error::Error + Send + Sync>) {
        panic!("no task here fails: {error}");
    }
}

fn pictures_shown(widget: &dyn Widget, shown: &mut Vec<String>) {
    if let Some((_, v)) = widget
        .properties()
        .into_iter()
        .find(|(k, _)| *k == "picture")
    {
        shown.push(v);
    }
    for child in widget.children() {
        pictures_shown(child.as_ref(), shown);
    }
}

fn shown(app: &App) -> Vec<String> {
    let mut shown = Vec::new();
    pictures_shown(app.root(), &mut shown);
    shown
}

#[test]
fn a_folder_with_more_items_than_pictures_kept_settles_with_each_picture_made_once() {
    agg_gui::set_device_scale(1.0);
    let made = Rc::new(Cell::new(0));
    let folder = Drawings {
        count: 30,
        made: Rc::clone(&made),
    };
    let config = BrowserConfig::builder()
        .pictures(Rc::new(OwnPictures::with_capacity(8)))
        .build();
    let state = BrowserState::new(Box::new(folder), config);
    state.borrow_mut().set_view_mode(ViewMode::Icons);
    let mut app = App::new(Box::new(BrowserList::new(&state, font())));
    // Tall enough for all 30 tiles (ten rows of three), so every one is in view and asks.
    let tall = Size::new(400.0, 2000.0);
    for _ in 0..10 {
        app.layout(tall);
        run_ready_local_tasks();
    }
    app.layout(tall);
    let tiles = shown(&app);
    assert_eq!(tiles.len(), 30, "every tile is built");
    assert!(
        tiles.iter().all(|p| p == "picture"),
        "every tile shows its picture: {tiles:?}"
    );
    assert_eq!(made.get(), 30, "each picture is made once");
    let before = made.get();
    for _ in 0..3 {
        app.layout(tall);
        run_ready_local_tasks();
    }
    assert_eq!(
        made.get(),
        before,
        "the settled list makes no more pictures"
    );
}

#[test]
fn a_picture_that_lands_while_the_list_builds_shows_at_the_next_layout() {
    agg_gui::set_device_scale(1.0);
    let made = Rc::new(Cell::new(0));
    let folder = Drawings {
        count: 2,
        made: Rc::clone(&made),
    };
    let config = BrowserConfig::builder()
        .spawner(Rc::new(InlineSpawner))
        .build();
    let state = BrowserState::new(Box::new(folder), config);
    state.borrow_mut().set_view_mode(ViewMode::Icons);
    let mut app = App::new(Box::new(BrowserList::new(&state, font())));
    app.layout(Size::new(400.0, 300.0));
    app.layout(Size::new(400.0, 300.0));
    assert_eq!(shown(&app), ["picture", "picture"]);
    assert_eq!(made.get(), 2);
}
