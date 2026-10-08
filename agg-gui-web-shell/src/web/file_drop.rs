//! Browser file drag-and-drop → [`agg_gui::App`]: `dragenter` / `dragover`
//! become `App::on_file_drag_hover`, `dragleave` becomes
//! `App::on_file_drag_leave`, and `drop` reads every dropped file's bytes and
//! delivers them in one `App::on_file_data_dropped`.
//!
//! A web page never sees a dropped file's path, and sees neither names nor
//! contents until the drop, so hovers carry no paths and the drop carries
//! names + bytes (`agg_gui::DroppedFileData`). Only drags that carry files
//! are handled; a text or link drag is left to the browser. Sits beside
//! [`super::input`], whose listener and coordinate helpers it reuses.

use wasm_bindgen_futures::JsFuture;

use super::input::{add_listener, pos};
use agg_gui::shell_input::ForwarderEvent;

use super::{forward, note_input};

/// Whether the drag carries files (`dataTransfer.types` contains `"Files"`).
fn carries_files(e: &web_sys::DragEvent) -> bool {
    e.data_transfer()
        .map(|dt| {
            dt.types()
                .iter()
                .any(|t| t.as_string().as_deref() == Some("Files"))
        })
        .unwrap_or(false)
}

pub(super) fn install_file_drop_listeners(canvas: &web_sys::HtmlCanvasElement) {
    let target: &web_sys::EventTarget = canvas.as_ref();
    for event in ["dragenter", "dragover"] {
        let c = canvas.clone();
        add_listener(target, event, move |e: web_sys::DragEvent| {
            if !carries_files(&e) {
                return;
            }
            // The browser only fires `drop` on a target that cancelled the
            // preceding `dragover`; without this it opens the file instead.
            e.prevent_default();
            if let Some(dt) = e.data_transfer() {
                dt.set_drop_effect("copy");
            }
            let (x, y) = pos(&c, e.client_x(), e.client_y());
            forward(ForwarderEvent::FileDragHover {
                x,
                y,
                paths: Vec::new(),
            });
            note_input();
        });
    }
    add_listener(target, "dragleave", move |e: web_sys::DragEvent| {
        if !carries_files(&e) {
            return;
        }
        forward(ForwarderEvent::FileDragLeave);
        note_input();
    });
    {
        let c = canvas.clone();
        add_listener(target, "drop", move |e: web_sys::DragEvent| {
            let Some(list) = e.data_transfer().and_then(|dt| dt.files()) else {
                return;
            };
            if list.length() == 0 {
                return;
            }
            e.prevent_default();
            let (x, y) = pos(&c, e.client_x(), e.client_y());
            // The drag is over now; reading the files is asynchronous, so end
            // the hover feedback without waiting for it.
            forward(ForwarderEvent::FileDragLeave);
            note_input();
            let files: Vec<web_sys::File> =
                (0..list.length()).filter_map(|i| list.get(i)).collect();
            wasm_bindgen_futures::spawn_local(read_and_deliver(files, x, y));
        });
    }
}

/// Read every file, then deliver them together. A file the browser fails to
/// read is reported to the console and left out; the rest still drop.
async fn read_and_deliver(files: Vec<web_sys::File>, x: f64, y: f64) {
    let mut read = Vec::with_capacity(files.len());
    for file in files {
        match JsFuture::from(file.array_buffer()).await {
            Ok(buffer) => {
                let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
                read.push(agg_gui::DroppedFileData::new(file.name(), bytes));
            }
            Err(err) => web_sys::console::error_2(
                &wasm_bindgen::JsValue::from_str(&format!(
                    "agg-gui-web-shell: could not read dropped file {}",
                    file.name()
                )),
                &err,
            ),
        }
    }
    forward(ForwarderEvent::FileDataDropped { x, y, files: read });
    note_input();
}
