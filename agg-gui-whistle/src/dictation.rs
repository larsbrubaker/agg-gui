//! [`Dictation`]: whistle dictation for a caller other than the on-screen
//! keyboard (an app's own microphone button, a chat input). The keyboard's
//! [`crate::WhistleVoice`] is a thin adapter over it; both drive the same
//! state machine (`machine.rs`) through the platform's host (`web/` or
//! `native/`), so everything the keyboard gets comes here too: the model
//! loaded lazily on the first start (or an early [`Dictation::prepare`]),
//! the microphone asked for inside the start (call it from the user's tap,
//! so the browser's prompt shows at once), audio captured from that moment
//! on and recognised once the model is in, and "listening" only once the
//! audio flows.
//!
//! What a caller adds over the keyboard: an optional *full pass*
//! ([`DictationOptions::full_pass`]): after the stream's final text, the
//! engine reads the whole recording once more with whistle's offline
//! options, and the poll that ends the dictation carries that reading
//! ([`FullPass`]) for the caller to weigh against the streamed words; and
//! the structured reason a dictation failed or ended early
//! ([`DictationError`]), so the caller can word its own message.
//!
//! Everything runs on the UI thread and returns at once; results come
//! through [`Dictation::poll`], which the caller calls as often as it wants
//! fresh text (each frame, or on a timer). Replies wake the app with
//! `agg_gui::animation::request_draw`.

use agg_gui::widgets::on_screen_keyboard::{VoiceContext, VoicePoll, VoiceState, VoiceText};

use crate::config::Config;
use crate::provider::{Host, PlatformHost};

/// How a dictation starts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DictationOptions {
    /// The language to recognise (`"en"`, ...), or `None` for the
    /// [`Config::language`] (which `None` there detects).
    pub language: Option<String>,
    /// The words start a sentence: keep the model's capital on the first
    /// one. When false the first word is lowercased (unless it is "I", an
    /// acronym or the like), for text inserted mid-sentence.
    pub sentence_start: bool,
    /// Keep the recording and, after the stream's final text, read it once
    /// more as a whole ([`FullPass`]). Costs the recording's memory (64 KB
    /// a second) and the pass's time at the stop.
    pub full_pass: bool,
}

impl From<&VoiceContext> for DictationOptions {
    fn from(ctx: &VoiceContext) -> DictationOptions {
        DictationOptions {
            language: ctx.language.clone(),
            sentence_start: ctx.sentence_start,
            full_pass: false,
        }
    }
}

impl From<&DictationOptions> for DictationOptions {
    fn from(options: &DictationOptions) -> DictationOptions {
        options.clone()
    }
}

/// What the full pass over a finished dictation's recording came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FullPass {
    /// Its text, trimmed (empty when it heard no speech), and whether
    /// whistle's repetition guard fired on its decode (the audio was likely
    /// garbled, so the pass may have lost words: `whistle::Transcript`).
    Read { text: String, repeated: bool },
    /// It could not read the recording; `detail` is for a log.
    Failed { detail: String },
}

/// Why a dictation failed, or ended before its stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictationError {
    /// The user (or a policy) refused the microphone; asking again (the
    /// next start) prompts or refuses at once.
    MicDenied,
    /// There is no microphone.
    NoMic,
    /// The platform has no microphone access (no `getUserMedia`, a page
    /// that isn't a secure context).
    MicUnsupported,
    /// The audio would not start (iOS starts it only inside a tap); the
    /// next start, from a tap, is one.
    MicSuspended,
    /// The microphone would not start for another reason.
    MicFailed,
    /// The microphone went away mid-dictation (unplugged, switched, its
    /// stream ended). Reported at once, while what was heard until then is
    /// finished: the dictation still ends with its text (and full pass).
    MicLost,
    /// The model could not be downloaded.
    ModelDownload,
    /// The model downloaded, but not as the file expected (size or SHA-256).
    ModelDamaged,
    /// The model checked out but could not be built.
    ModelLoad,
    /// The recogniser could not start (its worker or module failed to load).
    EngineStartup,
    /// Recognition failed mid-dictation.
    Engine,
    /// This platform can't dictate at all, for the reason given (no
    /// WebAssembly SIMD, no workers, no microphone API). Known before any
    /// start: [`Dictation::unsupported`].
    Unsupported(String),
}

