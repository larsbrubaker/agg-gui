//! The AppKit and Objective-C runtime code of the agg-gui crates, one
//! submodule per area, each behind its own Cargo feature (the rule they
//! follow: `src/lib.rs`). Built on objc2 0.6 and objc2-foundation /
//! objc2-app-kit 0.3, the versions winit 0.30 and wgpu already build.
//!
//! - `open_files` (feature `open-files`): makes the application delegate
//!   answer `application:openFiles:`, for agg-gui-shell's OS-delivered open
//!   requests (Finder double-click, Open With, a drop on the Dock icon).
//! - `menu` (feature `app-menu`): the Objective-C object that is the
//!   delegate of agg-gui-shell's native menus and the target of their items,
//!   calling back into a `menu::MenuHandler` the shell implements.

#[cfg(feature = "open-files")]
pub mod open_files;

#[cfg(feature = "app-menu")]
pub mod menu;
