//! How an [`super::InputEngine`] hears that its input went away. When the
//! input device is unplugged, the default input switches to another device,
//! or its channel count or sample rate changes, AVAudioEngine stops itself
//! and posts `AVAudioEngineConfigurationChangeNotification` for that engine;
//! the sink node then receives nothing more. A [`ChangeObserver`] watches for
//! it on one engine and calls its closure each time; agg-mic's
//! `mac_engine_change.rs` turns that into the end of the recording. Dropping
//! the observer removes it, which a stop does first.
//!
//! (AVAudioSession's interruption and route-change notifications are
//! iOS-only; on macOS the engine's configuration change is the one signal.)
//!
//! The block runs on the engine's internal queue, so the closure only
//! reports: the block holds no reference to the engine (Apple warns that an
//! engine released from inside the handler can deadlock). The closure runs
//! inside a [`CallbackScope`] naming its engine's change callback, so an
//! engine the closure drops is left running rather than torn down under the
//! call, and its observer, this block's, is left in place rather than
//! removed. A panic in the closure is caught, so it does not unwind into
//! Foundation.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_avf_audio::AVAudioEngineConfigurationChangeNotification;
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol};

use super::{CallbackKind, CallbackScope};

/// Watches one engine's configuration changes while it records (see the
/// module docs). Not `Send` itself: the [`super::InputEngine`] that holds it
/// says why it may move.
pub(super) struct ChangeObserver {
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

impl ChangeObserver {
    /// Calls `on_change` when `engine` (the AVAudioEngine, or any object a
    /// test posts for) posts a configuration change, inside a
    /// [`CallbackScope`] naming the change callback of `id`, its
    /// [`super::InputEngine`]'s id.
    pub(super) fn watch(
        engine: &AnyObject,
        id: u64,
        on_change: impl Fn() + Send + Sync + 'static,
    ) -> ChangeObserver {
        let block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                let _scope = CallbackScope::enter(id, CallbackKind::Change);
                on_change();
            }));
        });
        // SAFETY: a notification name AVFAudio defines, the engine as the posting object, no
        // queue (the block runs on the posting thread), and a block of the documented
        // `void (^)(NSNotification *)` shape, which is sendable: its closure is `Send + Sync`.
        // The center keeps the block until the observer is removed, which its engine never does
        // from inside the block's own call (`InputEngine::unobserve`, which `leak` goes through,
        // forgets the observer when this engine's change scope is the running one), so the
        // closure outlives every call.
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
