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

## Other callers

An app's own microphone button (a chat input, say) drives a `Dictation` directly: the same lazy
load, microphone and worker, with the committed and pending text from each poll.

```rust,ignore
use agg_gui_whistle::{Dictation, DictationOptions};

let dictation = Dictation::new(agg_gui_whistle::Config::default());
// In the user's tap (the microphone prompt shows at once):
dictation.start(&DictationOptions { sentence_start: true, full_pass: true, ..Default::default() });
// Each frame or timer tick:
let poll = dictation.poll(); // state, text, finished, full_pass, error
```

`DictationOptions::full_pass` keeps the recording and, after the stream's final text, reads it
once more with whistle's offline options (3 beams, the whole recording around every word); the
poll that ends the dictation (`finished`) carries it as `FullPass`, for the app to weigh against
the streamed words. On the web it took 171 ms for a 5 s clip and 316 ms for a 6.4 s one
(Chrome, simd128, one thread, Apple M-series). `DictationPoll::error` says what went wrong
(`DictationError`), so the app can word its own message; a microphone lost mid-dictation
(unplugged, switched, its track ended) is `MicLost` at once, while the words heard until then
are finished and delivered. `Dictation::unsupported` says, before any tap, why a platform can't
dictate at all. The keyboard's `WhistleVoice` is a thin adapter over `Dictation`, without the
full pass.

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

With the `native-mic` feature, agg-mic (this workspace's microphone: AVAudioEngine on macOS, WASAPI
on Windows, the PulseAudio protocol on Linux, all pure Rust) records the default input device and an
engine thread runs the model. On macOS a bundled app needs `NSMicrophoneUsageDescription` in its
Info.plist (agg-mic's docs say what else). The model comes from `Config::model_path`, else the cache directory,
else (feature `native-download`) a download into it, checked like the web's. Without `native-mic`
a tap on the key says that the build has no microphone.

## License

MIT (`LICENSE`). whistle-rust and the Whistle weights are Apache-2.0 (Cactus Compute, Inc.): an
app that ships this crate or lets it download the model includes `LICENSE-APACHE` and
`THIRD_PARTY_NOTICES.md`.
