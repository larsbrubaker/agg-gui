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
//! Nothing loads until the first `start` (the first tap on the microphone;
//! the keyboard never calls `prepare`, which an app may call to warm the
//! model early). Until then the machine reports `Ready`. Every start opens
//! its session at once, inside the tap: the load if the model isn't in
//! (`Load`), the engine's stream (`Start`), then the microphone (`OpenMic`,
//! the one microphone request of the dictation, so the permission prompt
//! shows at the tap). The engine works through its messages in order and
//! the load first, so the stream and the audio captured meanwhile wait in
//! its queue until the model is in (`protocol::cap_backlog` keeps the
//! latest 30 s of a long wait). Until then the machine reports `Preparing`
//! with the load's progress, then `Listening`, with no second tap. Listening
//! (the key's "Speak now") waits for the session's first audio to reach the
//! pipeline (its first level), not just for the microphone to open: a
//! starved browser may deliver the stream seconds after the device starts,
//! and the cue must not come before what is said after it is kept. A stop
//! or cancel meanwhile ends the session (the microphone closed, its queued
//! audio skipped by the engine) but not the load, so the model still
//! arrives and the next start begins at once. A load with no session
//! waiting for it (an app's `prepare`, or one whose session was cancelled)
//! reports `Ready`, and fails quietly: the next tap loads afresh. A load a
//! session waits for that fails ends the session with the error, one a
//! start retries. Once loaded, the model stays loaded. A refused microphone
//! is an error a tap retries: the browser is asked again.
//!
//! A stop closes the microphone (sending its last audio), asks the engine
//! to finish, reports `Finishing`, delivers the final text with a
//! `Finishing` poll, then reports `Ready`. A dictation started with
//! `full_pass` (`dictation.rs`; never the keyboard's) also asks the engine
//! for the full pass after the finish, and stays `Finishing` until it is
//! in: the stream's final text is delivered as soon as it arrives, and the
//! pass with the poll that ends the dictation. A microphone lost while
//! listening is reported at once ([`DictationError::MicLost`] in
//! [`Machine::poll_dictation`]) and ends the dictation like a stop.

#[cfg(test)]
use agg_gui::widgets::on_screen_keyboard::VoicePoll;
use agg_gui::widgets::on_screen_keyboard::{VoiceState, VoiceText};

use crate::dictation::{DictationError, DictationOptions, DictationPoll, FullPass};
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
    /// The audio would not start (iOS starts audio only inside a tap, and
    /// the next tap is one).
    Suspended,
    /// Anything else; the detail is for the console.
    Other(String),
}

impl MicError {
    /// What a start that failed this way tells its caller. A refusal is
    /// one a tap retries: the browser is asked again, and refuses at once
    /// while the site is blocked, or prompts or allows once it isn't.
    fn at_start(&self) -> DictationError {
        match self {
            MicError::Denied => DictationError::MicDenied,
            MicError::NoDevice => DictationError::NoMic,
            MicError::Unsupported => DictationError::MicUnsupported,
            MicError::Suspended => DictationError::MicSuspended,
            MicError::Other(_) => DictationError::MicFailed,
        }
    }
}

