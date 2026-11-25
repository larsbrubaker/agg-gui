//! The waking directory monitor (feature `disk`): agg-gui-files' [`PollingDirectoryMonitor`]
//! whose every report also posts an empty action to the UI queue of the thread that asked to
//! watch. The report itself only marks the folder for a relist; the wake brings the frame in
//! which the list pumps it (`BrowserList::layout`, `BrowserState::pump_live_refresh`), so an
//! idle window shows a folder changed on disk without the user moving the mouse. Like the polling
//! monitor, `watch` returns at once without reading the disk, and a folder that cannot be read
//! yet (a network share that is away) is watched and wakes the UI once it can be.
//!
//! An app registers it once at startup ([`register_directory_monitor`]); a browser on a disk
//! folder then relists when something outside the app changes the folder showing.

use std::path::Path;
use std::sync::Arc;

use agg_gui_files::disk::{
    set_directory_monitor, DirectoryChanged, DirectoryMonitor, DirectoryWatch,
    PollingDirectoryMonitor, PollingWatch,
};

/// [`PollingDirectoryMonitor`], waking the UI on each report.
#[derive(Default)]
pub struct WakingDirectoryMonitor;

impl WakingDirectoryMonitor {
    /// [`DirectoryMonitor::watch`] as the concrete [`PollingWatch`], which can also say when its
    /// first listing has been taken (`PollingWatch::wait_for_first_listing`). Returns at once
    /// without reading the disk; `None` where [`PollingDirectoryMonitor::watch_directory`] is.
    pub fn watch_directory(
        &self,
        directory_path: &Path,
        directory_changed: DirectoryChanged,
    ) -> Option<PollingWatch> {
        // The watch reports on its own thread; the wake goes to the UI thread that asked to
        // watch, captured here, not to whatever queue an unbound thread would post to.
        let ui_queue = agg_gui::ui_thread::current_queue();
        PollingDirectoryMonitor.watch_directory(
            directory_path,
            Arc::new(move || {
                directory_changed();
                ui_queue.run_on_idle(|| {});
            }),
        )
    }
}

impl DirectoryMonitor for WakingDirectoryMonitor {
    fn watch(
        &self,
        directory_path: &Path,
        directory_changed: DirectoryChanged,
    ) -> Option<Box<dyn DirectoryWatch>> {
        let watch = self.watch_directory(directory_path, directory_changed)?;
        Some(Box::new(watch))
    }
}

/// Registers [`WakingDirectoryMonitor`] as the process's directory monitor.
pub fn register_directory_monitor() {
    set_directory_monitor(Some(Arc::new(WakingDirectoryMonitor)));
}
