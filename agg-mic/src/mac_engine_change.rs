//! How the macOS capture (`mac_capture`) reports that its microphone went away. When the input
//! device is unplugged, the default input switches to another device, or its channel count or
//! sample rate changes, AVAudioEngine stops itself and posts
//! `AVAudioEngineConfigurationChangeNotification` for that engine; the sink node then delivers
//! nothing more. agg-os observes it (`agg_os::macos::audio_input`, from before the engine starts
//! until a stop removes the observer first thing) and calls the closure
//! [`input_change_reporter`] makes, which ends the recording through [`Delivery::fail`] with
//! [`INPUT_CHANGED_MESSAGE`], as cpal reported a lost device before.
//!
//! (AVAudioSession's interruption and route-change notifications are iOS-only; on macOS the
//! engine's configuration change is the one signal.)
//!
//! The closure runs on the engine's internal queue, so it only reports: it never touches the
//! engine, and holds no reference to it (Apple warns that an engine released from inside the
//! handler can deadlock).

use crate::delivery::Delivery;
use crate::{MicError, INPUT_CHANGED_MESSAGE};

/// What a configuration change of the recording's engine does: fails `delivery` (see the module
/// docs). Only the first failure reaches the consumer; later changes find the delivery closed.
pub(crate) fn input_change_reporter(delivery: Delivery) -> impl Fn() + Send + Sync + 'static {
    move || delivery.fail(MicError::NoDevice(INPUT_CHANGED_MESSAGE.to_owned()))
}

#[cfg(test)]
#[path = "mac_engine_change_tests.rs"]
mod tests;
