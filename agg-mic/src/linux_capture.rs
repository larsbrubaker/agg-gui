//! The Linux capture, over the PulseAudio native protocol (spoken by PulseAudio and by PipeWire's
//! pulse server) through the pure-Rust `pulseaudio` crate: no libpulse and no ALSA.
//! [`LinuxBackend`] is the [`Backend`] `Microphone::default_input` picks on Linux.
//!
//! A capture thread of its own connects to the server's socket (found from the environment as
//! libpulse does), authenticates with the user's cookie, names the client (the app name given to
//! `Microphone::with_app_name`, or the executable's), and opens a record stream on the default source at 16 kHz mono
//! s16 (the server resamples) with 20 ms fragments, waiting at most 5 s for each of the server's
//! answers (a wedged server fails the start with [`LINUX_SERVER_TIMEOUT_MESSAGE`] instead of
//! hanging it; a `Microphone::stop` meanwhile has already closed the delivery, and the start then
//! ends as `MicError::Stopped`). It then reads frames (`pulse_frames`),
//! delivering each as it arrives, until stopped; a stop shuts the socket down, which ends the
//! blocking read, and joins the thread, so nothing is delivered after it. That join is short and
//! bounded: the shut-down socket ends the read at once, and the thread then has nothing left to
//! tear down but the socket itself (the server cleans up its side on its own time). A stream the server
//! kills (the microphone unplugged) or a connection that breaks ends the recording through
//! [`Delivery::fail`].
//!
//! **Permission.** A desktop session has no microphone prompt; a sandbox (Flatpak) without
//! microphone access gets "access denied" from the server, which becomes
//! [`MicError::PermissionDenied`] with [`LINUX_PERMISSION_DENIED_MESSAGE`].
//!
//! From mattercad-ai's `audio/linux_recorder.rs`; its WAV and its level meter stay in MatterCAD.

use std::ffi::{CStr, CString};
use std::io::{BufReader, Cursor};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use pulseaudio::protocol::{
    self, stream::BufferAttr, AuthParams, AuthReply, ChannelMap, ChannelPosition, Command,
    CreateRecordStreamReply, Prop, Props, ProtocolError, PulseError, RecordStreamParams,
    SampleFormat, SampleSpec, SetClientNameReply,
};

use crate::delivery::Delivery;
use crate::microphone::Backend;
use crate::pulse_frames::{FrameEvent, RecordFrameReader};
use crate::{
    MicError, DISCONNECTED_MESSAGE, LINUX_NO_SOUND_SERVER_MESSAGE, LINUX_PERMISSION_DENIED_MESSAGE,
    LINUX_SERVER_TIMEOUT_MESSAGE, NO_INPUT_DEVICE_MESSAGE,
};

/// The rate the server converts to.
const SAMPLE_RATE: u32 = 16_000;

// The longest the start waits for each of the server's answers.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

// 20 ms of 16 kHz s16 mono: how much the server collects before sending, so a stop loses at most
// this much.
const FRAGMENT_BYTES: u32 = 640;

/// What the backend and its capture thread share.
struct Shared {
    stop: AtomicBool,
    // The socket, kept to shut it down from a stop.
    socket: Mutex<Option<UnixStream>>,
    delivery: Delivery,
}

impl Shared {
    fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(socket) = self
            .socket
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }
}

