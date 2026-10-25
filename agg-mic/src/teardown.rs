//! Running a capture's slow teardown off the caller's thread. `mac_capture` stops its
//! AVAudioEngine here: `AVAudioEngine.stop()` and detaching its node can take tens of
//! milliseconds (46 ms measured on a Mac), and `Microphone::stop` runs on a UI thread in
//! agg-gui-whistle. [`Teardown::hand_off`] starts the job on a thread of its own and returns at
//! once; [`Teardown::wait`], called by the next start (which runs off the UI thread anyway), waits
//! for it, so a new engine never starts while the old one is still stopping. No device here, so it
//! is tested (`teardown_tests.rs`) on every platform.

use std::sync::{Mutex, PoisonError};
use std::thread::JoinHandle;

/// The teardown in flight, if any (see the module docs).
#[derive(Default)]
pub(crate) struct Teardown {
    pending: Mutex<Option<JoinHandle<()>>>,
}

impl Teardown {
    /// Runs `job` on a thread of its own and returns at once. If no thread can be started, the
    /// job is dropped unrun, so a job whose drop tears down (as `mac_capture`'s engine does) is
    /// torn down here instead: slower, but nothing is left running. A start always waits before
    /// the next stop hands off again, so at most one is in flight.
    pub(crate) fn hand_off(&self, job: impl FnOnce() + Send + 'static) {
        let spawned = std::thread::Builder::new()
            .name("agg-mic-teardown".to_owned())
            .spawn(job);
        match spawned {
            Ok(thread) => {
                *self.pending.lock().unwrap_or_else(PoisonError::into_inner) = Some(thread);
            }
            Err(error) => eprintln!("agg-mic: no teardown thread ({error}); tore down in place"),
        }
    }

    /// Waits for the teardown in flight, if any. A teardown that panicked is over all the same.
    pub(crate) fn wait(&self) {
        let pending = self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(thread) = pending {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
#[path = "teardown_tests.rs"]
mod tests;
