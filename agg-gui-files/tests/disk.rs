//! The disk provider (`agg_gui_files::disk`, the `disk` feature): folders on disk listing through
//! their rules, the changes they take, the IDs and keys that keep disk items' identity, and the
//! rules' read-only refusal. Every test works in a fresh temporary folder, and the trash a remove
//! uses is a temporary folder too, so a test run never touches the developer's own files or
//! Trash.
#![cfg(feature = "disk")]

#[path = "disk/ids.rs"]
mod ids;
#[path = "disk/listing.rs"]
mod listing;
#[path = "disk/places.rs"]
mod places;
#[path = "disk/read_only.rs"]
mod read_only;
#[path = "disk/temp.rs"]
mod temp;
#[path = "disk/writes.rs"]
mod writes;
