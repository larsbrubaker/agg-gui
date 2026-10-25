# agg-mic

The system microphone in pure Rust: open the default input device, receive its audio as mono `f32`
chunks (-1 to 1) with their sample rate while it records, stop with every chunk delivered. No
agg-gui, wgpu or UI; no `-sys` crates and no build scripts.

| Platform | Capture | Rate of the chunks |
|---|---|---|
| macOS | AVFoundation's AVAudioEngine, its input into an AVAudioSinkNode (objc2) | the device's own |
| Windows | WASAPI, shared mode (Microsoft's `windows` bindings) | 16 kHz (the audio engine converts) |
| Linux | the PulseAudio native protocol (`pulseaudio`; PulseAudio or PipeWire's pulse server) | 16 kHz (the server converts) |
| elsewhere, wasm32 included | none: `is_available()` is false, `start` fails with `MicError::Unsupported` | |

```rust
use agg_mic::Microphone;

// Listed by the sound server under the executable's name; `Microphone::with_app_name` names it.
let microphone = Microphone::default_input();
// Blocks until audio flows (or a permission prompt is answered): call it off the UI thread.
microphone.start(
    // FnMut, called one chunk at a time; dropped inside the stop that ends the recording.
    Box::new(|samples: &[f32], rate: u32| { /* hand on, e.g. over a channel */ }),
    // FnOnce: a recording that ends on its own (unplugged) says so once.
    Box::new(|error| eprintln!("the microphone stopped: {error}")),
).expect("the microphone started");
// ...
microphone.stop(); // every chunk has been delivered, and the handlers dropped, when this returns
```

Resampling is the consumer's job (whistle-rust's `whistle::audio::StreamResampler` does it across
chunks); so are files and meters.

**macOS permission.** A bundled app needs `NSMicrophoneUsageDescription` in its Info.plist and,
under the hardened runtime, the `com.apple.security.device.audio-input` entitlement. An unbundled
run is attributed to the app that launched it (Terminal, the IDE).

**What a stop can lose** is the audio of the I/O cycle in flight: about 10 ms on macOS (one
AVAudioSinkNode cycle) and Windows (one 10 ms poll), one 20 ms fragment on Linux.

**A stop is quick**, since agg-gui-whistle stops on the UI thread. It returns once no chunk can
arrive any more, and every chunk delivered by then is the consumer's.
- macOS hands `AVAudioEngine.stop()` and the sink node's detach (tens of ms) to a teardown thread,
  which the next start waits for.
- Windows wakes its capture thread at once and waits, at most 200 ms, only for its last drain; COM
  teardown happens after.
- Linux joins a thread that only has its socket left to close.

`AGG_MIC_TRACE=1` prints each step's time.

**Live tests.** `AGG_LIVE_MIC_TESTS=1 cargo test -p agg-mic --test live_mic -- --ignored --nocapture`
records a second from the real microphone; the live tests take turns on the microphone. It prints how long before the stop the last chunk
arrived, how long the stop took (with its breakdown), and fails if either is too long (40 ms and 16 ms). With
`AGG_MIC_UNPLUG_TEST=1` as well, it also waits for the microphone to be unplugged and checks that
the recording ends with an error.

The recorders come from mattercad-rust's `mattercad-ai` (`src/audio/`), by the same author. MIT
(`LICENSE`); the platform crates' notices are in `THIRD_PARTY_NOTICES.md`.
