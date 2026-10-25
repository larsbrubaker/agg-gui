//! How a capture hands its audio on while a recording runs: the [`Delivery`] a [`crate::Backend`]
//! is given at start holds the recording's [`ChunkHandler`] and [`FailureHandler`], and is cloned
//! into whatever thread captures. [`crate::Microphone`] closes it when the recording stops (or a
//! stop comes while it still starts), and a failure closes it too. Closing drops both handlers
//! there and then, so nothing reaches the consumer after [`crate::Microphone::stop`] returns, and
//! a consumer whose handler flushes when dropped has its last audio out by then (on a failure,
//! before it hears of the failure).
//!
//! Calls into the handlers are serialized by the slot's lock, which is why the chunk handler can
//! be `FnMut`. Each runs under `catch_unwind`: a panicking handler ends the recording with
//! [`HANDLER_PANICKED_MESSAGE`] instead of unwinding into the platform's audio thread (on macOS
//! an Objective-C block, where it would abort the process).
//!
//! From mattercad-ai's `audio/chunks.rs` (`ChunkSink`, its `deliver` and `deliver_pcm16`), with
//! the failure report, the close and the panic guard added.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::{ChunkHandler, FailureHandler, MicError, HANDLER_PANICKED_MESSAGE};

/// The open recording's handlers.
struct Handlers {
    chunks: ChunkHandler,
    failure: FailureHandler,
}

/// The handlers while open; `None` once closed or failed.
type Slot = Option<Handlers>;

/// One recording's handlers, as its capture delivers to them (see the module docs). Cloning
/// shares them.
#[derive(Clone)]
pub struct Delivery {
    slot: Arc<Mutex<Slot>>,
}

impl Delivery {
    pub(crate) fn new(chunks: ChunkHandler, failure: FailureHandler) -> Delivery {
        Delivery {
            slot: Arc::new(Mutex::new(Some(Handlers { chunks, failure }))),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Slot> {
        // A panic is caught inside the lock, so the slot is only poisoned by a bug here.
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether audio still goes through: false once the recording stopped or failed.
    pub fn is_open(&self) -> bool {
        self.lock().is_some()
    }

    /// Hands `samples` (mono, -1 to 1) at `rate` Hz to the chunk handler, while open. An empty
    /// chunk is not news. A handler that panics fails the recording.
    pub fn deliver(&self, samples: &[f32], rate: u32) {
        if samples.is_empty() {
            return;
        }
        let mut slot = self.lock();
        let Some(handlers) = slot.as_mut() else {
            return;
        };
        let handled = catch_unwind(AssertUnwindSafe(|| (handlers.chunks)(samples, rate)));
        if handled.is_err() {
            fail_locked(
                &mut slot,
                MicError::Failed(HANDLER_PANICKED_MESSAGE.to_owned()),
            );
        }
    }

    /// Hands 16-bit `samples` at `rate` Hz to the chunk handler as `f32`, while open (full scale is
    /// 32768, so -32768 maps to exactly -1).
    pub fn deliver_pcm16(&self, samples: &[i16], rate: u32) {
        if samples.is_empty() || !self.is_open() {
            return;
        }
        let converted: Vec<f32> = samples
            .iter()
            .map(|&sample| f32::from(sample) / 32768.0)
            .collect();
        self.deliver(&converted, rate);
    }

    /// The recording ended on its own: drops the chunk handler (its last flush goes out first),
    /// then tells the failure handler, the first time only and only while open. No chunk follows.
    pub fn fail(&self, error: MicError) {
        fail_locked(&mut self.lock(), error);
    }

    /// Closes for good: waits for a handler call in flight, then drops both handlers, so nothing
    /// is delivered from now on.
    pub(crate) fn close(&self) {
        let closed = self.lock().take();
        if let Some(handlers) = closed {
            contain(move || drop(handlers));
        }
    }
}

/// [`Delivery::fail`] with the slot already locked.
fn fail_locked(slot: &mut Slot, error: MicError) {
    let Some(Handlers { chunks, failure }) = slot.take() else {
        return;
    };
    contain(move || drop(chunks));
    contain(move || failure(error));
}

/// Runs `f`, swallowing a panic (already reported by the panic hook): the audio thread goes on.
fn contain(f: impl FnOnce()) {
    let _ = catch_unwind(AssertUnwindSafe(f));
}
