//! The AppKit half of OS-delivered open requests: has agg-os make `NSApp`'s
//! delegate answer `application:openFiles:` (AppKit sends it for a Finder
//! double-click, Open With, a drop on the Dock icon, `open file` while the app
//! runs, and a Finder launch), with [`super::push_open_files`] receiving the
//! paths.
//!
//! The Objective-C runtime work (adding the method to winit's delegate class,
//! replying to AppKit) is in `agg_os::macos::open_files`, the workspace's home
//! for unsafe code; this file decides when to install it and logs a failure.

use agg_os::macos::open_files::{self, InstallError};
use objc2::MainThreadMarker;

/// Make the app delegate answer `application:openFiles:`. Call after
/// `EventLoop::new()` (which installs winit's delegate) and before the loop
/// runs, on the main thread. A no-op, logged, when no delegate is installed.
pub(crate) fn install() {
    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("agg-gui-shell: open-file events need the main thread; not installed");
        return;
    };
    match open_files::install(mtm, super::push_open_files) {
        Ok(()) => {}
        Err(InstallError::NoDelegate) => {
            log::warn!("agg-gui-shell: NSApp has no delegate; open-file events not installed");
        }
        Err(InstallError::AddMethodFailed) => {
            log::warn!("agg-gui-shell: could not add application:openFiles: to the app delegate");
        }
        Err(other) => log::warn!("agg-gui-shell: open-file events not installed: {other}"),
    }
}
