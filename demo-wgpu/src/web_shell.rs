//! Deprecated compatibility wrapper over [`agg_gui_web_shell`].
//!
//! This module used to *be* the browser shell. It now forwards to the
//! `agg-gui-web-shell` crate, the published, maintained version of the same
//! thing plus what AtomArtist's hand-rolled shell had that this one didn't
//! (the first-paint guarantee, reactive/continuous run modes with deadline
//! wake-ups, pointer-leave, page-hide flush hooks, a window-level
//! pointer-release resync, device-loss rebuild, WebGPU, localStorage
//! settings).
//!
//! The old entry points are kept so external path-dependency consumers keep
//! compiling. Behaviour changes a caller can observe:
//!
//! - `build_app` runs once the GPU is up (like the native shell), not
//!   synchronously inside `start`; until then [`with_app`] is a no-op.
//! - The `on_frame` hook runs on every animation frame only once the app is
//!   built (it used to run from the very first frame).
//! - Start-up failures are logged with `console.error` and replace the canvas
//!   with a readable error panel (previously a blank canvas); a missing canvas
//!   is still only logged.
//! - The first frame always paints, and reactive mode also wakes for
//!   scheduled deadlines (`App::next_draw_deadline` — cursor blink, delayed
//!   tooltips); mouse moves still only paint through `App::wants_draw()`.
//! - Pointer-leave clears hover; the canvas backing store is fitted to the
//!   device's texture limit (scale reduced uniformly, pointer mapping kept
//!   consistent) instead of overflowing it; a lost GPU device is rebuilt
//!   (with backoff), a lost WebGL2 context shows the error panel.
//!
//! New code
//! should call [`agg_gui_web_shell::start`] directly, which gives it a real
//! error type, a backend choice, and the [`agg_gui_web_shell::WebShellHost`]
//! hooks:
//!
//! ```ignore
//! agg_gui_web_shell::start(
//!     agg_gui_web_shell::WebShellConfig::new("canvas"),
//!     |_init| Ok((build_my_app(), agg_gui_web_shell::NoHost)),
//! )
//! ```
//!
//! The native equivalent is [`agg_gui_shell`] (see `crate::native_shell` on
//! native targets).

use agg_gui::App;
use agg_gui_web_shell::{Backend, WebShellConfig, WebShellHost};

/// A per-tick closure in [`WebShellHost`] clothing — all this wrapper's
/// callers ever supplied.
struct OnTickHost<F: FnMut()>(F);

impl<F: FnMut()> WebShellHost for OnTickHost<F> {
    fn on_tick(&mut self, _app: &mut App) {
        (self.0)();
    }
}

/// Request a repaint on the next animation frame.
#[deprecated(since = "0.5.0", note = "use agg_gui_web_shell::mark_dirty")]
pub fn mark_dirty() {
    agg_gui_web_shell::mark_dirty();
}

/// Run `f` with the shell-owned [`App`], if it is built and not borrowed.
#[deprecated(since = "0.5.0", note = "use agg_gui_web_shell::with_app")]
pub fn with_app(f: impl FnOnce(&mut App)) {
    let _ = agg_gui_web_shell::with_app(f);
}

/// Boot the shell on `#canvas_id` (WebGL2, as this shell always was).
/// `on_frame` runs on every animation-frame tick, painted or not.
#[deprecated(
    since = "0.5.0",
    note = "use agg_gui_web_shell::start, which reports errors and takes WebShellHost hooks"
)]
pub fn start(
    canvas_id: &str,
    build_app: impl FnOnce() -> App + 'static,
    on_frame: impl FnMut() + 'static,
) {
    let config = WebShellConfig::new(canvas_id)
        .with_backend(Backend::WebGl2)
        .with_device_label("agg-gui-web-shell");
    if let Err(err) =
        agg_gui_web_shell::start(config, move |_init| Ok((build_app(), OnTickHost(on_frame))))
    {
        log::error!("web shell: {err}");
    }
}
