//! Tests for `linux_capture.rs` without a sound server: a server that accepts the connection but
//! never answers fails the start within the handshake timeout, instead of hanging it forever. (A
//! real server's answers are checked live, `tests/live_mic.rs`.) Linux only: run on a Linux
//! machine or CI.

use std::os::unix::net::UnixListener;
use std::time::{Duration, Instant};

use super::*;

#[test]
fn a_sound_server_that_never_answers_times_out() {
    let path = std::env::temp_dir().join(format!("agg-mic-silent-server-{}", std::process::id()));
    let _ = std::fs::remove_file(&path);
    // Bound and listening, so the connection is accepted by the kernel, but nothing ever reads
    // or answers it.
    let listener = UnixListener::bind(&path).expect("a socket to listen on");
    let socket = UnixStream::connect(&path).expect("connected");
    let shared = Shared {
        stop: AtomicBool::new(false),
        socket: Mutex::new(None),
        delivery: Delivery::new(Box::new(|_: &[f32], _: u32| {}), Box::new(|_| {})),
    };
    let began = Instant::now();
    let result = handshake(
        &shared,
        c"agg-mic tests",
        socket,
        Duration::from_millis(200),
    );
    let waited = began.elapsed();
    drop(listener);
    let _ = std::fs::remove_file(&path);
    assert!(
        matches!(&result, Err(MicError::Failed(message)) if message == LINUX_SERVER_TIMEOUT_MESSAGE),
        "{:?}",
        result.err()
    );
    assert!(waited < Duration::from_secs(5), "waited {waited:?}");
}
