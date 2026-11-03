//! A list over a folder of hundreds of items asks the pictures hook only for the rows and tiles
//! near its visible part, in rows and in tiles, and scrolling asks for the rest as they come into
//! view: opening a big photo folder makes a screenful of pictures, not the whole folder's. A jump
//! to a row the app selects asks at the place the view jumps to, in the same layout.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{App, Size};
use agg_gui_file_browser::{
    BrowserConfig, BrowserList, BrowserState, Entry, ItemPictures, PictureCx, PictureState,
    ViewMode,
};
use agg_gui_files::MemoryFolder;

const WIDTH: f64 = 400.0;
const HEIGHT: f64 = 300.0;
const COUNT: usize = 400;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// A folder of `COUNT` photos, `photo 000.jpg` up.
fn photos() -> MemoryFolder {
    let folder = MemoryFolder::new("Photos");
    for i in 0..COUNT {
        folder.add_file(&format!("photo {i:03}.jpg"), vec![0u8; 4]);
    }
    folder
}

/// Pictures that record which entries they were asked about, and how often.
#[derive(Default)]
struct AskedPictures {
    names: RefCell<Vec<String>>,
    asks: Cell<usize>,
}

impl AskedPictures {
    fn distinct(&self) -> usize {
        let mut names = self.names.borrow().clone();
        names.sort();
        names.dedup();
        names.len()
    }
    fn asked_about(&self, name: &str) -> bool {
        self.names.borrow().iter().any(|n| n == name)
    }
}

impl ItemPictures for AskedPictures {
    fn picture(&self, _cx: &PictureCx<'_>, entry: &Entry, _side: i32) -> PictureState {
        self.names.borrow_mut().push(entry.name().to_string());
        self.asks.set(self.asks.get() + 1);
        PictureState::Generating
    }
}

/// The list of the photos in `mode`, on a host-kept scroll offset, laid out once.
fn photo_list(mode: ViewMode) -> (App, Rc<AskedPictures>, Rc<Cell<f64>>) {
    let (app, pictures, offset, _) = photo_list_with_state(mode);
    (app, pictures, offset)
}

/// A laid-out photo list: the app, its pictures, its host-kept offset and the browser's state.
type PhotoList = (
    App,
    Rc<AskedPictures>,
    Rc<Cell<f64>>,
    Rc<RefCell<BrowserState>>,
);

/// [`photo_list`], with the browser's state.
fn photo_list_with_state(mode: ViewMode) -> PhotoList {
    agg_gui::set_device_scale(1.0);
    let pictures = Rc::new(AskedPictures::default());
    let config = BrowserConfig::builder().pictures(pictures.clone()).build();
    let state = BrowserState::new(Box::new(photos()), config);
    state.borrow_mut().set_view_mode(mode);
    let offset = Rc::new(Cell::new(0.0));
    let list = BrowserList::new(&state, font()).with_offset_cell(Rc::clone(&offset));
    let mut app = App::new(Box::new(list));
    app.layout(Size::new(WIDTH, HEIGHT));
    (app, pictures, offset, state)
}

#[test]
fn opening_a_big_folder_in_rows_asks_only_for_the_rows_near_view() {
    let (mut app, pictures, offset) = photo_list(ViewMode::Rows);
    // 300 units show about five 56-unit rows; the read-ahead adds a couple above and below.
    let asked = pictures.distinct();
    assert!(asked > 0, "the rows on screen ask for their pictures");
    assert!(
        asked <= 12,
        "only the rows near view ask: {asked} of {COUNT}"
    );
    assert!(pictures.asked_about("photo 000.jpg"));
    assert!(!pictures.asked_about("photo 399.jpg"));
    // Laid out again with nothing new, no row asks again.
    let asks = pictures.asks.get();
    app.layout(Size::new(WIDTH, HEIGHT));
    assert_eq!(pictures.asks.get(), asks, "nothing new to ask for");
    // Scrolled to the end (the scroll view clamps the offset), the last rows ask.
    offset.set(1.0e9);
    app.layout(Size::new(WIDTH, HEIGHT));
    app.layout(Size::new(WIDTH, HEIGHT));
    assert!(pictures.asked_about("photo 399.jpg"), "the last row asked");
    assert!(
        !pictures.asked_about("photo 200.jpg"),
        "the rows scrolled past never came into view"
    );
    assert!(pictures.distinct() <= 24, "two screenfuls asked in all");
}

#[test]
fn opening_a_big_folder_in_tiles_asks_only_for_the_tiles_near_view() {
    let (mut app, pictures, offset) = photo_list(ViewMode::Icons);
    let asked = pictures.distinct();
    assert!(asked > 0, "the tiles on screen ask for their pictures");
    // 100-unit tiles: three across and three rows of them in view, with a row read ahead.
    assert!(
        asked <= 18,
        "only the tiles near view ask: {asked} of {COUNT}"
    );
    assert!(!pictures.asked_about("photo 399.jpg"));
    offset.set(1.0e9);
    app.layout(Size::new(WIDTH, HEIGHT));
    app.layout(Size::new(WIDTH, HEIGHT));
    assert!(pictures.asked_about("photo 399.jpg"), "the last tile asked");
    assert!(pictures.distinct() <= 36, "two screenfuls asked in all");
}

#[test]
fn a_jump_to_a_selected_row_asks_at_the_place_jumped_to_in_the_same_layout() {
    let (mut app, pictures, _offset, state) = photo_list_with_state(ViewMode::Rows);
    assert!(!pictures.asked_about("photo 399.jpg"));
    let last = state
        .borrow()
        .entries()
        .last()
        .map(|entry| entry.identity_key())
        .expect("the folder lists its photos");
    state.borrow_mut().select_when_listed(vec![last]);
    app.layout(Size::new(WIDTH, HEIGHT));
    assert!(
        pictures.asked_about("photo 399.jpg"),
        "the selected row asked in the layout that scrolled to it"
    );
    assert!(
        !pictures.asked_about("photo 200.jpg"),
        "the rows jumped over never asked"
    );
}
