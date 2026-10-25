//! The provider's state machine, shared by the web and native hosts: what
//! the keyboard is told ([`Machine::poll`]) and what the host must do
//! ([`Command`]s), from the keyboard's calls, the engine's replies and the
//! microphone's events. Pure: no I/O, so every path is unit-tested
//! (`machine_tests.rs`).
//!
//! A host (`web/`, `native/`) feeds it events, then runs the commands it
//! queued, in order: messages for the engine (`protocol.rs`), and opening
//! or closing the microphone. Each dictation is a numbered session; the
//! engine's replies and the microphone's events name theirs, and those for
//! a session that is over are dropped, so a cancel or an aborted start
//! never lets late words or a late microphone through.
//!
//! The model loads on `prepare` (the keyboard coming up); the microphone
//! opens on `start` (the first tap), when the browser asks for permission.
//! A stop closes the microphone (sending its last audio), asks the engine
//! to finish, reports `Finishing`, delivers the final text with a
//! `Finishing` poll, then reports `Ready`.

use agg_gui::widgets::on_screen_keyboard::{VoiceContext, VoicePoll, VoiceState, VoiceText};

use crate::protocol::{FromWorker, LoadFailure, ToWorker};
use crate::text_map::TextMapper;

/// What the host must do.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Command {
    /// Send to the engine.
    Worker(ToWorker),
    /// Open the microphone for `session` and stream its audio to the engine
    /// as `Push`es; report [`Machine::on_mic_opened`] or
    /// [`Machine::on_mic_failed`].
    OpenMic { session: u32 },
    /// Close the microphone of `session` (or, if it is still opening,
    /// release it as soon as it opens). With `flush`, its last partial chunk
    /// goes to the engine first.
    CloseMic { session: u32, flush: bool },
}

/// Why the microphone did not open. Each host reports the reasons its
/// platform can tell apart, so some variants go unused on some.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MicError {
    /// The user (or a policy) refused permission.
    Denied,
    /// There is no input device.
    NoDevice,
    /// The platform has no microphone access (no `getUserMedia`, no HTTPS).
    Unsupported,
    /// Anything else; the detail is for the console.
    Other(String),
}

impl MicError {
    /// The message the keyboard shows, and whether a tap may retry.
    fn shown(&self) -> (String, bool) {
        match self {
            MicError::Denied => ("Microphone access was denied".into(), false),
            MicError::NoDevice => ("No microphone found".into(), true),
            MicError::Unsupported => ("This browser can't record audio".into(), false),
            MicError::Other(_) => ("Couldn't start the microphone".into(), true),
        }
    }
}

fn load_failure_message(reason: LoadFailure) -> &'static str {
    match reason {
        LoadFailure::Download => "Couldn't download the speech model",
        LoadFailure::Checksum => "The speech model download was damaged",
        LoadFailure::Model => "Couldn't load the speech model",
        LoadFailure::Startup => "Couldn't start speech recognition",
    }
}

const ENGINE_FAILED: &str = "Speech recognition failed";

#[derive(Debug, Clone, PartialEq)]
enum Model {
    /// Not asked for yet.
    Idle,
    Loading {
        progress: Option<f32>,
    },
    Loaded,
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dictation {
    Idle,
    /// The microphone is opening (perhaps behind a permission prompt).
    Opening {
        session: u32,
    },
    Listening {
        session: u32,
    },
    /// Stopped; `done` once the engine's final reply is in.
    Finishing {
        session: u32,
        done: bool,
    },
}

/// See the module docs.
pub(crate) struct Machine {
    urls: Vec<String>,
    /// The language when the keyboard names none ("" detects it).
    default_language: Option<String>,
    /// Set for good when the platform can't do dictation at all.
    unsupported: Option<String>,
    model: Model,
    dictation: Dictation,
    last_session: u32,
    mapper: TextMapper,
    level: Option<f32>,
    /// The last dictation's error, shown until the next start or prepare.
    error: Option<(String, bool)>,
    /// A start waiting for the model to load.
    start_when_loaded: Option<VoiceContext>,
    /// A poll has reported since the last start (an error that lands before
    /// one is held back a poll, so the keyboard sees the retry begin).
    polled_since_start: bool,
    commands: Vec<Command>,
}

impl Machine {
    /// A machine that loads the model from `urls` (the host's cache first)
    /// and recognises `language` when the keyboard names none.
    pub fn new(urls: Vec<String>, language: Option<String>) -> Machine {
        Machine {
            urls,
            default_language: language,
            unsupported: None,
            model: Model::Idle,
            dictation: Dictation::Idle,
            last_session: 0,
            mapper: TextMapper::default(),
            level: None,
            error: None,
            start_when_loaded: None,
            polled_since_start: true,
            commands: Vec::new(),
        }
    }