impl DictationError {
    /// A short message for the user, as the keyboard shows it.
    pub fn message(&self) -> &str {
        match self {
            DictationError::MicDenied => "Microphone access was denied",
            DictationError::NoMic => "No microphone found",
            DictationError::MicUnsupported => "This browser can't record audio",
            DictationError::MicSuspended => "Couldn't start the audio; tap to try again",
            DictationError::MicFailed => "Couldn't start the microphone",
            DictationError::MicLost => "The microphone was disconnected",
            DictationError::ModelDownload => "Couldn't download the speech model",
            DictationError::ModelDamaged => "The speech model download was damaged",
            DictationError::ModelLoad => "Couldn't load the speech model",
            DictationError::EngineStartup => "Couldn't start speech recognition",
            DictationError::Engine => "Speech recognition failed",
            DictationError::Unsupported(message) => message,
        }
    }

    /// Whether starting again may work.
    pub fn retry(&self) -> bool {
        !matches!(
            self,
            DictationError::MicUnsupported | DictationError::Unsupported(_)
        )
    }
}

/// What [`Dictation::poll`] reports.
#[derive(Debug, Clone, PartialEq)]
pub struct DictationPoll {
    /// The state, as the keyboard sees it: `Preparing` (with the model's
    /// download progress) until the model is in and the audio flows, then
    /// `Listening` (with the input level), `Finishing` after the stop, and
    /// `Error` once a failed dictation is over.
    pub state: VoiceState,
    /// The text since the last poll, as the keyboard inserts it: new
    /// committed words (spaced to follow those before) and the whole
    /// unconfirmed tail. `None` when nothing changed.
    pub text: Option<VoiceText>,
    /// This poll ends the dictation: its text is the last, and with
    /// [`DictationOptions::full_pass`] its `full_pass` is set.
    pub finished: bool,
    /// The full pass, on the poll that ends a full-pass dictation.
    pub full_pass: Option<FullPass>,
    /// What went wrong, if anything, since the last start: a failed start
    /// (with `state` an `Error`), or [`DictationError::MicLost`] at once,
    /// while the dictation still finishes what it heard.
    pub error: Option<DictationError>,
}

impl From<DictationPoll> for VoicePoll {
    fn from(poll: DictationPoll) -> VoicePoll {
        VoicePoll {
            state: poll.state,
            text: poll.text,
        }
    }
}

/// See the module docs. Create it on the UI thread; dropping it releases
/// the microphone and (on the web) terminates the worker.
pub struct Dictation {
    host: PlatformHost,
}

impl Dictation {
    pub fn new(config: Config) -> Dictation {
        Dictation {
            host: PlatformHost::new(config),
        }
    }

    /// Why this platform can't dictate at all (the web: no `getUserMedia`
    /// or secure context, no module workers, no WebAssembly SIMD; natively:
    /// a build without the microphone), so a caller can leave its
    /// microphone button out; `None` when it can try.
    pub fn unsupported(&self) -> Option<String> {
        let mut out = None;
        self.host
            .dispatch(&mut |m| out = m.unsupported_reason().map(str::to_string));
        out
    }

    /// Load the model ahead of the first start. Optional: a start loads it.
    pub fn prepare(&self) {
        self.host.dispatch(&mut |m| m.prepare());
    }

    /// Start a dictation (does nothing while one is in progress). Call it
    /// inside the user's tap: it asks for the microphone at once.
    pub fn start(&self, options: &DictationOptions) {
        self.host.dispatch(&mut |m| m.start(options));
    }

    /// Stop listening and finish: the text heard so far is recognised and
    /// ends the dictation (after its full pass, if asked for). A stop before
    /// anything could be recognised (still preparing) is a cancel.
    pub fn stop(&self) {
        self.host.dispatch(&mut |m| m.stop());
    }

    /// End the dictation at once, dropping what it has not delivered.
    pub fn cancel(&self) {
        self.host.dispatch(&mut |m| m.cancel());
    }

    /// The state and what arrived since the last poll.
    pub fn poll(&self) -> DictationPoll {
        let mut out = None;
        self.host.dispatch(&mut |m| out = Some(m.poll_dictation()));
        // `dispatch` always runs `f`; the fallback only satisfies the type.
        out.unwrap_or(DictationPoll {
            state: VoiceState::Preparing { progress: None },
            text: None,
            finished: false,
            full_pass: None,
            error: None,
        })
    }
}
