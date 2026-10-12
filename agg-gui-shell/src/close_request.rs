//! What a native close request does: macOS's red button and Cmd-Q,
//! `WM_CLOSE`, `WM_DELETE_WINDOW`.
//!
//! Ports the decision in agg-sharp `Gui/Platform/PlatformCloseArbitration.cs`
//! (`HandlePlatformCloseRequest`). C#'s single window mode re-points the host
//! at whatever dialog is on top, so it has to route the close back to the
//! application shell and undo the host's "closing" flag when the shell
//! vetoes on a second ask. Here dialogs are widgets inside the one window's
//! tree, so the close always goes to the application's [`ShellHost`], asked
//! exactly once per request, and the shell holds no mid-close state to undo.
//! `shell_loop.rs` calls this on `WindowEvent::CloseRequested` and ends the
//! event loop when it returns `true`.

use agg_gui::App;

use crate::ShellHost;

/// Ask the application whether the window may close now. `true` ends the
/// event loop; `false` keeps the native window, whatever dialog is open.
pub(crate) fn close_requested<H: ShellHost>(host: &mut H, app: &mut App) -> bool {
    host.on_close_requested(app)
}

#[cfg(test)]
#[path = "close_request_tests.rs"]
mod tests;