fn load_failure(reason: LoadFailure) -> DictationError {
    match reason {
        LoadFailure::Download => DictationError::ModelDownload,
        LoadFailure::Checksum => DictationError::ModelDamaged,
        LoadFailure::Model => DictationError::ModelLoad,
        LoadFailure::Startup => DictationError::EngineStartup,
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Model {
    /// Not asked for yet.
    Idle,
    Loading {
        progress: Option<f32>,
    },
    Loaded,
    Failed {
        error: DictationError,
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
    /// Stopped; `done` once the engine's final reply is in (and, for a
    /// full-pass session, the pass: [`Machine::full_pass`]).
    Finishing {
        session: u32,
        done: bool,
    },
}

/// A session's full pass.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pass {
    /// Not asked for.
    Unwanted,
    /// Asked for (at the start); not in yet.
    Waiting,
    /// In, to deliver with the poll that ends the dictation.
    Read(FullPass),
}

/// See the module docs.
pub(crate) struct Machine {
    urls: Vec<String>,
    /// The language when the keyboard names none ("" detects it).
    default_language: Option<String>,
    /// Set for good when the platform can't do dictation at all.
    unsupported: Option<String>,
    /// The user tapped the key on such a platform: from then on it says
    /// why.
    unsupported_tapped: bool,
    model: Model,
    dictation: Dictation,
    last_session: u32,
    mapper: TextMapper,
    level: Option<f32>,
    /// The session's audio has reached the pipeline (its first level): only
    /// then does the key say listening.
    heard: bool,
    /// The session's full pass.
    full_pass: Pass,
    /// The stream's final reply is in (the session finishes once its full
    /// pass is too).
    final_in: bool,
    /// The last dictation's error, shown until the next start (or an app's
    /// prepare).
    error: Option<DictationError>,
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
            unsupported_tapped: false,
            model: Model::Idle,
            dictation: Dictation::Idle,
            last_session: 0,
            mapper: TextMapper::default(),
            level: None,
            heard: false,
            full_pass: Pass::Unwanted,
            final_in: false,
            error: None,
            polled_since_start: true,
            commands: Vec::new(),
        }
    }

    /// A machine for a platform without dictation: it does nothing, and
    /// reports `message` as an error without retry from the first tap on.
    /// Until then it reports `Ready`, so that tap reaches it: the keyboard's
    /// `Unavailable` key ignores taps and shows no reason, and an error
    /// before any tap would put up a banner nobody asked for.
    pub fn unsupported(message: &str) -> Machine {
        let mut m = Machine::new(Vec::new(), None);
        m.unsupported = Some(message.to_string());
        m
    }

    /// Why the platform can't dictate, if it can't.
    pub fn unsupported_reason(&self) -> Option<&str> {
        self.unsupported.as_deref()
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

    /// The session in progress waits for the model: its audio queues in
    /// the engine behind the load.
    fn waiting_for_model(&self) -> bool {
        self.current_session().is_some() && self.model != Model::Loaded
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

    /// Start a dictation: the keyboard's (`&VoiceContext`) or another
    /// caller's (`&DictationOptions`).
    pub fn start<'a>(&mut self, options: impl Into<DictationOptions> + 'a) {
        let options: DictationOptions = options.into();
        if self.unsupported.is_some() {
            self.unsupported_tapped = true;
            return;
        }
        if self.dictation != Dictation::Idle {
            return;
        }
        self.error = None;
        self.polled_since_start = false;
        self.load();
        self.begin(&options);
    }

    /// Open a session: the engine's stream first (behind the load, if the
    /// model isn't in), so no audio arrives before it, then the microphone.
    fn begin(&mut self, options: &DictationOptions) {
        self.last_session = self.last_session.wrapping_add(1);
        let session = self.last_session;
        self.mapper = TextMapper::new(options.sentence_start);
        self.level = None;
        self.heard = false;
        self.final_in = false;
        self.full_pass = if options.full_pass {
            Pass::Waiting
        } else {
            Pass::Unwanted
        };
        self.dictation = Dictation::Opening { session };
        let language = options
            .language
            .clone()
            .or_else(|| self.default_language.clone())
            .unwrap_or_default();
        self.commands.push(Command::Worker(ToWorker::Start {
            session,
            language,
            full_pass: options.full_pass,
        }));
        self.commands.push(Command::OpenMic { session });
    }

    /// End the session at once, dropping what it has not delivered.
    fn abort(&mut self) {
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
        if self.waiting_for_model() {
            // Nothing recognised yet: the audio queued behind the load is
            // dropped, as for a cancel.
            self.abort();
            return;
        }
        match self.dictation {
            Dictation::Listening { session } => {
                self.commands.push(Command::CloseMic {
                    session,
                    flush: true,
                });
                self.commands
                    .push(Command::Worker(ToWorker::Finish { session }));
                if self.full_pass != Pass::Unwanted {
                    self.commands
                        .push(Command::Worker(ToWorker::FullPass { session }));
                }
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

    /// The microphone did not open: the dictation ends with the error. Or
    /// (unplugged, switched, its stream ended) it stopped while listening:
    /// like a stop, what it captured is recognised and committed, and
    /// [`DictationError::MicLost`] shows after the final text (and is in
    /// every poll from now on).
    pub fn on_mic_failed(&mut self, session: u32, err: MicError) {
        let listening = match self.dictation {
            Dictation::Opening { session: s } if s == session => false,
            Dictation::Listening { session: s } if s == session => true,
            _ => return,
        };
        if let MicError::Other(detail) = &err {
            log(&format!("microphone: {detail}"));
        }
        // Shown once the dictation is idle (`state` reports it no sooner).
        if listening {
            self.error = Some(DictationError::MicLost);
            self.stop();
            return;
        }
        self.error = Some(err.at_start());
        self.dictation = Dictation::Idle;
        self.commands
            .push(Command::Worker(ToWorker::Cancel { session }));
    }

    /// The input level, 0..=1: audio of `session` reached the pipeline.
    /// Returns whether it is the session's first, which turns the key to
    /// listening (the host repaints for it).
    pub fn on_level(&mut self, session: u32, level: f32) -> bool {
        if self.dictation != (Dictation::Listening { session }) {
            return false;
        }
        self.level = Some(level.clamp(0.0, 1.0));
        !std::mem::replace(&mut self.heard, true)
    }

    /// The engine (the worker, or the native thread) is gone.
    pub fn worker_lost(&mut self, detail: &str) {
        log(&format!("engine lost: {detail}"));
        let waiting = self.waiting_for_model();
        if self.current_session().is_some() {
            self.abort();
            // Its cancel has nowhere to go.
            self.commands
                .retain(|c| !matches!(c, Command::Worker(ToWorker::Cancel { .. })));
            self.error = Some(DictationError::Engine);
        }
        // A session waiting for the model hears why; with nobody waiting the
        // key stays ready and the next tap loads afresh. A load that already
        // failed keeps its error: losing the engine after it (natively, the
        // thread a failed load ended) says nothing new.
        self.model = match &self.model {
            Model::Failed { .. } => self.model.clone(),
            _ if waiting => Model::Failed {
                error: load_failure(LoadFailure::Startup),
            },
            _ => Model::Idle,
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
                // The session's stream and audio, queued behind the load,
                // go on in the engine now.
                self.model = Model::Loaded;
            }
            FromWorker::LoadFailed { reason, detail } => {
                log(&format!("model: {detail}"));
                // Shown only to a session waiting for it, which ends; a load
                // nobody waits for (an app's prepare, a cancelled tap's)
                // fails quietly, and the next tap loads afresh.
                self.model = if self.waiting_for_model() {
                    self.abort();
                    Model::Failed {
                        error: load_failure(reason),
                    }
                } else {
                    Model::Idle
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
                    self.final_in = true;
                    self.finish_if_done(session);
                }
            }
            FromWorker::FullPass {
                session,
                text,
                repeated,
            } => self.on_full_pass(session, FullPass::Read { text, repeated }),
            FromWorker::FullPassFailed { session, detail } => {
                log(&format!("full pass: {detail}"));
                self.on_full_pass(session, FullPass::Failed { detail });
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
                self.error = Some(DictationError::Engine);
            }
        }
    }

    /// The full pass of the session finishing (a stale one is dropped).
    fn on_full_pass(&mut self, session: u32, pass: FullPass) {
        if self.dictation
            == (Dictation::Finishing {
                session,
                done: false,
            })
            && self.full_pass == Pass::Waiting
        {
            self.full_pass = Pass::Read(pass);
            self.finish_if_done(session);
        }
    }

    /// The finishing session is done once its final reply and (if asked
    /// for) its full pass are in.
    fn finish_if_done(&mut self, session: u32) {
        if self.final_in && self.full_pass != Pass::Waiting {
            self.dictation = Dictation::Finishing {
                session,
                done: true,
            };
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
        if let Some(error) = self.unsupported_error() {
            return VoiceState::Error {
                message: error.message().to_string(),
                retry: error.retry(),
            };
        }
        if self.unsupported.is_some() {
            return VoiceState::Ready;
        }
        let progress = match &self.model {
            Model::Loading { progress } => *progress,
            _ => None,
        };
        match self.dictation {
            Dictation::Opening { .. } => return VoiceState::Preparing { progress },
            // Capturing, but not listening until the model is in and the
            // audio flows: the cue to speak comes when nothing said after it
            // can be lost.
            Dictation::Listening { .. } if self.model != Model::Loaded || !self.heard => {
                return VoiceState::Preparing { progress }
            }
            Dictation::Listening { .. } => return VoiceState::Listening { level: self.level },
            Dictation::Finishing { .. } => return VoiceState::Finishing,
            Dictation::Idle => {}
        }
        match self.idle_error() {
            Some(error) => VoiceState::Error {
                message: error.message().to_string(),
                retry: error.retry(),
            },
            // Ready for the tap that loads it; loading for nobody yet, a tap
            // may still ask to dictate.
            None => VoiceState::Ready,
        }
    }

    /// An unsupported platform's error, once a start was tried.
    fn unsupported_error(&self) -> Option<DictationError> {
        let reason = self.unsupported.as_ref()?;
        self.unsupported_tapped
            .then(|| DictationError::Unsupported(reason.clone()))
    }

    /// The error an idle machine reports: the last dictation's, else the
    /// model's failed load.
    fn idle_error(&self) -> Option<&DictationError> {
        self.error.as_ref().or(match &self.model {
            Model::Failed { error } => Some(error),
            _ => None,
        })
    }

    /// The keyboard's poll ([`Machine::poll_dictation`] without what only
    /// other callers use), as the keyboard's tests read it.
    #[cfg(test)]
    pub fn poll(&mut self) -> VoicePoll {
        self.poll_dictation().into()
    }

    /// The state and the text since the last poll. A finished dictation's
    /// final text goes out with a `Finishing` poll (marked `finished`, with
    /// its full pass); the next reports the state after it.
    pub fn poll_dictation(&mut self) -> DictationPoll {
        let mut state = self.state();
        let held_back = !self.polled_since_start && matches!(state, VoiceState::Error { .. });
        if held_back {
            state = VoiceState::Preparing { progress: None };
        }
        self.polled_since_start = true;
        let text = self.mapper.take().map(|d| VoiceText {
            committed: d.committed,
            pending: d.pending,
        });
        let finished = matches!(self.dictation, Dictation::Finishing { done: true, .. });
        let mut full_pass = None;
        if finished {
            self.dictation = Dictation::Idle;
            if let Pass::Read(pass) = std::mem::replace(&mut self.full_pass, Pass::Unwanted) {
                full_pass = Some(pass);
            }
        }
        // While a dictation runs, only a lost microphone is news (it is
        // finishing what it heard); otherwise what the state reports.
        let error = if held_back {
            None
        } else if let Some(error) = self.unsupported_error() {
            Some(error)
        } else if self.current_session().is_some() || finished {
            self.error.clone().filter(|e| *e == DictationError::MicLost)
        } else if matches!(state, VoiceState::Error { .. }) {
            self.idle_error().cloned()
        } else {
            None
        };
        DictationPoll {
            state,
            text,
            finished,
            full_pass,
            error,
        }
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
