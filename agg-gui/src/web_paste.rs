//! The browser `paste` listener that `web_adapter::install_keyboard_listeners`
//! installs: it carries the system clipboard's text and picture into the
//! in-process clipboard (`wasm_clipboard`) and then synthesizes `Ctrl+V`, so
//! the focused widget's paste handler reads them through
//! `clipboard::get_text` and `clipboard::get_image_rgba`.
//!
//! The decisions (which item is a picture, when to deliver, which of several
//! overlapping pastes wins) live in `clipboard_paste`, tested natively; this
//! file is only the DOM glue. A picture is decoded by the browser
//! (`createImageBitmap`, asynchronous and off the main thread) and read back
//! through an `OffscreenCanvas`, so no frame waits on the decode.
//!
//! Compiled only on `wasm32` targets.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::clipboard::ClipboardImage;
use crate::clipboard_paste::{image_item_index, PastePlan, PasteSequencer};
use crate::event::{Key, Modifiers};

/// The shell's key callback, shared with the keyboard listeners.
pub(crate) type OnKey = Rc<RefCell<dyn FnMut(Key, Modifiers, bool)>>;

/// Register the window-level `paste` listener for the page's lifetime.
pub(crate) fn install(window: &web_sys::Window, on_key: OnKey) {
    let sequencer = Rc::new(RefCell::new(PasteSequencer::default()));
    let paste_cb =
        Closure::<dyn FnMut(web_sys::ClipboardEvent)>::new(move |e: web_sys::ClipboardEvent| {
            if crate::web_adapter::targets_dom_editor(&e) {
                return;
            }
            let Some(data) = e.clipboard_data() else {
                return;
            };
            let text = data.get_data("text/plain").unwrap_or_default();
            let items = data.items();
            let kinds: Vec<(String, String)> = (0..items.length())
                .filter_map(|i| items.get(i))
                .map(|item| (item.kind(), item.type_()))
                .collect();
            let image_index = image_item_index(
                kinds
                    .iter()
                    .map(|(kind, mime)| (kind.as_str(), mime.as_str())),
            );
            let plan = sequencer.borrow_mut().plan(text, image_index);
            match plan {
                PastePlan::Ignore => {}
                PastePlan::Text(text) => {
                    e.prevent_default();
                    deliver(&on_key, &text, None);
                }
                PastePlan::DecodeImage {
                    index,
                    ticket,
                    text,
                } => {
                    e.prevent_default();
                    // Take the file now: the event's DataTransfer is emptied
                    // once this handler returns.
                    let file = u32::try_from(index)
                        .ok()
                        .and_then(|i| items.get(i))
                        .and_then(|item| item.get_as_file().ok().flatten());
                    let on_key = Rc::clone(&on_key);
                    let sequencer = Rc::clone(&sequencer);
                    wasm_bindgen_futures::spawn_local(async move {
                        let image = match file {
                            Some(file) => decode(&file).await,
                            None => None,
                        };
                        let delivery = sequencer.borrow().finish(ticket, text, image);
                        if let Some(delivery) = delivery {
                            deliver(&on_key, &delivery.text, delivery.image);
                        }
                    });
                }
            }
        });
    let _ = window.add_event_listener_with_callback("paste", paste_cb.as_ref().unchecked_ref());
    paste_cb.forget();
}

/// Fill the in-process clipboard with the paste, then synthesize `Ctrl+V` so
/// the focused widget's paste handler runs against it.
fn deliver(on_key: &OnKey, text: &str, image: Option<ClipboardImage>) {
    crate::wasm_clipboard::set_paste(text, image);
    let mods = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    (on_key.borrow_mut())(Key::Char('v'), mods, true);
}

/// Decode a pasted picture file to RGBA8 pixels, `None` when the browser
/// can't decode it (or has no `OffscreenCanvas` to read it back through:
/// Safari before 16.4, where the paste then delivers only its text).
async fn decode(blob: &web_sys::Blob) -> Option<ClipboardImage> {
    // Ask for the file's pixels as stored: no alpha premultiplication and no
    // colour-profile conversion, so they reach the app unchanged.
    let options = web_sys::ImageBitmapOptions::new();
    options.set_premultiply_alpha(web_sys::PremultiplyAlpha::None);
    options.set_color_space_conversion(web_sys::ColorSpaceConversion::None);
    let promise = web_sys::window()?
        .create_image_bitmap_with_blob_and_image_bitmap_options(blob, &options)
        .ok()?;
    let bitmap: web_sys::ImageBitmap = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .ok()?
        .dyn_into()
        .ok()?;
    let pixels = read_pixels(&bitmap);
    bitmap.close();
    pixels
}

/// Draw a decoded bitmap into an offscreen 2D canvas and read its pixels
/// back (`getImageData` returns RGBA8 with alpha not premultiplied).
fn read_pixels(bitmap: &web_sys::ImageBitmap) -> Option<ClipboardImage> {
    let (width, height) = (bitmap.width(), bitmap.height());
    let canvas = web_sys::OffscreenCanvas::new(width, height).ok()?;
    let context: web_sys::OffscreenCanvasRenderingContext2d =
        canvas.get_context("2d").ok()??.dyn_into().ok()?;
    context
        .draw_image_with_image_bitmap(bitmap, 0.0, 0.0)
        .ok()?;
    let data = context
        .get_image_data(0.0, 0.0, f64::from(width), f64::from(height))
        .ok()?;
    ClipboardImage::new(width, height, data.data().0)
}
