//! [`WhistleVoice`], the [`VoiceInput`] the keyboard drives: each call goes
//! to the state machine (`machine.rs`) through the platform's host, which
//! first takes in what arrived (engine replies, microphone events), then
//! runs the commands the call queued. The host is `web::WebHost` on wasm and
//! `native::NativeHost` elsewhere.

use agg_gui::widgets::on_screen_keyboard::{VoiceContext, VoiceInput, VoicePoll, VoiceState};

use crate::config::Config;
use crate::machine::Machine;

/// A platform's side of the provider.
pub(crate) trait Host {
    /// Apply `f` to the machine, then run the commands it queued.
    fn dispatch(&self, f: &mut dyn FnMut(&mut Machine));

    /// The keyboard's `start`, called inside the user's tap: a host may
    /// do there what only a user gesture allows (the web host starts its
    /// audio).
    fn start(&self, ctx: &VoiceContext) {
        self.dispatch(&mut |m| m.start(ctx));
    }
}

#[cfg(target_arch = "wasm32")]
type PlatformHost = crate::web::WebHost;
#[cfg(not(target_arch = "wasm32"))]
type PlatformHost = crate::native::NativeHost;

/// Dictation for agg-gui's on-screen keyboard on whistle. Register it with
/// [`crate::install`], or build one with [`WhistleVoice::new`] and pass it
/// to `agg_gui::widgets::on_screen_keyboard::set_voice_input` yourself.
/// Create it on the UI thread.
pub struct WhistleVoice {
    host: PlatformHost,
}

impl WhistleVoice {
    pub fn new(config: Config) -> WhistleVoice {
        WhistleVoice {
            host: PlatformHost::new(config),
        }
    }
}

impl VoiceInput for WhistleVoice {
    fn prepare(&mut self) {
        self.host.dispatch(&mut |m| m.prepare());
    }

    fn start(&mut self, ctx: &VoiceContext) {
        self.host.start(ctx);
    }

    fn stop(&mut self) {
        self.host.dispatch(&mut |m| m.stop());
    }

    fn cancel(&mut self) {
        self.host.dispatch(&mut |m| m.cancel());
    }

    fn poll(&mut self) -> VoicePoll {
        let mut out = None;
        self.host.dispatch(&mut |m| out = Some(m.poll()));
        // `dispatch` always runs `f`; the fallback only satisfies the type.
        out.unwrap_or(VoicePoll {
            state: VoiceState::Preparing { progress: None },
            text: None,
        })
    }
}
