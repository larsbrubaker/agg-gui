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
//! the device scale the first ask was made at, so a tile draws them pixel for pixel. The kept
//! pictures live as long as the hook does (one per browser config).

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

/// One item's picture at one side: being made, or made (`None`: the item has none).
enum Slot {
    Making,
    Made(Option<IconImage>),
}

/// The pictures items and their folders make of them (see the module header).
#[derive(Default)]
pub struct OwnPictures {
    kept: Rc<RefCell<HashMap<(String, i32), Slot>>>,
}

impl OwnPictures {
    /// Pictures with none kept yet.
    pub fn new() -> Self {
        Self::default()
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
        if let Some(slot) = self.kept.borrow().get(&key) {
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
            self.kept.borrow_mut().insert(key, Slot::Made(None));
            return PictureState::Default;
        };
        self.kept.borrow_mut().insert(key.clone(), Slot::Making);
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
            kept.borrow_mut().insert(key, Slot::Made(image));
            arrived();
        }));
        PictureState::Default
    }
}
