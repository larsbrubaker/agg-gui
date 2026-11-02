//! [`OwnPictures`], the pictures of a browser whose app gives none: each item shows the picture
//! it or its folder makes of it (`Folder::get_thumbnail`, read through
//! [`PictureCx::folder_picture`], else the item's own `Item::as_thumbnail`), and the look's item
//! icon while that picture is on its way, when it has none or when making it failed (the
//! failure goes to the spawner's log, not to the user: a tile with its icon still works).
//! Folder rows are the look's.
//!
//! A picture is asked for once per item and side: the first ask starts it on the context's
//! spawner and answers `Default`; once it lands it is kept, the browser is told
//! (`PictureCx::picture_arrived`) and every later ask answers it at once. Pictures are made at
//! the device scale the first ask was made at, so a tile draws them pixel for pixel.
//!
//! At most [`KEPT_PICTURES`] pictures are kept (the least recently asked for leave first), so a
//! browser walked through many folders holds a bounded amount of pixels: 1024 covers a folder of
//! a few hundred items shown at two sides (rows and an icon size) with the one walked back to.
//! A picture that left is made again at its next ask, the item icon showing meanwhile.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::icon_image::IconImage;
use agg_gui::Size;
use agg_gui_files::Picture;

use crate::entry::Entry;
use crate::pictures::{ItemPictures, PictureCx, PictureState};
use crate::state::item_identity_key;

/// How many pictures an [`OwnPictures`] keeps (see the module header).
pub const KEPT_PICTURES: usize = 1024;

/// One item's picture at one side: being made, or made (`None`: the item has none).
enum Slot {
    Making,
    Made(Option<IconImage>),
}

/// An item and side's picture (an item's identity key and the side).
type Key = (String, i32);

/// The kept pictures, each with when it was last asked for.
struct Kept {
    slots: HashMap<Key, (Slot, u64)>,
    clock: u64,
    capacity: usize,
}

impl Kept {
    /// The slot of `key`, marked as asked for now.
    fn ask(&mut self, key: &Key) -> Option<&Slot> {
        self.clock += 1;
        let now = self.clock;
        let (slot, asked) = self.slots.get_mut(key)?;
        *asked = now;
        Some(slot)
    }

    /// Keeps `slot` for `key`, the least recently asked for leaving while over the capacity.
    fn keep(&mut self, key: Key, slot: Slot) {
        self.clock += 1;
        self.slots.insert(key, (slot, self.clock));
        while self.slots.len() > self.capacity {
            // Ties cannot happen: every ask and keep takes its own tick.
            let oldest = self
                .slots
                .iter()
                .min_by_key(|(_, (_, asked))| *asked)
                .map(|(key, _)| key.clone());
            match oldest {
                Some(key) => self.slots.remove(&key),
                None => break,
            };
        }
    }
}

/// The pictures items and their folders make of them (see the module header).
pub struct OwnPictures {
    kept: Rc<RefCell<Kept>>,
}

impl Default for OwnPictures {
    fn default() -> Self {
        Self::with_capacity(KEPT_PICTURES)
    }
}

impl OwnPictures {
    /// Pictures with none kept yet, keeping up to [`KEPT_PICTURES`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Pictures keeping up to `capacity` (at least one).
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            kept: Rc::new(RefCell::new(Kept {
                slots: HashMap::new(),
                clock: 0,
                capacity: capacity.max(1),
            })),
        }
    }

    /// How many pictures are kept now (made, being made, or known to be none).
    pub fn kept(&self) -> usize {
        self.kept.borrow().slots.len()
    }
}

/// `picture` as an image drawn `side` logical units across at most, at the device `scale`.
fn picture_image(picture: Picture, side: i32, scale: f64) -> Option<IconImage> {
    let (w, h) = (f64::from(picture.width), f64::from(picture.height));
    // A picture larger than its room is drawn smaller; a smaller one at its own pixels.
    let fit = (f64::from(side) * scale / w.max(h)).min(1.0) / scale;
    IconImage::from_rgba(
        Arc::new(picture.rgba.to_vec()),
        picture.width,
        picture.height,
        Size::new(w * fit, h * fit),
    )
}

impl ItemPictures for OwnPictures {
    fn picture(&self, cx: &PictureCx<'_>, entry: &Entry, side: i32) -> PictureState {
        if entry.is_folder() {
            return PictureState::Default;
        }
        let key = (item_identity_key(entry.item().as_ref()), side);
        if let Some(slot) = self.kept.borrow_mut().ask(&key) {
            return match slot {
                Slot::Made(Some(image)) => PictureState::Ready(image.clone()),
                _ => PictureState::Default,
            };
        }
        let scale = agg_gui::device_scale();
        // Saturates for an absurd side; a tile is a few hundred units.
        let pixels = (f64::from(side) * scale).round() as i32;
        let item = entry.item();
        let future = cx
            .folder_picture(item, pixels, pixels)
            .or_else(|| item.as_thumbnail().map(|t| t.get_thumbnail(pixels, pixels)));
        let Some(future) = future else {
            self.kept.borrow_mut().keep(key, Slot::Made(None));
            return PictureState::Default;
        };
        self.kept.borrow_mut().keep(key.clone(), Slot::Making);
        let kept = Rc::clone(&self.kept);
        let spawner = Rc::clone(cx.spawner());
        let arrived = cx.arrived_callback();
        cx.spawner().spawn_task(Box::pin(async move {
            let image = match future.await {
                Ok(Some(picture)) => picture_image(picture, side, scale),
                Ok(None) => None,
                Err(error) => {
                    spawner.log_error("BrowserPicture", &error.to_string());
                    None
                }
            };
            kept.borrow_mut().keep(key, Slot::Made(image));
            arrived();
        }));
        PictureState::Default
    }
}
