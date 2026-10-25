//! The transcription engine: one whistle model and at most one live stream,
//! answering the provider's session messages ([`ToWorker`]) with
//! [`FromWorker`] replies.
//!
//! The same engine runs in the Web Worker (`worker/`) and on the native
//! engine thread (`native/`); each host loads the model its own way and then
//! hands every message but `Load` to [`Engine::handle`]. Streams use
//! whistle's live preset, `Options::streaming()` (one beam, so a one-thread
//! wasm pass stays under the 1 s step). Audio at another rate than the
//! model's is resampled as one stream per session (`resample.rs`), so the
//! pushes it came in leave no trace; its last few milliseconds go in at
//! `Finish`.
//!
//! A session started with `full_pass` also keeps its audio, at the model's
//! rate, through its `Finish`; the `FullPass` that follows reads the whole
//! recording once more with whistle's offline options (the whole recording
//! around every word, a wider beam search: [`full_pass_options`]), for a
//! caller that weighs that reading against the streamed one. The kept
//! recording goes with its pass, a cancel, or the next session's start.

use whistle::{Options, Stream, StreamUpdate, Whistle};

use crate::protocol::{FromWorker, Recognised, ToWorker};
use crate::resample::ModelRate;

/// The model and the live stream.
pub struct Engine {
    /// Leaked: a `Stream` borrows its `Whistle`, and a host keeps the model
    /// for the rest of its life (the worker's, or the process's natively,
    /// where `native/` builds it once), so `'static` is the honest lifetime.
    whistle: &'static Whistle,
    /// The open stream.
    stream: Option<Open>,
    /// A finished full-pass session's recording, waiting for its pass.
    finished: Option<Kept>,
}

/// A finished session's recording, at the model's rate, for its full pass.
struct Kept {
    session: u32,
    language: String,
    samples: Vec<f32>,
}

/// The options of a full pass in `language` ("" detects it): whistle's
/// defaults, its offline transcription.
pub(crate) fn full_pass_options(language: &str) -> Options {
    Options {
        language: (!language.is_empty()).then(|| language.to_string()),
        ..Options::default()
    }
}

/// A session's stream and its audio's conversion to the model's rate.
struct Open {
    session: u32,
    stream: Stream<'static>,
    input: ModelRate,
    language: String,
    /// The audio pushed so far, when the session wants a full pass.
    kept: Option<Vec<f32>>,
}

/// Two updates in a row as one: `first`'s committed words, then `second`'s
/// (spaced as `text_map.rs` spaces updates), and `second`'s tail.
pub(crate) fn merge(first: Recognised, second: Recognised) -> Recognised {
    let trimmed = |r: &Recognised| {
        if r.continues {
            r.text.trim_end().to_string()
        } else {
            r.text.trim().to_string()
        }
    };
    let (a, b) = (trimmed(&first), trimmed(&second));
    if a.is_empty() {
        return second;
    }
    let text = if b.is_empty() {
        a
    } else if second.continues {
        format!("{a}{b}")
    } else {
        format!("{a} {b}")
    };
    Recognised {
        text,
        continues: first.continues,
        pending: second.pending,
        pending_continues: second.pending_continues,
    }
}

impl From<&StreamUpdate> for Recognised {
    fn from(update: &StreamUpdate) -> Recognised {
        Recognised {
            text: update.text.clone(),
            continues: update.continues,
            pending: update.pending.clone(),
            pending_continues: update.pending_continues,
        }
    }
}

impl Engine {
    /// An engine for a model already built (and leaked: see the field).
    pub fn new(whistle: &'static Whistle) -> Engine {
        Engine {
            whistle,
            stream: None,
            finished: None,
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
        self.stream.as_ref().map(|open| open.session)
    }

    /// Answer one message. `Load` is the host's business and is ignored.
    pub fn handle(&mut self, msg: ToWorker) -> Option<FromWorker> {
        match msg {
            ToWorker::Load { .. } => None,
            ToWorker::Start {
                session,
                language,
                full_pass,
            } => {
                // The last session's recording is no longer wanted.
                self.finished = None;
                let options = Options {
                    language: (!language.is_empty()).then(|| language.clone()),
                    ..Options::streaming()
                };
                match self.whistle.stream(&options) {
                    Ok(stream) => {
                        self.stream = Some(Open {
                            session,
                            stream,
                            input: ModelRate::default(),
                            language,
                            kept: full_pass.then(Vec::new),
                        });
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
                let open = self.stream.as_mut()?;
                if open.session != session {
                    return None;
                }
                let Some(samples) = open.input.push(rate, samples) else {
                    self.stream = None;
                    return Some(FromWorker::Failed {
                        session,
                        detail: format!("unsupported sample rate {rate} Hz"),
                    });
                };
                if let Some(kept) = open.kept.as_mut() {
                    kept.extend_from_slice(&samples);
                }
                Some(match open.stream.push(&samples) {
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
                let open = match self.stream.take() {
                    Some(open) if open.session == session => open,
                    other => {
                        self.stream = other;
                        return Some(FromWorker::Final {
                            session,
                            update: Recognised::default(),
                        });
                    }
                };
                Some(match finish(open, &mut self.finished) {
                    Ok(update) => FromWorker::Final { session, update },
                    Err(err) => FromWorker::Failed {
                        session,
                        detail: err.to_string(),
                    },
                })
            }
            ToWorker::FullPass { session } => Some(self.full_pass(session)),
            ToWorker::Cancel { session } => {
                if self.session() == Some(session) {
                    self.stream = None;
                }
                if self.finished.as_ref().map(|k| k.session) == Some(session) {
                    self.finished = None;
                }
                None
            }
        }
    }
}

impl Engine {
    /// The full pass over `session`'s kept recording (taken: it goes with
    /// its pass). A session that kept none (it didn't ask, was cancelled,
    /// or another started since) fails, so the provider never waits.
    fn full_pass(&mut self, session: u32) -> FromWorker {
        let kept = match self.finished.take() {
            Some(kept) if kept.session == session => kept,
            other => {
                self.finished = other;
                return FromWorker::FullPassFailed {
                    session,
                    detail: "no recording was kept for the full pass".into(),
                };
            }
        };
        // Stopped before any audio arrived: nothing to read.
        if kept.samples.is_empty() {
            return FromWorker::FullPass {
                session,
                text: String::new(),
                repeated: false,
            };
        }
        match self
            .whistle
            .transcribe(&kept.samples, &full_pass_options(&kept.language))
        {
            Ok(transcript) => FromWorker::FullPass {
                session,
                text: transcript.text.trim().to_string(),
                repeated: transcript.repeated,
            },
            Err(err) => FromWorker::FullPassFailed {
                session,
                detail: err.to_string(),
            },
        }
    }
}

/// End `open`'s stream: the resampler's last audio, then whistle's finish,
/// as one update. A full-pass session's recording, with that last audio,
/// goes to `finished` for its pass.
fn finish(open: Open, finished: &mut Option<Kept>) -> Result<Recognised, whistle::Error> {
    let Open {
        session,
        mut stream,
        mut input,
        language,
        kept,
    } = open;
    let tail = input.finish();
    if let Some(mut samples) = kept {
        samples.extend_from_slice(&tail);
        *finished = Some(Kept {
            session,
            language,
            samples,
        });
    }
    let before = if tail.is_empty() {
        Recognised::default()
    } else {
        (&stream.push(&tail)?).into()
    };
    let last: Recognised = (&stream.finish()?).into();
    Ok(merge(before, last))
}

#[cfg(all(test, not(target_arch = "wasm32"), feature = "provider"))]
#[path = "engine_tests.rs"]
pub(crate) mod tests;
