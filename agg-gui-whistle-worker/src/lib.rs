//! The transcription Web Worker's wasm module for agg-gui-whistle.
//!
//! `agg-gui-whistle/web/worker.js` loads this module in a dedicated worker
//! and calls [`run`] with the messages that arrived while it loaded; from
//! then on `agg_gui_whistle::worker` answers the provider's messages. All
//! the logic lives in agg-gui-whistle (its `worker` feature); this crate
//! only makes it a module of its own, built with simd128 (see Cargo.toml).
//!
//! Natively the crate is empty (a workspace member so fmt and clippy see it).

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;

/// Start answering messages; `queued` holds those that arrived before the
/// module was ready, oldest first.
#[wasm_bindgen]
pub fn run(queued: js_sys::Array) {
    agg_gui_whistle::worker::run(queued);
}
