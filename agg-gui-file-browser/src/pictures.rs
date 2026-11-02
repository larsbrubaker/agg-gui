//! The pictures a browser's tiles and rows show: the [`ItemPictures`] hook, asked for each listed
//! entry's picture ([`PictureState`]) with a [`PictureCx`] (where work runs, how to say a picture
//! arrived, and the open folder's own reading of an item), and for a widget laid over a tile
//! (an app's favourite marker). [`NoPictures`] is the hook of an app that wants no pictures: every
//! entry draws the look's item or folder icon (`BrowserIcon::Item`, `BrowserIcon::Folder`).
//!
//! A picture that takes time is started on the first ask and answered `Generating` (or
//! `Default`) until it lands; the hook then calls [`PictureCx::picture_arrived`] and the browser
//! asks again. The hook keeps its own cache: the browser asks on every build of a row.

use std::rc::Rc;

use agg_gui::icon_image::IconImage;
use agg_gui::Widget;
use agg_gui_files::{Item, PictureFuture, Spawner};

use crate::entry::Entry;

/// What an entry shows where its picture goes.
#[derive(Clone)]
pub enum PictureState {
    /// The entry's picture.
    Ready(IconImage),
    /// The picture is being made: the look's `BrowserIcon::Generating`.
    Generating,
    /// No picture (or none yet): the look's item or folder icon.
    Default,
    /// The app's own image for the entry, standing for `kind` (its picture, its generating
    /// marker, or its item icon) and reported by the row under `name` (its `thumbnail`
    /// property): an app whose icons depend on the item (a file kind with its own default) or
    /// whose pictures carry a name of their own (a cache file) answers this. The image is drawn
    /// centred at its own size, shrunk to fit the square.
    Shown {
        /// What to draw.
        image: IconImage,
        /// What it stands for.
        kind: PictureKind,
        /// What the row reports as its `thumbnail` property.
        name: String,
    },
}

/// What a [`PictureState::Shown`] image stands for, reported as a row's `picture` property.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PictureKind {
    /// The entry's own picture.
    Picture,
    /// A marker that the picture is being made.
    Generating,
    /// The entry's icon, standing in for a picture it has none of (or none yet).
    Item,
}

impl PictureKind {
    /// The kind as the row's `picture` property: `picture`, `generating` or `item`.
    pub fn as_str(self) -> &'static str {
        match self {
            PictureKind::Picture => "picture",
            PictureKind::Generating => "generating",
            PictureKind::Item => "item",
        }
    }
}

/// The open folder's own reading of an item's picture (`Folder::get_thumbnail`), for an item at a
/// width and height.
pub type FolderPicture<'a> = &'a dyn Fn(&Rc<dyn Item>, i32, i32) -> Option<PictureFuture>;

/// What an [`ItemPictures`] hook is handed with each ask.
pub struct PictureCx<'a> {
    spawner: &'a Rc<dyn Spawner>,
    arrived: &'a Rc<dyn Fn()>,
    folder_picture: Option<FolderPicture<'a>>,
}

impl<'a> PictureCx<'a> {
    /// A context whose work runs on `spawner` and whose `arrived` the hook calls when a picture
    /// it could not answer at once has landed. The open folder has no reading of its own.
    pub fn new(spawner: &'a Rc<dyn Spawner>, arrived: &'a Rc<dyn Fn()>) -> Self {
        Self {
            spawner,
            arrived,
            folder_picture: None,
        }
    }

    /// The same context, the open folder reading pictures through `folder_picture`.
    pub fn with_folder_picture(mut self, folder_picture: FolderPicture<'a>) -> Self {
        self.folder_picture = Some(folder_picture);
        self
    }

    /// Where the hook runs work nobody awaits (loading a picture).
    pub fn spawner(&self) -> &Rc<dyn Spawner> {
        self.spawner
    }

    /// Tells the browser a picture landed, so it asks again.
    pub fn picture_arrived(&self) {
        (self.arrived)();
    }

    /// The callback [`Self::picture_arrived`] calls, for a hook to keep in the work it starts.
    pub fn arrived_callback(&self) -> Rc<dyn Fn()> {
        Rc::clone(self.arrived)
    }

    /// The open folder's reading of `item`'s picture at `width` x `height`, when the folder has
    /// one (a remote folder's own thumbnails); `None` sends the hook to the item itself.
    pub fn folder_picture(
        &self,
        item: &Rc<dyn Item>,
        width: i32,
        height: i32,
    ) -> Option<PictureFuture> {
        self.folder_picture
            .and_then(|read| read(item, width, height))
    }
}

/// The pictures a browser shows for its entries.
pub trait ItemPictures {
    /// What `entry` shows in a square `side` logical pixels wide.
    fn picture(&self, cx: &PictureCx<'_>, entry: &Entry, side: i32) -> PictureState;

    /// A widget laid over `entry`'s tile (an app's favourite marker), or `None`. The default lays
    /// nothing.
    fn tile_overlay(&self, entry: &Entry) -> Option<Box<dyn Widget>> {
        let _ = entry;
        None
    }
}

/// The pictures of an app with none: every entry draws the look's icon.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoPictures;

impl ItemPictures for NoPictures {
    fn picture(&self, _cx: &PictureCx<'_>, _entry: &Entry, _side: i32) -> PictureState {
        PictureState::Default
    }
}
