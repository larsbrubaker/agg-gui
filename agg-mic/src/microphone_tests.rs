//! Tests for `microphone.rs` that need its private parts: the backend every platform without a
//! capture gets (wasm32 among them) says so and refuses to start. The public behaviour, over a
//! fake backend, is tested in `tests/microphone.rs`.

use super::*;
use crate::UNSUPPORTED_MESSAGE;

#[test]
fn a_platform_without_a_capture_says_so_and_refuses_to_start() {
    let microphone = Microphone::with_backend(Unsupported);
    assert!(!microphone.is_available());
    let started = microphone.start(Box::new(|_: &[f32], _: u32| {}), Box::new(|_| {}));
    assert_eq!(
        started,
        Err(MicError::Unsupported(UNSUPPORTED_MESSAGE.to_owned()))
    );
    assert!(!microphone.is_recording());
    // Stopping what never started is harmless.
    microphone.stop();
}
