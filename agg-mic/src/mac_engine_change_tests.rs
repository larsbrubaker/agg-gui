//! Tests for `mac_engine_change.rs` without a microphone: the configuration change's reporter,
//! called as agg-os's observer calls it, fails the recording once however often the change comes.
//! That the observer calls it for its own engine only, and not after a stop removed it, is tested
//! beside the observer in agg-os (`macos/audio_input/tests.rs`). That a real AVAudioEngine posts
//! the change when a device is unplugged can only be seen live (`tests/live_mic.rs`,
//! `AGG_MIC_UNPLUG_TEST=1`).

use std::sync::{Arc, Mutex};

use super::*;

/// A delivery whose failures are kept, and the kept failures.
fn delivery() -> (Delivery, Arc<Mutex<Vec<MicError>>>) {
    let failed: Arc<Mutex<Vec<MicError>>> = Arc::default();
    let into = Arc::clone(&failed);
    let delivery = Delivery::new(
        Box::new(|_: &[f32], _: u32| {}),
        Box::new(move |error| into.lock().unwrap().push(error)),
    );
    (delivery, failed)
}

#[test]
fn a_configuration_change_fails_the_recording_once() {
    let (delivery, failed) = delivery();
    let on_change = input_change_reporter(delivery.clone());
    on_change();
    on_change();
    assert_eq!(
        *failed.lock().unwrap(),
        vec![MicError::NoDevice(INPUT_CHANGED_MESSAGE.to_owned())]
    );
    assert!(!delivery.is_open(), "no chunk follows the failure");
}
