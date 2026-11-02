//! The waking directory monitor (feature `disk`): agg-gui-files' [`PollingDirectoryMonitor`]
//! whose every report also posts an empty action to the UI queue of the thread that asked to
//! watch. The report itself only marks the folder for a relist; the wake brings the frame in
//! which the list pumps it (`BrowserList::layout`, `BrowserState::pump_live_refresh`), so an
//! idle window shows a folder changed on disk without the user moving the mouse.
//!
//! An app registers it once at startup ([`register_directory_monitor`]); a browser on a disk
//! folder then relists when something outside the app changes the folder showing.

use std::path::Path;
use std::sync::Arc;

use agg_gui_files::disk::{
    set_directory_monitor, DirectoryChanged, DirectoryMonitor, DirectoryWatch,
    PollingDirectoryMonitor,
};

/// [`PollingDirectoryMonitor`], waking the UI on each report.
#[derive(Default)]
pub struct WakingDirectoryMonitor;

impl DirectoryMonitor for WakingDirectoryMonitor {
    fn watch(
        &self,
        directory_path: &Path,
        directory_changed: DirectoryChanged,
    ) -> Option<Box<dyn DirectoryWatch>> {
        // The watch reports on its own thread; the wake goes to the UI thread that asked to
        // watch, captured here, not to whatever queue an unbound thread would post to.
        let ui_queue = agg_gui::ui_thread::current_queue();
        PollingDirectoryMonitor.watch(
            directory_path,
            Arc::new(move || {
                directory_changed();
                ui_queue.run_on_idle(|| {});
            }),
        )
    }
}

/// Registers [`WakingDirectoryMonitor`] as the process's directory monitor.
pub fn register_directory_monitor() {
    set_directory_monitor(Some(Arc::new(WakingDirectoryMonitor)));
}
