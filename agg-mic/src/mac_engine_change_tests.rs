//! Tests for `mac_engine_change.rs` without a microphone: the configuration change is posted by
//! hand, for a stand-in object in place of the engine. A posted change fails the recording once;
//! one for another engine, or after the observer is dropped (a stop), reports nothing. That a real
//! AVAudioEngine posts it when a device is unplugged can only be seen live (`tests/live_mic.rs`,
//! `AGG_MIC_UNPLUG_TEST=1`).

use std::sync::{Arc, Mutex};

use objc2_foundation::{NSNotificationCenter, NSObject};

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

/// Posts the engine's configuration change for `object`, as AVAudioEngine does.
fn post_change(object: &NSObject) {
    // SAFETY: the notification name AVFAudio defines and a plain object as its poster.
    unsafe {
        NSNotificationCenter::defaultCenter().postNotificationName_object(
            AVAudioEngineConfigurationChangeNotification,
            Some(object.as_ref()),
        );
    }
}

#[test]
fn a_configuration_change_fails_the_recording_once() {
    let engine = NSObject::new();
    let (delivery, failed) = delivery();
    let _observer = ChangeObserver::watch(engine.as_ref(), delivery.clone());
    post_change(&engine);
    post_change(&engine);
    assert_eq!(
        *failed.lock().unwrap(),
        vec![MicError::NoDevice(INPUT_CHANGED_MESSAGE.to_owned())]
    );
    assert!(!delivery.is_open(), "no chunk follows the failure");
}

#[test]
fn another_engines_change_is_not_ours() {
    let (engine, other) = (NSObject::new(), NSObject::new());
    let (delivery, failed) = delivery();
    let _observer = ChangeObserver::watch(engine.as_ref(), delivery.clone());
    post_change(&other);
    assert!(failed.lock().unwrap().is_empty());
    assert!(delivery.is_open());
}

#[test]
fn nothing_is_reported_once_the_observer_is_dropped() {
    let engine = NSObject::new();
    let (delivery, failed) = delivery();
    drop(ChangeObserver::watch(engine.as_ref(), delivery.clone()));
    post_change(&engine);
    assert!(failed.lock().unwrap().is_empty());
    assert!(delivery.is_open(), "the observer is gone, not the delivery");
}
