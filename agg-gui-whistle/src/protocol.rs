//! The messages between the voice provider and the transcription engine.
//!
//! On the web the engine runs in a dedicated Web Worker (`worker/`): the
//! provider (`web/`) posts [`ToWorker`] and the worker posts [`FromWorker`]
//! back, each as a flat JavaScript object whose fields are [`Field`]s
//! (`web/js_fields.rs` converts). Natively (`native/`) the same messages
//! travel over channels between the UI thread and the engine thread, so both
//! hosts share one state machine (`machine.rs`) and one engine (`engine.rs`).
//!
//! Every dictation is a *session*, numbered by the provider. Messages about a
//! session carry its number, so a reply for a session the provider has since
//! cancelled is recognised and dropped (`machine.rs`), and the engine skips
//! work queued for a cancelled session ([`next_job`]).

use std::collections::VecDeque;

/// One field of a message: what a JavaScript object property holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Str(String),
    Num(f64),
    Bool(bool),
    /// Audio samples (a `Float32Array` on the web).
    Samples(Vec<f32>),
}

/// A message the engine can't read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError(pub String);

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "voice protocol: {}", self.0)
    }
}

impl std::error::Error for ProtocolError {}

/// Provider to engine.
#[derive(Debug, Clone, PartialEq)]
pub enum ToWorker {
    /// Load the model: try the cache, then each URL in turn.
    Load { urls: Vec<String> },
    /// Open a stream for a dictation. `language` is a code ("en"), or empty
    /// to detect it.
    Start { session: u32, language: String },
    /// Mono audio at `rate` Hz for the session's stream.
    Push {
        session: u32,
        rate: u32,
        samples: Vec<f32>,
    },
    /// The microphone is closed: recognise what is left and reply
    /// [`FromWorker::Final`].
    Finish { session: u32 },
    /// Drop the session's stream; no reply.
    Cancel { session: u32 },
}

/// Why the model could not be loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadFailure {
    /// Every URL failed (network, HTTP status, aborted).
    Download,
    /// A download arrived but is not the expected file (size or SHA-256).
    Checksum,
    /// The file checked out but the engine could not build the model.
    Model,
    /// The worker itself could not start (its script or wasm failed to load).
    Startup,
}

impl LoadFailure {
    fn name(self) -> &'static str {
        match self {
            LoadFailure::Download => "download",
            LoadFailure::Checksum => "checksum",
            LoadFailure::Model => "model",
            LoadFailure::Startup => "startup",
        }
    }

    fn from_name(name: &str) -> Option<LoadFailure> {
        Some(match name {
            "download" => LoadFailure::Download,
            "checksum" => LoadFailure::Checksum,
            "model" => LoadFailure::Model,
            "startup" => LoadFailure::Startup,
            _ => return None,
        })
    }
}

/// What one engine pass reports: the fields of `whistle::StreamUpdate` the
/// provider uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recognised {
    /// Words newly committed, joined with spaces.
    pub text: String,
    /// `text` begins with the rest of the last word committed before it.
    pub continues: bool,
    /// The unconfirmed tail.
    pub pending: String,
    /// `pending` begins with the rest of the last committed word ("0 mm"
    /// after a committed "2"): show it with no space before it.
    pub pending_continues: bool,
}

/// Engine to provider.
#[derive(Debug, Clone, PartialEq)]
pub enum FromWorker {
    /// Model download progress, in bytes.
    Progress { loaded: u64, total: u64 },
    /// The model is ready; `source` is "cache", a URL or a path.
    Loaded { source: String },
    /// The model could not be loaded; `detail` is for the console.
    LoadFailed { reason: LoadFailure, detail: String },
    /// A pass of the session's stream.
    Update { session: u32, update: Recognised },
    /// The session's last update after [`ToWorker::Finish`]; the stream is
    /// closed.
    Final { session: u32, update: Recognised },
    /// The session's stream failed; it is closed.
    Failed { session: u32, detail: String },
}

/// Read fields by name, failing with the field's name.
struct Reader<'a> {
    get: &'a dyn Fn(&str) -> Option<Field>,
}

impl Reader<'_> {
    fn str(&self, key: &str) -> Result<String, ProtocolError> {
        match (self.get)(key) {
            Some(Field::Str(s)) => Ok(s),
            _ => Err(ProtocolError(format!("`{key}` is not a string"))),
        }
    }

    fn num(&self, key: &str) -> Result<f64, ProtocolError> {
        match (self.get)(key) {
            Some(Field::Num(n)) if n.is_finite() && n >= 0.0 => Ok(n),
            _ => Err(ProtocolError(format!("`{key}` is not a count"))),
        }
    }

    fn u32(&self, key: &str) -> Result<u32, ProtocolError> {
        let n = self.num(key)?;
        if n <= u32::MAX as f64 && n.fract() == 0.0 {
            Ok(n as u32)
        } else {
            Err(ProtocolError(format!("`{key}` is out of range")))
        }
    }

    fn u64(&self, key: &str) -> Result<u64, ProtocolError> {
        // Byte counts stay far below 2^53, where f64 is exact.
        Ok(self.num(key)? as u64)
    }

    fn bool(&self, key: &str) -> Result<bool, ProtocolError> {
        match (self.get)(key) {
            Some(Field::Bool(b)) => Ok(b),
            _ => Err(ProtocolError(format!("`{key}` is not a boolean"))),
        }
    }

    fn samples(&self, key: &str) -> Result<Vec<f32>, ProtocolError> {
        match (self.get)(key) {
            Some(Field::Samples(s)) => Ok(s),
            _ => Err(ProtocolError(format!("`{key}` is not audio"))),
        }
    }

    fn recognised(&self) -> Result<Recognised, ProtocolError> {
        Ok(Recognised {
            text: self.str("text")?,
            continues: self.bool("continues")?,
            pending: self.str("pending")?,
            pending_continues: self.bool("pending_continues")?,
        })
    }
}

