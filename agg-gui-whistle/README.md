# agg-gui-whistle

On-device dictation for [agg-gui](https://github.com/larsbrubaker/agg-gui)'s on-screen keyboard,
on [whistle-rust](https://github.com/larsbrubaker/whistle-rust), a pure-Rust port of Cactus
Compute's Whistle speech-to-text model (English, German, French, Spanish, Italian, Dutch,
Polish). agg-gui knows nothing of whistle; an app opts in with one call on its UI thread:

```rust,ignore
agg_gui_whistle::install(agg_gui_whistle::Config::default());
```

The keyboard then shows a microphone key on every text field. Installing fetches only the
capture worklet's module (about a kilobyte); the model waits for the first tap on the key, which
asks for the microphone inside the tap (the browser's prompt shows at once) and downloads the
model (16.9 MB) once, checked against its pinned SHA-256 and cached, while the key shows the
progress. Speech is captured from the tap: what is said during the download is recognised once
the model is in, the text trailing the speech until that backlog is caught up (a second tap
meanwhile cancels the dictation and drops it; the download carries on). Then the words appear
as you speak, with no second tap: the unconfirmed tail as composition text, committed words as
they are confirmed. An app that wants the model warm sooner
calls `VoiceInput::prepare` on a `WhistleVoice` before registering it with `set_voice_input`.

## Web

The model runs in a dedicated Web Worker, never on the UI thread. Serve one directory
(`Config::worker_url` names it, default `whistle/` beside the page) holding:

| file | from |
|---|---|
| `worker.js`, `capture-worklet.js` | this crate's `web/` |
| `agg_gui_whistle_worker.js`, `agg_gui_whistle_worker_bg.wasm` | `agg-gui-whistle-worker`, built with simd128 |
| `whistle.cact` (optional) | whistle-rust's `scripts/fetch-weights.sh`; otherwise fetched from whistle-rust's GitHub Pages copy, then Hugging Face |

```sh
RUSTFLAGS="-C target-feature=+simd128" CARGO_TARGET_DIR=target/wasm-simd128 \
  wasm-pack build agg-gui-whistle-worker --release --target web --out-dir <dir> --no-typescript
cp agg-gui-whistle/web/worker.js agg-gui-whistle/web/capture-worklet.js <dir>/
```

Build the app's own (UI) module **without** simd128: rustc 1.99's LLVM fails to compile agg-gui
with it (whistle-rust's `demo/README.md`, "simd128 and the UI module"). agg-gui's
`demo/build-whistle.sh` is a complete example. The page must be a secure context (HTTPS or
localhost) for the microphone.

## Native

With the `native-mic` feature, cpal records the default input device on a recorder thread and an
engine thread runs the model. The model comes from `Config::model_path`, else the cache directory,
else (feature `native-download`) a download into it, checked like the web's. Without `native-mic`
a tap on the key says that the build has no microphone.

## License

MIT (`LICENSE`). whistle-rust and the Whistle weights are Apache-2.0 (Cactus Compute, Inc.): an
app that ships this crate or lets it download the model includes `LICENSE-APACHE` and
`THIRD_PARTY_NOTICES.md`.
