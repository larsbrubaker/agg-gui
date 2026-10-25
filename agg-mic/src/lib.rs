//! agg-mic: the system microphone, in pure Rust. A [`Microphone`] opens the default input device,
//! hands its audio to a [`ChunkHandler`] as mono `f32` chunks (-1 to 1) with their sample rate
//! while it records, and stops when asked, every chunk delivered by the time [`Microphone::stop`]
//! returns. It is all a consumer needs to feed speech-to-text that transcribes as the user speaks
//! (agg-gui-whistle's dictation, whistle-rust's desktop demo) or to keep a recording (MatterCAD's
//! voice input). Converting the rate is the consumer's job (whistle-rust's
//! `whistle::audio::StreamResampler` does it across chunks), so nothing here resamples audio
//! nobody asked for; nor does it keep a file or measure a level.
//!
//! The platform captures sit behind [`Backend`]: `mac_capture` (AVFoundation's AVAudioEngine, its
//! input into an AVAudioSinkNode, at the device's own rate), `windows_capture` (WASAPI, converted
//! by the audio engine to 16 kHz, or the device's own format where it won't convert) and
//! `linux_capture` (the PulseAudio native protocol, read through `pulse_frames`, converted by the
//! sound server to 16 kHz); `mixdown` brings a device's own layout to mono. A stop can lose the
//! audio of the last I/O cycle in flight: about 10 ms on macOS and Windows, a 20 ms fragment on
//! Linux. Elsewhere, wasm32 included, [`Microphone::default_input`] is one that
//! [`Microphone::is_available`] says is missing and whose start fails with
//! [`MicError::Unsupported`]. `delivery` is how a capture hands its audio on, and `microphone`
//! the one-recording-at-a-time state over a backend; a test passes its own backend to
//! [`Microphone::with_backend`]. On macOS `mac_engine_change` hears the engine stop when its input
//! goes away, so every platform reports a microphone unplugged mid-recording.
//!
//! Extracted from mattercad-rust's mattercad-ai (`src/audio/`: `mac_recorder.rs`,
//! `windows_recorder.rs`, `linux_recorder.rs`, `pulse_frames.rs`, `chunks.rs`), the same author's,
//! MIT. MatterCAD's own recorder (its duration cap, its WAV for the cloud, its meter) builds on
//! this crate's chunks.
//!
//! **Permission** is asked by [`Microphone::start`], which therefore can block for as long as the
//! user leaves a prompt up: call it off the UI thread. On macOS the system asks once and remembers;
//! a bundled app needs `NSMicrophoneUsageDescription` in its Info.plist and, under the hardened
//! runtime, the `com.apple.security.device.audio-input` entitlement (without it the process gets
//! silence and no prompt). An unbundled run (`cargo run`) is attributed to the app that launched it
//! (Terminal, the IDE), whose grant every program it starts shares; if that app's own Info.plist
//! lacks the usage description, macOS may end the process instead of prompting. Windows has no
//! prompt for desktop apps, only the privacy setting; Linux has none on a desktop, and a sandbox
//! (Flatpak) without microphone access is refused by the sound server.

mod delivery;
#[cfg(target_os = "linux")]
mod linux_capture;
#[cfg(target_os = "macos")]
mod mac_capture;
#[cfg(target_os = "macos")]
mod mac_engine_change;
mod microphone;
#[cfg(any(target_os = "macos", target_os = "windows", test))]
mod mixdown;
#[cfg(any(target_os = "linux", test))]
mod pulse_frames;
#[cfg(any(target_os = "macos", test))]
mod teardown;
#[cfg(target_os = "windows")]
mod windows_capture;

use std::fmt;
use std::time::Duration;

/// Whether `AGG_MIC_TRACE=1`: a stop prints how long each of its steps took (the live test sets
/// it). Read at each stop, which is rare.
pub(crate) fn tracing() -> bool {
    std::env::var_os("AGG_MIC_TRACE").is_some_and(|value| value == "1")
}

/// `duration` in milliseconds, for a trace.
pub(crate) fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

pub use delivery::Delivery;
pub use microphone::{Backend, Microphone};

/// The macOS microphone permission and default input device, read without prompting.
#[cfg(target_os = "macos")]
pub mod macos {
    pub use crate::mac_capture::{
        authorization_status, default_input_device, InputDevice, AUTHORIZATION_AUTHORIZED,
        AUTHORIZATION_DENIED, AUTHORIZATION_NOT_DETERMINED, AUTHORIZATION_RESTRICTED,
    };
}

/// Takes the captured audio while a recording runs: mono samples (-1 to 1) and their rate in Hz.
/// Called on the capture's own thread (on macOS a real-time audio thread), one call at a time and
/// in capture order (hence `FnMut`: it may keep its own buffer), so it hands the samples on (a
/// channel send) and returns at once. It is dropped inside the [`Microphone::stop`] that ends the
/// recording (or when the recording fails, before the [`FailureHandler`] runs), so state it
/// flushes on drop is out by then. It must not call back into the [`Microphone`]; a panic in it
/// ends the recording with [`HANDLER_PANICKED_MESSAGE`].
pub type ChunkHandler = Box<dyn FnMut(&[f32], u32) + Send>;

