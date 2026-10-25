//! The transcription engine: one whistle model and at most one live stream,
//! answering the provider's session messages ([`ToWorker`]) with
//! [`FromWorker`] replies.
//!
//! The same engine runs in the Web Worker (`worker/`) and on the native
//! engine thread (`native/`); each host loads the model its own way and then
//! hands every message but `Load` to [`Engine::handle`]. Streams use
//! whistle's live preset, `Options::streaming()` (one beam, so a one-thread
//! wasm pass stays under the 1 s step).

use whistle::{Options, Stream, StreamUpdate, Whistle};

use crate::protocol::{FromWorker, Recognised, ToWorker};

/// The model's sample rate; other rates are resampled.
const RATE: u32 = 16_000;

/// The model and the live stream.
pub struct Engine {
    /// Leaked: a `Stream` borrows its `Whistle`, and a host keeps the model
    /// for the rest of its life (the worker's, or the process's natively,
    /// where `native/` builds it once), so `'static` is the honest lifetime.
    whistle: &'static Whistle,
    /// The open stream and its session.
    stream: Option<(u32, Stream<'static>)>,
}

impl From<&StreamUpdate> for Recognised {
    fn from(update: &StreamUpdate) -> Recognised {
        Recognised {
            text: update.text.clone(),
            continues: update.continues,
            pending: update.pending.clone(),
        }
    }
}

impl Engine {
    /// An engine for a model already built (and leaked: see the field).
    pub fn new(whistle: &'static Whistle) -> Engine {
        Engine {
            whistle,
            stream: None,
        }
    }

    /// Build the model from `whistle.cact`'s bytes (already checked). The
    /// worker's way; natively the model is built once per process instead.
    #[cfg(target_arch = "wasm32")]
    pub fn from_cact(model: &[u8]) -> Result<Engine, whistle::Error> {
        let whistle = Whistle::from_cact(model)?;
        Ok(Engine::new(Box::leak(Box::new(whistle))))
    }

    /// The session of the open stream.
    pub fn session(&self) -> Option<u32> {
        self.stream.as_ref().map(|(s, _)| *s)
    }

    /// Answer one message. `Load` is the host's business and is ignored.
    pub fn handle(&mut self, msg: ToWorker) -> Option<FromWorker> {
        match msg {
            ToWorker::Load { .. } => None,
            ToWorker::Start { session, language } => {
                let options = Options {
                    language: (!language.is_empty()).then_some(language),
                    ..Options::streaming()
                };
                match self.whistle.stream(&options) {
                    Ok(stream) => {
                        self.stream = Some((session, stream));
                        None
                    }
                    Err(err) => {
                        self.stream = None;
                        Some(FromWorker::Failed {
                            session,
                            detail: err.to_string(),
                        })
                    }
                }
            }
            ToWorker::Push {
                session,
                rate,
                samples,
            } => {
                let (open, stream) = self.stream.as_mut()?;
                if *open != session {
                    return None;
                }
                let pushed = if rate == RATE {
                    stream.push(&samples)
                } else {
                    stream.push(&whistle::audio::resample(&samples, rate, RATE))
                };
                Some(match pushed {
                    Ok(update) => FromWorker::Update {
                        session,
                        update: (&update).into(),
                    },
                    Err(err) => {
                        self.stream = None;
                        FromWorker::Failed {
                            session,
                            detail: err.to_string(),
                        }
                    }
                })
            }
            ToWorker::Finish { session } => {
                // A finish for a stream that is not open (its start failed)
                // still gets its final reply, so the provider never waits.
                let stream = match self.stream.take() {
                    Some((open, stream)) if open == session => stream,
                    other => {
                        self.stream = other;
                        return Some(FromWorker::Final {
                            session,
                            update: Recognised::default(),
                        });
                    }
                };
                Some(match stream.finish() {
                    Ok(update) => FromWorker::Final {
                        session,
                        update: (&update).into(),
                    },
                    Err(err) => FromWorker::Failed {
                        session,
                        detail: err.to_string(),
                    },
                })
            }
            ToWorker::Cancel { session } => {
                if self.session() == Some(session) {
                    self.stream = None;
                }
                None
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32"), feature = "provider"))]
#[path = "engine_tests.rs"]
pub(crate) mod tests;
