//! The capture worklet's module (`web/capture-worklet.js`) for the web
//! host's microphone (`mic.rs`), ready the moment a stream arrives.
//!
//! What the microphone hears before its graph is wired is lost, and the
//! request (`ask.rs`) goes out inside the tap, alongside the module's load,
//! so the two race. The module's bytes (about a kilobyte, not the model) are
//! therefore fetched when the provider is installed ([`prefetch`]) and kept
//! as a Blob URL, from which the tap's load ([`start`]) compiles in
//! milliseconds; a page whose policy refuses Blob URLs to worklets loads it
//! from its own URL instead. A stream that arrives first waits for the
//! module at most [`MODULE_WAIT_MS`] ([`ready_within`]); past that the
//! dictation captures with a `ScriptProcessor`, so a slow module costs at
//! most that wait.

use std::rc::Rc;

use js_sys::{Array, Promise};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AudioContext, Blob, BlobPropertyBag, Response, Url};

use super::{mic, Shared};

/// How long a stream that arrived waits for the module, in ms.
pub(super) const MODULE_WAIT_MS: i32 = 1_500;

/// What the wait's timer resolves to.
const LATE: &str = "late";

/// Fetch the module's bytes now, while the provider is installed, and keep
/// them as a Blob URL (`Shared::worklet_blob`). Failing quietly: the tap
/// then loads the module from its URL.
pub(super) fn prefetch(shared: &Rc<Shared>) {
    let weak = Rc::downgrade(shared);
    let url = shared.worklet_url.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let Some(blob_url) = blob_url(&url).await else {
            return;
        };
        match weak.upgrade() {
            Some(shared) => *shared.worklet_blob.borrow_mut() = Some(blob_url),
            None => {
                let _ = Url::revoke_object_url(&blob_url);
            }
        }
    });
}

/// The module at `url` as a Blob URL.
async fn blob_url(url: &str) -> Option<String> {
    let window = web_sys::window()?;
    let response: Response = JsFuture::from(window.fetch_with_str(url))
        .await
        .ok()?
        .dyn_into()
        .ok()?;
    if !response.ok() {
        return None;
    }
    let text = JsFuture::from(response.text().ok()?).await.ok()?;
    let options = BlobPropertyBag::new();
    options.set_type("text/javascript");
    let blob = Blob::new_with_str_sequence_and_options(&Array::of1(&text), &options).ok()?;
    Url::create_object_url_with_blob(&blob).ok()
}

/// Start loading the module into `ctx` (inside the tap): from the
/// prefetched bytes when they are in, else (or when the page refuses the
/// Blob URL) from its URL. The promise resolves once it is loaded and
/// rejects with why it isn't.
pub(super) fn start(ctx: &AudioContext, shared: &Shared) -> Promise {
    let url = shared.worklet_url.clone();
    let blob = shared.worklet_blob.borrow().clone();
    let worklet = ctx.audio_worklet();
    wasm_bindgen_futures::future_to_promise(async move {
        let worklet = worklet.map_err(|_| JsValue::from_str("this browser has none"))?;
        if let Some(blob) = blob {
            if let Ok(loading) = worklet.add_module(&blob) {
                if JsFuture::from(loading).await.is_ok() {
                    return Ok(JsValue::TRUE);
                }
            }
        }
        let loading = worklet
            .add_module(&url)
            .map_err(|e| JsValue::from_str(&mic::js_text(&e)))?;
        JsFuture::from(loading)
            .await
            .map_err(|e| JsValue::from_str(&format!("{url} did not load: {}", mic::js_text(&e))))?;
        Ok(JsValue::TRUE)
    })
}

/// Wait for the module [`start`] loads, at most `ms`; why not, where it
/// failed or is still loading then. Resolves within the same task when it
/// is already loaded.
pub(super) async fn ready_within(module: &Promise, ms: i32) -> Result<(), String> {
    let timer = Promise::new(&mut |resolve, _| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_1(
                &resolve,
                ms,
                &JsValue::from_str(LATE),
            );
        }
    });
    match JsFuture::from(Promise::race(&Array::of2(module, &timer))).await {
        Ok(value) if value.as_string().as_deref() == Some(LATE) => {
            Err(format!("not loaded {ms} ms after the microphone opened"))
        }
        Ok(_) => Ok(()),
        Err(why) => Err(why
            .as_string()
            .unwrap_or_else(|| "the module did not load".into())),
    }
}
