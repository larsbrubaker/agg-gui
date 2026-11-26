//! The Windows calls of the agg-gui crates, one submodule per area, each
//! behind its own Cargo feature (the rule they follow: `src/lib.rs`).
//!
//! - [`desktop_settings`] (feature `desktop-settings`): one read of a desktop
//!   setting per function, for agg-gui-shell (tooltip hover time, double-click
//!   settings, the cursor position, font smoothing and the primary display's
//!   rotation). Built on windows-sys 0.52, the version winit 0.30 builds.
//! - [`recycle_bin`] (feature `recycle-bin`): moving a file to the Recycle Bin
//!   and the reads around it, for agg-gui-files' trash. Built on
//!   windows-sys 0.61, the version agg-gui-files has always built.

#[cfg(feature = "desktop-settings")]
pub mod desktop_settings;

#[cfg(feature = "recycle-bin")]
pub mod recycle_bin;
