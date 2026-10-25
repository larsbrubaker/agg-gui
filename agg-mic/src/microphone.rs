//! [`Microphone`]: one recording at a time over a platform's [`Backend`]. It owns the recording's
//! [`Delivery`] and its state (idle, starting, recording), so every platform starts, stops and
//! refuses a second start the same way. A stop that comes while a start is still waiting on a
//! permission prompt closes the recording's delivery at once (nothing of it reaches the handlers
//! after that stop returns) and is honoured once the prompt is answered: the start then stops
//! what it opened and returns [`MicError::Stopped`], whether it succeeded or gave up. The platform captures (`mac_capture`, `windows_capture`, `linux_capture`) are the backends
//! [`Microphone::default_input`] picks; a test hands [`Microphone::with_backend`] its own.
//!
//! The one-recording state and the stop-while-starting rule follow mattercad-ai's
//! `audio/recorder_base.rs` (its duration cap, clock and WAV stay in MatterCAD). Unlike that
//! recorder, a start here blocks until the device is open, so any thread, with or without an
//! executor, can drive it. A stop, on the other hand, must be quick (agg-gui-whistle stops on the
//! UI thread): each backend returns once no chunk can arrive, leaving slow teardown to a thread of
//! its own, and `AGG_MIC_TRACE=1` prints how long the backend's stop and the delivery's close
//! took.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use crate::delivery::Delivery;
use crate::{ChunkHandler, FailureHandler, MicError, ALREADY_RUNNING_MESSAGE, UNSUPPORTED_MESSAGE};

/// What a platform supplies: the capture itself. Called by [`Microphone`] only, never twice at
/// once: a `start` is always followed by a `stop` before the next `start`.
pub trait Backend: Send + Sync + 'static {
    /// Whether this platform can record at all; says nothing about permission.
    fn is_available(&self) -> bool;

    /// Asks for permission if the user has not been asked, opens the default input device and
    /// starts handing its audio to `delivery`; returns once audio is flowing, or with why it
    /// isn't. It may block for as long as a permission prompt stays up. A capture that ends on its
    /// own later reports through [`Delivery::fail`].
    fn start(&self, delivery: Delivery) -> Result<(), MicError>;

    /// Ends the capture: every chunk captured is delivered before this returns, and none after.
    /// Also called after a start that succeeded but was stopped meanwhile, and after a failure.
    fn stop(&self);
}

/// Where the microphone is in a recording's life.
enum State {
    Idle,
    /// `start` is waiting on the backend with `delivery`; `stop_requested` is a stop that came
    /// meanwhile (and closed the delivery).
    Starting {
        delivery: Delivery,
        stop_requested: bool,
    },
    Recording(Delivery),
}

/// The system microphone (see the crate docs). `Send + Sync`: one thread can wait in
/// [`Self::start`] while another calls [`Self::stop`].
pub struct Microphone {
    backend: Box<dyn Backend>,
    state: Mutex<State>,
}