    /// A machine for a platform without dictation: it reports `message` as
    /// an error without retry and does nothing.
    pub fn unsupported(message: &str) -> Machine {
        let mut m = Machine::new(Vec::new(), None);
        m.unsupported = Some(message.to_string());
        m
    }

    /// The commands queued since the last call, in order.
    pub fn take_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.commands)
    }

    /// The session in progress (opening, listening or finishing).
    pub fn current_session(&self) -> Option<u32> {
        match self.dictation {
            Dictation::Idle => None,
            Dictation::Opening { session }
            | Dictation::Listening { session }
            | Dictation::Finishing { session, .. } => Some(session),
        }
    }

    fn load(&mut self) {
        if matches!(self.model, Model::Idle | Model::Failed { .. }) {
            self.model = Model::Loading { progress: None };
            self.commands.push(Command::Worker(ToWorker::Load {
                urls: self.urls.clone(),
            }));
        }
    }

    pub fn prepare(&mut self) {
        if self.unsupported.is_some() {
            return;
        }
        if self.dictation == Dictation::Idle {
            self.error = None;
        }
        self.load();
    }

    pub fn start(&mut self, ctx: &VoiceContext) {
        if self.unsupported.is_some() || self.dictation != Dictation::Idle {
            return;
        }
        self.error = None;
        self.polled_since_start = false;
        if self.model == Model::Loaded {
            self.begin(ctx);
        } else {
            self.start_when_loaded = Some(ctx.clone());
            self.load();
        }
    }

    /// Open a session: the engine's stream first, so no audio arrives
    /// before it, then the microphone.
    fn begin(&mut self, ctx: &VoiceContext) {
        self.last_session = self.last_session.wrapping_add(1);
        let session = self.last_session;
        self.mapper = TextMapper::new(ctx.sentence_start);
        self.level = None;
        self.dictation = Dictation::Opening { session };
        let language = ctx
            .language
            .clone()
            .or_else(|| self.default_language.clone())
            .unwrap_or_default();
        self.commands
            .push(Command::Worker(ToWorker::Start { session, language }));
        self.commands.push(Command::OpenMic { session });
    }

    /// End the session at once, dropping what it has not delivered.
    fn abort(&mut self) {
        self.start_when_loaded = None;
        match self.dictation {
            Dictation::Idle => {}
            Dictation::Opening { session } | Dictation::Listening { session } => {
                self.commands.push(Command::CloseMic {
                    session,
                    flush: false,
                });
                self.commands
                    .push(Command::Worker(ToWorker::Cancel { session }));
            }
            Dictation::Finishing { session, .. } => {
                self.commands
                    .push(Command::Worker(ToWorker::Cancel { session }));
            }
        }
        self.dictation = Dictation::Idle;
        self.mapper = TextMapper::default();
    }

    pub fn stop(&mut self) {
        match self.dictation {
            Dictation::Listening { session } => {
                self.commands.push(Command::CloseMic {
                    session,
                    flush: true,
                });
                self.commands
                    .push(Command::Worker(ToWorker::Finish { session }));
                self.dictation = Dictation::Finishing {
                    session,
                    done: false,
                };
            }
            // Nothing was heard yet: stopping is cancelling.
            Dictation::Opening { .. } | Dictation::Idle => self.abort(),
            Dictation::Finishing { .. } => {}
        }
    }

    pub fn cancel(&mut self) {
        self.abort();
    }

    pub fn on_mic_opened(&mut self, session: u32) {
        if self.dictation == (Dictation::Opening { session }) {
            self.dictation = Dictation::Listening { session };
            self.level = Some(0.0);
        } else {
            // An aborted start: release the microphone it opened.
            self.commands.push(Command::CloseMic {
                session,
                flush: false,
            });
        }
    }

    pub fn on_mic_failed(&mut self, session: u32, err: MicError) {
        if self.dictation != (Dictation::Opening { session }) {
            return;
        }
        if let MicError::Other(detail) = &err {
            log(&format!("microphone: {detail}"));
        }
        self.dictation = Dictation::Idle;
        self.error = Some(err.shown());
        self.commands
            .push(Command::Worker(ToWorker::Cancel { session }));
    }

    /// The input level, 0..=1.
    pub fn on_level(&mut self, session: u32, level: f32) {
        if self.dictation == (Dictation::Listening { session }) {
            self.level = Some(level.clamp(0.0, 1.0));
        }
    }

    /// The engine (the worker, or the native thread) is gone.
    pub fn worker_lost(&mut self, detail: &str) {
        log(&format!("engine lost: {detail}"));
        if self.current_session().is_some() {
            self.abort();
            // Its cancel has nowhere to go.
            self.commands
                .retain(|c| !matches!(c, Command::Worker(ToWorker::Cancel { .. })));
            self.error = Some((ENGINE_FAILED.into(), true));
        }
        self.start_when_loaded = None;
        self.model = Model::Failed {
            message: load_failure_message(LoadFailure::Startup).into(),
        };
    }

    pub fn on_worker(&mut self, msg: FromWorker) {
        match msg {
            FromWorker::Progress { loaded, total } => {
                if let Model::Loading { progress } = &mut self.model {
                    if total > 0 {
                        *progress = Some((loaded as f32 / total as f32).min(1.0));
                    }
                }
            }
            FromWorker::Loaded { .. } => {
                self.model = Model::Loaded;
                if let Some(ctx) = self.start_when_loaded.take() {
                    self.begin(&ctx);
                }
            }
            FromWorker::LoadFailed { reason, detail } => {
                log(&format!("model: {detail}"));
                self.start_when_loaded = None;
                self.model = Model::Failed {
                    message: load_failure_message(reason).into(),
                };
            }
            FromWorker::Update { session, update } => {
                if self.receiving(session) {
                    self.mapper.update(&update);
                }
            }
            FromWorker::Final { session, update } => {
                if self.dictation
                    == (Dictation::Finishing {
                        session,
                        done: false,
                    })
                {
                    self.mapper.update(&update);
                    self.mapper.finish();
                    self.dictation = Dictation::Finishing {
                        session,
                        done: true,
                    };
                }
            }
            FromWorker::Failed { session, detail } => {
                if !self.receiving(session) && self.dictation != (Dictation::Opening { session }) {
                    return;
                }
                log(&format!("engine: {detail}"));
                if !matches!(self.dictation, Dictation::Finishing { .. }) {
                    self.commands.push(Command::CloseMic {
                        session,
                        flush: false,
                    });
                }
                self.dictation = Dictation::Idle;
                self.error = Some((ENGINE_FAILED.into(), true));
            }
        }
    }

    /// The session's updates still count (listening, or finishing before
    /// the final reply).
    fn receiving(&self, session: u32) -> bool {
        matches!(self.dictation,
            Dictation::Listening { session: s } | Dictation::Finishing { session: s, done: false }
                if s == session)
    }

    fn state(&self) -> VoiceState {
        if let Some(message) = &self.unsupported {
            return VoiceState::Error {
                message: message.clone(),
                retry: false,
            };
        }
        match self.dictation {
            Dictation::Opening { .. } => return VoiceState::Preparing { progress: None },
            Dictation::Listening { .. } => return VoiceState::Listening { level: self.level },
            Dictation::Finishing { .. } => return VoiceState::Finishing,
            Dictation::Idle => {}
        }
        if let Some((message, retry)) = &self.error {
            return VoiceState::Error {
                message: message.clone(),
                retry: *retry,
            };
        }
        match &self.model {
            Model::Idle => VoiceState::Preparing { progress: None },
            Model::Loading { progress } => VoiceState::Preparing {
                progress: *progress,
            },
            Model::Loaded => VoiceState::Ready,
            Model::Failed { message } => VoiceState::Error {
                message: message.clone(),
                retry: true,
            },
        }
    }

    /// The state and the text since the last poll. A finished dictation's
    /// final text goes out with a `Finishing` poll; the next reports the
    /// state after it.
    pub fn poll(&mut self) -> VoicePoll {
        let mut state = self.state();
        if !self.polled_since_start && matches!(state, VoiceState::Error { .. }) {
            state = VoiceState::Preparing { progress: None };
        }
        self.polled_since_start = true;
        let text = self.mapper.take().map(|d| VoiceText {
            committed: d.committed,
            pending: d.pending,
        });
        if let Dictation::Finishing { done: true, .. } = self.dictation {
            self.dictation = Dictation::Idle;
        }
        VoicePoll { state, text }
    }
}

/// A detail for the developer console (the keyboard shows the short
/// message).
fn log(message: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::warn_1(&format!("agg-gui-whistle: {message}").into());
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("agg-gui-whistle: {message}");
}

#[cfg(test)]
#[path = "machine_tests.rs"]
mod tests;
