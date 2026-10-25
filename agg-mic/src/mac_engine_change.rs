//! How the macOS capture (`mac_capture`) hears that its microphone went away. When the input
//! device is unplugged, the default input switches to another device, or its channel count or
//! sample rate changes, AVAudioEngine stops itself and posts
//! `AVAudioEngineConfigurationChangeNotification` for that engine; the tap then delivers nothing
//! more. A [`ChangeObserver`] watches for it on one engine and ends the recording through
//! [`Delivery::fail`] with [`INPUT_CHANGED_MESSAGE`], as cpal reported a lost device before.
//! Dropping the observer removes it, which the capture does first thing in a stop.
//!
//! (AVAudioSession's interruption and route-change notifications are iOS-only; on macOS the
//! engine's configuration change is the one signal.)
//!
//! The block runs on the engine's internal queue, so it only reports: it never touches the engine,
//! and holds no reference to it (Apple warns that an engine released from inside the handler can
//! deadlock).

use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_avf_audio::AVAudioEngineConfigurationChangeNotification;
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol};

use crate::delivery::Delivery;
use crate::{MicError, INPUT_CHANGED_MESSAGE};

/// Watches one engine's configuration changes while it records (see the module docs).
pub(crate) struct ChangeObserver {
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

// SAFETY: the token is only handed back to NSNotificationCenter, which is thread-safe, and
// Objective-C objects may be retained and released from any thread.
unsafe impl Send for ChangeObserver {}

impl ChangeObserver {
    /// Fails `delivery` when `engine` (the AVAudioEngine, or any object a test posts for) posts
    /// a configuration change.
    pub(crate) fn watch(engine: &AnyObject, delivery: Delivery) -> ChangeObserver {
        let block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
            delivery.fail(MicError::NoDevice(INPUT_CHANGED_MESSAGE.to_owned()));
        });
        // SAFETY: a notification name AVFAudio defines, the engine as the posting object, no
        // queue (the block runs on the posting thread), and a block of the documented
        // `void (^)(NSNotification *)` shape, which is sendable: it holds only a `Delivery`.
        let token = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(AVAudioEngineConfigurationChangeNotification),
                Some(engine),
                None,
                &block,
            )
        };
        ChangeObserver { token }
    }
}

impl Drop for ChangeObserver {
    fn drop(&mut self) {
        // SAFETY: the token is the observer `addObserverForName:...` returned.
        unsafe {
            NSNotificationCenter::defaultCenter().removeObserver(self.token.as_ref());
        }
    }
}

#[cfg(test)]
#[path = "mac_engine_change_tests.rs"]
mod tests;
