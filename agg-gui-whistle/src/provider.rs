//! [`WhistleVoice`], the [`VoiceInput`] the keyboard drives: a thin adapter
//! over [`Dictation`] (`dictation.rs`), which sends each call to the state
//! machine (`machine.rs`) through the platform's host. A host first takes in
//! what arrived (engine replies, microphone events), then runs the commands
//! the call queued. The host is `web::WebHost` on wasm and
//! `native::NativeHost` elsewhere.

use agg_gui::widgets::on_screen_keyboard::{VoiceContext, VoiceInput, VoicePoll};

use crate::config::Config;
use crate::dictation::{Dictation, DictationOptions};
use crate::machine::Machine;

/// A platform's side of the provider.
pub(crate) trait Host {
    /// Apply `f` to the machine, then run the commands it queued. The
    /// keyboard's `start` comes through here inside the user's tap, so what
    /// it opens (the web host's audio and microphone request) starts there.
    fn dispatch(&self, f: &mut dyn FnMut(&mut Machine));
}

#[cfg(target_arch = "wasm32")]
pub(crate) type PlatformHost = crate::web::WebHost;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) type PlatformHost = crate::native::NativeHost;

/// Dictation for agg-gui's on-screen keyboard on whistle. Register it with
/// [`crate::install`], or build one with [`WhistleVoice::new`] and pass it
/// to `agg_gui::widgets::on_screen_keyboard::set_voice_input` yourself.
/// Create it on the UI thread. It loads the model on the first tap on the
/// microphone; call [`VoiceInput::prepare`] on it before registering it to
/// load it sooner (the keyboard never does). The keyboard's dictation asks
/// for no full pass.
pub struct WhistleVoice {
    dictation: Dictation,
}

impl WhistleVoice {
    pub fn new(config: Config) -> WhistleVoice {
        WhistleVoice {
            dictation: Dictation::new(config),
        }
    }
}

impl VoiceInput for WhistleVoice {
    fn prepare(&mut self) {
        self.dictation.prepare();
    }

    fn start(&mut self, ctx: &VoiceContext) {
        self.dictation.start(&DictationOptions::from(ctx));
    }

    fn stop(&mut self) {
        self.dictation.stop();
    }

    fn cancel(&mut self) {
        self.dictation.cancel();
    }

    fn poll(&mut self) -> VoicePoll {
        self.dictation.poll().into()
    }
}