impl Microphone {
    /// The system's default input device, listed by the sound server (Linux) under the name of
    /// the running executable. Where there is no microphone support, a microphone that is not
    /// [`Self::is_available`].
    pub fn default_input() -> Microphone {
        let program = std::env::current_exe().ok().and_then(|path| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        });
        Microphone::with_app_name(program.as_deref().unwrap_or("agg-mic"))
    }

    /// [`Self::default_input`], listed by the sound server as `app_name` (Linux; macOS and Windows
    /// name the process themselves).
    pub fn with_app_name(app_name: &str) -> Microphone {
        Microphone::with_backend(platform_backend(app_name))
    }

    /// A microphone over `backend`: a platform's capture, or a test's stand-in.
    pub fn with_backend(backend: impl Backend) -> Microphone {
        Microphone {
            backend: Box::new(backend),
            state: Mutex::new(State::Idle),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether this platform can record at all. Permission is only asked by [`Self::start`].
    pub fn is_available(&self) -> bool {
        self.backend.is_available()
    }

    /// True from a successful start until a stop, or until the recording fails on its own.
    pub fn is_recording(&self) -> bool {
        matches!(&*self.lock(), State::Recording(delivery) if delivery.is_open())
    }

    /// Starts recording: asks for permission first if the user has not been asked, then hands
    /// every chunk to `on_chunk` until [`Self::stop`], and tells `on_failure` if the recording
    /// ends on its own first. Blocks until audio is flowing, which a permission prompt can make
    /// as long as the user takes, so call it off the UI thread. A recording that already failed
    /// is cleaned up first; one still running is refused ([`ALREADY_RUNNING_MESSAGE`]). A
    /// [`Self::stop`] while this waits makes it return [`MicError::Stopped`].
    pub fn start(
        &self,
        on_chunk: ChunkHandler,
        on_failure: FailureHandler,
    ) -> Result<(), MicError> {
        if !self.backend.is_available() {
            return Err(MicError::Unsupported(UNSUPPORTED_MESSAGE.to_owned()));
        }
        let delivery = Delivery::new(on_chunk, on_failure);
        {
            let mut state = self.lock();
            match &*state {
                State::Idle => {}
                State::Recording(delivery) if !delivery.is_open() => self.backend.stop(),
                State::Starting { .. } | State::Recording(_) => {
                    return Err(MicError::Failed(ALREADY_RUNNING_MESSAGE.to_owned()));
                }
            }
            *state = State::Starting {
                delivery: delivery.clone(),
                stop_requested: false,
            };
        }
        let started = self.backend.start(delivery.clone());
        let mut state = self.lock();
        let stop_requested = matches!(
            *state,
            State::Starting {
                stop_requested: true,
                ..
            }
        );
        if stop_requested {
            // The stop came while the prompt was up (and already closed the delivery): honour it
            // now there is something to stop, and say it was a stop even if the start failed.
            if started.is_ok() {
                self.backend.stop();
            }
            *state = State::Idle;
            return Err(MicError::Stopped);
        }
        if let Err(error) = started {
            delivery.close();
            *state = State::Idle;
            return Err(error);
        }
        *state = State::Recording(delivery);
        Ok(())
    }

    /// Stops the recording: every chunk captured has reached the handler when this returns, and
    /// none follows; both handlers have been dropped. During a start that is still waiting, it
    /// closes that start's delivery and asks the start to stop: it returns at once, nothing of
    /// that start reaches the handlers, and the start ends with [`MicError::Stopped`]. Does
    /// nothing when idle.
    pub fn stop(&self) {
        let mut state = self.lock();
        match &mut *state {
            State::Idle => {}
            State::Starting {
                delivery,
                stop_requested,
            } => {
                *stop_requested = true;
                delivery.close();
            }
            State::Recording(delivery) => {
                // Held through the backend's stop, so a new start waits for it to finish.
                let began = crate::tracing().then(Instant::now);
                self.backend.stop();
                let stopped = began.map(|began| began.elapsed());
                delivery.close();
                *state = State::Idle;
                if let (Some(began), Some(stopped)) = (began, stopped) {
                    eprintln!(
                        "agg-mic trace: stop: backend stop {:.2} ms, delivery closed {:.2} ms, \
                         total {:.2} ms",
                        crate::ms(stopped),
                        crate::ms(began.elapsed() - stopped),
                        crate::ms(began.elapsed())
                    );
                }
            }
        }
    }
}

impl Drop for Microphone {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The backend for a platform with no capture: never available.
#[cfg(any(
    test,
    not(any(target_os = "macos", target_os = "windows", target_os = "linux"))
))]
struct Unsupported;

#[cfg(any(
    test,
    not(any(target_os = "macos", target_os = "windows", target_os = "linux"))
))]
impl Backend for Unsupported {
    fn is_available(&self) -> bool {
        false
    }
    fn start(&self, _delivery: Delivery) -> Result<(), MicError> {
        Err(MicError::Unsupported(UNSUPPORTED_MESSAGE.to_owned()))
    }
    fn stop(&self) {}
}

#[cfg(target_os = "macos")]
fn platform_backend(_app_name: &str) -> impl Backend {
    crate::mac_capture::MacBackend::default()
}

#[cfg(target_os = "windows")]
fn platform_backend(_app_name: &str) -> impl Backend {
    crate::windows_capture::WindowsBackend::default()
}

#[cfg(target_os = "linux")]
fn platform_backend(app_name: &str) -> impl Backend {
    crate::linux_capture::LinuxBackend::new(app_name)
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn platform_backend(_app_name: &str) -> impl Backend {
    Unsupported
}

#[cfg(test)]
#[path = "microphone_tests.rs"]
mod tests;