struct Running {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Running {
    // Stops the capture thread and waits for it: the shut-down socket ends its read at once.
    fn finish(mut self) {
        self.shared.request_stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

// A capture dropped unfinished still stops its thread, without waiting for it.
impl Drop for Running {
    fn drop(&mut self) {
        self.shared.request_stop();
    }
}

/// The Linux capture (see the module docs).
pub(crate) struct LinuxBackend {
    /// The client name the server lists.
    app_name: CString,
    running: Mutex<Option<Running>>,
}

impl LinuxBackend {
    /// A capture the server lists as `app_name` (anything up to a NUL, which it cannot carry).
    pub(crate) fn new(app_name: &str) -> LinuxBackend {
        let name = app_name.split('\0').next().unwrap_or_default();
        let name = if name.is_empty() { "agg-mic" } else { name };
        LinuxBackend {
            app_name: CString::new(name).unwrap_or_else(|_| c"agg-mic".to_owned()),
            running: Mutex::new(None),
        }
    }

    fn take_running(&self) -> Option<Running> {
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

impl Backend for LinuxBackend {
    fn is_available(&self) -> bool {
        true
    }

    fn start(&self, delivery: Delivery) -> Result<(), MicError> {
        // A capture left by an earlier start that never stopped.
        if let Some(stale) = self.take_running() {
            stale.finish();
        }
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            socket: Mutex::new(None),
            delivery,
        });
        let (started, answer) = mpsc::channel();
        let thread_shared = Arc::clone(&shared);
        let app_name = self.app_name.clone();
        let thread = std::thread::Builder::new()
            .name("agg-mic-pulse".to_owned())
            .spawn(move || capture_thread(&thread_shared, &app_name, started))
            .map_err(|error| MicError::Failed(error.to_string()))?;
        let running = Running {
            shared,
            thread: Some(thread),
        };
        let result = answer
            .recv()
            .unwrap_or_else(|_| Err(MicError::Failed(NO_INPUT_DEVICE_MESSAGE.to_owned())));
        match result {
            Ok(()) => {
                *self.running.lock().unwrap_or_else(PoisonError::into_inner) = Some(running);
                Ok(())
            }
            Err(error) => {
                running.finish();
                Err(error)
            }
        }
    }

    fn stop(&self) {
        if let Some(running) = self.take_running() {
            running.finish();
        }
    }
}

fn capture_thread(shared: &Shared, app_name: &CString, started: Sender<Result<(), MicError>>) {
    let (mut reader, channel, version) = match open_stream(shared, app_name) {
        Ok(opened) => opened,
        Err(error) => {
            let _ = started.send(Err(error));
            return;
        }
    };
    let _ = started.send(Ok(()));
    let mut frames = RecordFrameReader::new(&mut reader, channel);
    loop {
        match frames.next_event() {
            Ok(FrameEvent::Samples(samples)) => {
                // The server already converted to 16 kHz mono, so the frame goes on as it is.
                shared.delivery.deliver_pcm16(&samples, SAMPLE_RATE);
            }
            Ok(FrameEvent::Command(payload)) => {
                let command = Command::read_tag_prefixed(&mut Cursor::new(payload), version);
                if let Ok((_, Command::RecordStreamKilled(_))) = command {
                    shared
                        .delivery
                        .fail(MicError::NoDevice(DISCONNECTED_MESSAGE.to_owned()));
                    return;
                }
            }
            Err(error) => {
                // A stop shuts the socket down; anything else ended the stream under us.
                if !shared.stop.load(Ordering::SeqCst) {
                    shared.delivery.fail(MicError::Failed(error.to_string()));
                }
                return;
            }
        }
    }
}

type Opened = (BufReader<UnixStream>, u32, u16);

// Connects to the server the environment names and opens the record stream.
fn open_stream(shared: &Shared, app_name: &CStr) -> Result<Opened, MicError> {
    let no_server = || MicError::Failed(LINUX_NO_SOUND_SERVER_MESSAGE.to_owned());
    let socket_path = pulseaudio::socket_path_from_env().ok_or_else(no_server)?;
    let socket = UnixStream::connect(socket_path).map_err(|_| no_server())?;
    handshake(shared, app_name, socket, HANDSHAKE_TIMEOUT)
}

// Authenticates, names the client and opens the record stream on `socket`, each answer awaited
// for at most `timeout` (a wedged server fails the start rather than hanging it).
fn handshake(
    shared: &Shared,
    app_name: &CStr,
    socket: UnixStream,
    timeout: Duration,
) -> Result<Opened, MicError> {
    socket.set_read_timeout(Some(timeout)).map_err(io_error)?;
    socket.set_write_timeout(Some(timeout)).map_err(io_error)?;
    *shared.socket.lock().unwrap_or_else(PoisonError::into_inner) =
        Some(socket.try_clone().map_err(io_error)?);
    // A stop that came while connecting.
    if shared.stop.load(Ordering::SeqCst) {
        let _ = socket.shutdown(Shutdown::Both);
    }
    let mut reader = BufReader::new(socket);

    let cookie = pulseaudio::cookie_path_from_env()
        .and_then(|path| std::fs::read(path).ok())
        .unwrap_or_default();
    let auth = AuthParams {
        version: protocol::MAX_VERSION,
        supports_shm: false,
        supports_memfd: false,
        cookie,
    };
    protocol::write_command_message(
        reader.get_mut(),
        0,
        &Command::Auth(auth),
        protocol::MAX_VERSION,
    )
    .map_err(protocol_error)?;
    let (_, auth_reply) =
        protocol::read_reply_message::<AuthReply>(&mut reader, protocol::MAX_VERSION)
            .map_err(protocol_error)?;
    let version = std::cmp::min(protocol::MAX_VERSION, auth_reply.version);

    let mut props = Props::new();
    props.set(Prop::ApplicationName, app_name);
    protocol::write_command_message(reader.get_mut(), 1, &Command::SetClientName(props), version)
        .map_err(protocol_error)?;
    protocol::read_reply_message::<SetClientNameReply>(&mut reader, version)
        .map_err(protocol_error)?;

    let params = RecordStreamParams {
        sample_spec: SampleSpec {
            format: SampleFormat::S16Le,
            channels: 1,
            sample_rate: SAMPLE_RATE,
        },
        channel_map: ChannelMap::new([ChannelPosition::Mono]),
        source_name: Some(CString::from(protocol::DEFAULT_SOURCE)),
        buffer_attr: BufferAttr {
            fragment_size: FRAGMENT_BYTES,
            ..BufferAttr::default()
        },
        ..RecordStreamParams::default()
    };
    protocol::write_command_message(
        reader.get_mut(),
        2,
        &Command::CreateRecordStream(params),
        version,
    )
    .map_err(protocol_error)?;
    let (_, stream) = protocol::read_reply_message::<CreateRecordStreamReply>(&mut reader, version)
        .map_err(protocol_error)?;
    // Recording reads block until audio comes; a stop shuts the socket down to end them.
    reader.get_ref().set_read_timeout(None).map_err(io_error)?;
    reader.get_ref().set_write_timeout(None).map_err(io_error)?;
    Ok((reader, stream.channel, version))
}

fn io_error(error: std::io::Error) -> MicError {
    MicError::Failed(error.to_string())
}

// Why the stream would not open, in words a user can act on.
fn protocol_error(error: ProtocolError) -> MicError {
    match error {
        ProtocolError::ServerError(PulseError::AccessDenied) => {
            MicError::PermissionDenied(LINUX_PERMISSION_DENIED_MESSAGE.to_owned())
        }
        ProtocolError::ServerError(PulseError::NoEntity) => {
            MicError::NoDevice(NO_INPUT_DEVICE_MESSAGE.to_owned())
        }
        ProtocolError::Timeout => MicError::Failed(LINUX_SERVER_TIMEOUT_MESSAGE.to_owned()),
        ProtocolError::Io(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            MicError::Failed(LINUX_SERVER_TIMEOUT_MESSAGE.to_owned())
        }
        other => MicError::Failed(other.to_string()),
    }
}

#[cfg(test)]
#[path = "linux_capture_tests.rs"]
mod tests;