/// Told when a recording ends on its own (the microphone unplugged, the sound server gone): at
/// most once per recording, on the capture's thread, never after [`Microphone::stop`] returns and
/// never for a stop. No chunk follows it. Like a [`ChunkHandler`] it must not call back into the
/// [`Microphone`].
pub type FailureHandler = Box<dyn FnOnce(MicError) + Send>;

/// The start refusal where there is no microphone support (wasm32, or an OS without a capture).
pub const UNSUPPORTED_MESSAGE: &str =
    "Recording from the microphone isn't available on this platform.";

/// A second start while a recording runs.
pub const ALREADY_RUNNING_MESSAGE: &str = "The microphone is already recording.";

/// What [`MicError::Stopped`] says.
pub const STOPPED_MESSAGE: &str = "The microphone was stopped before it started.";

/// The text for a recording ended because its chunk handler panicked (a bug in the consumer).
pub const HANDLER_PANICKED_MESSAGE: &str =
    "The recording stopped because of an error in the app's audio handling.";

/// The text for a microphone that would not start, shared by every capture.
pub const NO_INPUT_DEVICE_MESSAGE: &str =
    "The microphone could not start recording. Check that an input device is connected.";

/// The text for a microphone that went away while recording, shared by every capture.
pub const DISCONNECTED_MESSAGE: &str = "The microphone was disconnected while recording.";

/// The macOS text when the engine stopped mid-recording because its input went away: the
/// microphone unplugged, the default input switched to another device, or its format changed.
pub const INPUT_CHANGED_MESSAGE: &str =
    "The microphone was disconnected or changed while recording.";

/// The permission refusal on macOS: where a Mac user turns the microphone back on. Here rather
/// than in `mac_capture` so an app's translation check sees it on every platform.
pub const MAC_PERMISSION_DENIED_MESSAGE: &str = "Microphone access is turned off. Turn it on in System Settings > Privacy & Security > Microphone (for this app, or for the app it was started from, such as Terminal), then try again.";

/// The macOS refusal when a policy (Screen Time, or a device management profile) blocks the
/// microphone: the user can't turn it on in System Settings themselves.
pub const MAC_RESTRICTED_MESSAGE: &str = "Microphone access is blocked on this Mac by a policy, such as Screen Time or one your organization manages. Ask whoever manages this Mac to allow it.";

/// The permission refusal on Windows: where a Windows user lets desktop apps use the microphone.
pub const WINDOWS_PERMISSION_DENIED_MESSAGE: &str = "Microphone access is turned off. Turn it on in Settings > Privacy & security > Microphone (with \"Let desktop apps access your microphone\" on), then try again.";

/// The permission refusal on Linux: the sound server said no, which on a desktop means a sandbox
/// without microphone access.
pub const LINUX_PERMISSION_DENIED_MESSAGE: &str = "The sound server refused access to the microphone. If this app runs in a sandbox such as Flatpak, allow it to use the microphone, then try again.";

/// The Linux text when no PulseAudio or PipeWire server answers.
pub const LINUX_NO_SOUND_SERVER_MESSAGE: &str =
    "No sound server (PulseAudio or PipeWire) is running, so the microphone can't be reached.";

/// The Linux text when the sound server accepts the connection but never answers.
pub const LINUX_SERVER_TIMEOUT_MESSAGE: &str =
    "The sound server (PulseAudio or PipeWire) did not answer, so the microphone can't be reached.";

/// Every English text this crate hands an app to show, for its translation check.
pub const DISPLAY_TEXTS: &[&str] = &[
    UNSUPPORTED_MESSAGE,
    ALREADY_RUNNING_MESSAGE,
    STOPPED_MESSAGE,
    HANDLER_PANICKED_MESSAGE,
    NO_INPUT_DEVICE_MESSAGE,
    DISCONNECTED_MESSAGE,
    INPUT_CHANGED_MESSAGE,
    MAC_PERMISSION_DENIED_MESSAGE,
    MAC_RESTRICTED_MESSAGE,
    WINDOWS_PERMISSION_DENIED_MESSAGE,
    LINUX_PERMISSION_DENIED_MESSAGE,
    LINUX_NO_SOUND_SERVER_MESSAGE,
    LINUX_SERVER_TIMEOUT_MESSAGE,
];

/// Why a recording would not start, or ended on its own. Each message is ready to show (English).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MicError {
    /// The user (or a policy, or a sandbox) refused the microphone. The message says where to turn
    /// it back on.
    PermissionDenied(String),
    /// There is no input device, or it went away while recording.
    NoDevice(String),
    /// This platform has no microphone support ([`UNSUPPORTED_MESSAGE`]).
    Unsupported(String),
    /// [`Microphone::stop`] came while the start was still waiting (on a permission prompt, or the
    /// device opening); nothing is recording.
    Stopped,
    /// Anything else, such as a recording already running or the sound server missing.
    Failed(String),
}

impl MicError {
    /// The message to show.
    pub fn message(&self) -> &str {
        match self {
            Self::PermissionDenied(message)
            | Self::NoDevice(message)
            | Self::Unsupported(message)
            | Self::Failed(message) => message,
            Self::Stopped => STOPPED_MESSAGE,
        }
    }
}

impl fmt::Display for MicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for MicError {}
