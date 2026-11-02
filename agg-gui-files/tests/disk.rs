//! The disk provider (`agg_gui_files::disk`, the `disk` feature): folders on disk listing through
//! their rules, the changes they take, the IDs and keys that keep disk items' identity, and the
//! rules' read-only refusal. Every test works in a fresh temporary folder, and the trash a remove
//! uses is a temporary folder too, so a test run never touches the developer's own files or
//! Trash.
#![cfg(feature = "disk")]

#[path = "disk/computer.rs"]
mod computer;
#[path = "disk/copies.rs"]
mod copies;
#[path = "disk/history.rs"]
mod history;
#[path = "disk/ids.rs"]
mod ids;
#[path = "support/late_folder.rs"]
mod late_folder;
#[path = "disk/listing.rs"]
mod listing;
#[path = "disk/live_refresh.rs"]
mod live_refresh;
#[path = "disk/monitor.rs"]
mod monitor;
#[path = "disk/places.rs"]
mod places;
#[path = "disk/quick_look.rs"]
mod quick_look;
#[path = "disk/read_only.rs"]
mod read_only;
#[path = "disk/search.rs"]
mod search;
#[path = "disk/temp.rs"]
mod temp;
#[path = "disk/trash.rs"]
mod trash;
#[path = "disk/writes.rs"]
mod writes;
#[cfg(feature = "zip")]
#[path = "disk/zip_in_folder.rs"]
mod zip_in_folder;