type Fields = Vec<(&'static str, Field)>;

fn ty(name: &str) -> (&'static str, Field) {
    ("type", Field::Str(name.to_string()))
}

fn session(n: u32) -> (&'static str, Field) {
    ("session", Field::Num(n as f64))
}

fn recognised_fields(fields: &mut Fields, r: &Recognised) {
    fields.push(("text", Field::Str(r.text.clone())));
    fields.push(("continues", Field::Bool(r.continues)));
    fields.push(("pending", Field::Str(r.pending.clone())));
    fields.push(("pending_continues", Field::Bool(r.pending_continues)));
}

impl ToWorker {
    /// The message as named fields (`type` names the variant).
    pub fn to_fields(&self) -> Fields {
        match self {
            ToWorker::Load { urls } => vec![ty("load"), ("urls", Field::Str(urls.join("\n")))],
            ToWorker::Start {
                session: s,
                language,
            } => vec![
                ty("start"),
                session(*s),
                ("language", Field::Str(language.clone())),
            ],
            ToWorker::Push {
                session: s,
                rate,
                samples,
            } => vec![
                ty("push"),
                session(*s),
                ("rate", Field::Num(*rate as f64)),
                ("samples", Field::Samples(samples.clone())),
            ],
            ToWorker::Finish { session: s } => vec![ty("finish"), session(*s)],
            ToWorker::Cancel { session: s } => vec![ty("cancel"), session(*s)],
        }
    }

    /// Read a message from its fields (`get` looks one up by name).
    pub fn from_fields(get: &dyn Fn(&str) -> Option<Field>) -> Result<ToWorker, ProtocolError> {
        let r = Reader { get };
        Ok(match r.str("type")?.as_str() {
            "load" => ToWorker::Load {
                urls: r
                    .str("urls")?
                    .split('\n')
                    .filter(|u| !u.is_empty())
                    .map(str::to_string)
                    .collect(),
            },
            "start" => ToWorker::Start {
                session: r.u32("session")?,
                language: r.str("language")?,
            },
            "push" => ToWorker::Push {
                session: r.u32("session")?,
                rate: r.u32("rate")?,
                samples: r.samples("samples")?,
            },
            "finish" => ToWorker::Finish {
                session: r.u32("session")?,
            },
            "cancel" => ToWorker::Cancel {
                session: r.u32("session")?,
            },
            other => return Err(ProtocolError(format!("unknown message `{other}`"))),
        })
    }
}

impl FromWorker {
    /// The message as named fields (`type` names the variant).
    pub fn to_fields(&self) -> Fields {
        match self {
            FromWorker::Progress { loaded, total } => vec![
                ty("progress"),
                ("loaded", Field::Num(*loaded as f64)),
                ("total", Field::Num(*total as f64)),
            ],
            FromWorker::Loaded { source } => {
                vec![ty("loaded"), ("source", Field::Str(source.clone()))]
            }
            FromWorker::LoadFailed { reason, detail } => vec![
                ty("loadFailed"),
                ("reason", Field::Str(reason.name().to_string())),
                ("detail", Field::Str(detail.clone())),
            ],
            FromWorker::Update { session: s, update } => {
                let mut f = vec![ty("update"), session(*s)];
                recognised_fields(&mut f, update);
                f
            }
            FromWorker::Final { session: s, update } => {
                let mut f = vec![ty("final"), session(*s)];
                recognised_fields(&mut f, update);
                f
            }
            FromWorker::Failed { session: s, detail } => vec![
                ty("failed"),
                session(*s),
                ("detail", Field::Str(detail.clone())),
            ],
        }
    }

    /// Read a message from its fields (`get` looks one up by name).
    pub fn from_fields(get: &dyn Fn(&str) -> Option<Field>) -> Result<FromWorker, ProtocolError> {
        let r = Reader { get };
        Ok(match r.str("type")?.as_str() {
            "progress" => FromWorker::Progress {
                loaded: r.u64("loaded")?,
                total: r.u64("total")?,
            },
            "loaded" => FromWorker::Loaded {
                source: r.str("source")?,
            },
            "loadFailed" => {
                let reason = r.str("reason")?;
                FromWorker::LoadFailed {
                    reason: LoadFailure::from_name(&reason)
                        .ok_or_else(|| ProtocolError(format!("unknown reason `{reason}`")))?,
                    detail: r.str("detail")?,
                }
            }
            "update" => FromWorker::Update {
                session: r.u32("session")?,
                update: r.recognised()?,
            },
            "final" => FromWorker::Final {
                session: r.u32("session")?,
                update: r.recognised()?,
            },
            "failed" => FromWorker::Failed {
                session: r.u32("session")?,
                detail: r.str("detail")?,
            },
            other => return Err(ProtocolError(format!("unknown message `{other}`"))),
        })
    }
}

/// The next job for the engine from its queue, in order, except that work
/// for a session cancelled later in the queue (its start, pushes and finish)
/// is skipped, so a cancel never waits behind audio nobody wants.
pub fn next_job(queue: &mut VecDeque<ToWorker>) -> Option<ToWorker> {
    loop {
        let job = queue.pop_front()?;
        let session = match &job {
            ToWorker::Start { session, .. }
            | ToWorker::Push { session, .. }
            | ToWorker::Finish { session } => *session,
            ToWorker::Load { .. } | ToWorker::Cancel { .. } => return Some(job),
        };
        let cancelled_later = queue
            .iter()
            .any(|m| matches!(m, ToWorker::Cancel { session: s } if *s == session));
        if !cancelled_later {
            return Some(job);
        }
    }
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
