//! # agg-gui-whistle
//!
//! On-device dictation for agg-gui's on-screen keyboard, on
//! [whistle-rust](https://github.com/larsbrubaker/whistle-rust) (a pure-Rust
//! port of Cactus Compute's Whistle speech-to-text model). An app opts in
//! with one call, on its UI thread:
//!
//! ```ignore
//! agg_gui_whistle::install(agg_gui_whistle::Config::default());
//! ```
//!
//! and the keyboard's microphone key dictates into every text field. agg-gui
//! itself knows nothing of whistle: this crate implements its engine-neutral
//! `VoiceInput` ([`WhistleVoice`]) and registers it.
//!
//! ## Web
//!
//! The model runs in a dedicated Web Worker, never on the UI thread. The app
//! serves, from one directory (`Config::worker_url` names it):
//!
//! - `web/worker.js` and `web/capture-worklet.js` from this crate;
//! - the worker module, `agg-gui-whistle-worker` built with
//!   `RUSTFLAGS="-C target-feature=+simd128" wasm-pack build
//!   agg-gui-whistle-worker --target web` (the UI's own module must be built
//!   *without* simd128; see that crate);
//! - optionally `whistle.cact` itself; otherwise it comes from
//!   whistle-rust's GitHub Pages copy or Hugging Face
//!   ([`Config::model_urls`]).
//!
//! Installing fetches only the capture worklet's module (about a kilobyte),
//! and nothing more downloads when the keyboard comes up. The first tap on
//! the microphone asks for the microphone inside the tap (the prompt shows
//! at once; that stream is the dictation's) and has the worker download the
//! model (16.9 MB, once: it is checked against its pinned SHA-256 and kept
//! in the Cache API) while the key shows the progress. What is said
//! meanwhile waits in the worker and is recognised once the model is in,
//! when the key listens, with no second tap; the text then catches up,
//! trailing the speech by as long as recognising that backlog takes. To have the model warm before that tap,
//! call `VoiceInput::prepare` on a [`WhistleVoice`] before registering it.
//! Speech streams to the worker, whose partial results fill the field as the
//! keyboard's composition text, committed words as they are confirmed.
//! agg-gui's `demo/build-wasm.sh` shows the whole build.
//!
//! ## Native
//!
//! With the `native-mic` feature (agg-mic, this workspace's pure-Rust
//! microphone), a thread per session opens the default input device and an
//! engine thread runs the model. The model comes
//! from [`Config::model_path`], else the cache directory, else (feature
//! `native-download`) a download into it. Without `native-mic` a tap on the key
//! says that there is no microphone.
//!
//! ## Layout
//!
//! - `protocol.rs`: the messages between provider and engine; `engine.rs`:
//!   the model and its stream, answering them (web worker and native thread
//!   alike); `resample.rs`: the device's audio to the model's 16 kHz, one
//!   whistle `StreamResampler` per session; `text_map.rs`: stream updates to the keyboard's committed and
//!   pending text; `model_check.rs`: the model's size and checksum.
//! - `machine.rs`: the provider's state machine, shared by both hosts;
//!   `provider.rs`: the `VoiceInput` over it.
//! - `web/` (the worker and the microphone, on the UI thread; `drain.rs`
//!   delivers the worklet's last audio at a stop), `worker/`
//!   (inside the worker), `native/` (threads and agg-mic).
//!
//! ## License
//!
//! whistle-rust and the Whistle weights are Apache-2.0 (Cactus Compute,
//! Inc.); an app that ships or downloads the model includes the Apache-2.0
//! license and `THIRD_PARTY_NOTICES.md` (beside this crate's Cargo.toml).

#[cfg(feature = "provider")]
mod config;
#[cfg(any(all(feature = "provider", target_arch = "wasm32"), test))]
mod drain_book;
#[cfg(any(
    all(feature = "worker", target_arch = "wasm32"),
    all(feature = "provider", not(target_arch = "wasm32"))
))]
mod engine;
#[cfg(target_arch = "wasm32")]
mod js_fields;
#[cfg(feature = "provider")]
mod machine;
#[cfg(any(
    all(feature = "worker", target_arch = "wasm32"),
    all(feature = "provider", not(target_arch = "wasm32"))
))]
mod model_check;
#[cfg(all(feature = "provider", not(target_arch = "wasm32")))]
mod native;
#[cfg(any(all(feature = "provider", target_arch = "wasm32"), test))]
mod processor_clock;
pub mod protocol;
#[cfg(feature = "provider")]
mod provider;
#[cfg(any(
    all(feature = "worker", target_arch = "wasm32"),
    all(feature = "provider", not(target_arch = "wasm32"))
))]
mod resample;
#[cfg(feature = "provider")]
mod text_map;
#[cfg(all(feature = "provider", target_arch = "wasm32"))]
mod web;
#[cfg(all(feature = "worker", target_arch = "wasm32"))]
pub mod worker;

#[cfg(feature = "provider")]
pub use config::{Config, HUGGING_FACE_MODEL_URL, WHISTLE_PAGES_MODEL_URL};
#[cfg(feature = "provider")]
pub use provider::WhistleVoice;

/// Register whistle dictation with the on-screen keyboard of this UI thread
/// (call it on that thread, once). On the web it fetches the capture
/// worklet's module (about a kilobyte); the worker and the model wait for the
/// first tap on the microphone key.
#[cfg(feature = "provider")]
pub fn install(config: Config) {
    agg_gui::widgets::on_screen_keyboard::set_voice_input(Some(Box::new(WhistleVoice::new(
        config,
    ))));
}
