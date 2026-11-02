//! Folders on the computer's disk (the `disk` feature): [`DiskFolder`] lists a folder and takes
//! changes, [`DiskFolderLink`] is a sub-folder's row, [`DiskFile`] a file. What an app decides
//! about its folders is its [`DiskRules`] (which files list, whether a folder may change), handed
//! to a folder and passed by each of its rows to the folder they open.
//!
//! Disk items keep their identity across listings: an item's ID hashes its path and last write
//! time (`paths`), the same text-for-text as it has always been, since an app's thumbnail cache
//! is keyed by it; a folder and its row answer the same location key (its path,
//! [`disk_location_key`]) and the same place, the computer.
//!
//! Files: `rules` ([`DiskRules`], [`AllFiles`]), `paths` (names, dates, IDs, free names, location
//! keys), `file`, `folder_link`, `folder` (the folder and its listing), `folder_writes` (add,
//! remove, rename, move; [`MovedPath`]), `copy` (copying in, keeping the originals) and `trash`
//! (the system trash a remove sends items to, and puts them back from).
//!
//! Everything here reads `std::fs`. In the browser (wasm32-unknown-unknown) there is no disk:
//! every read fails, so a folder lists its "Missing directory" row and every write fails.

mod copy;
mod file;
mod folder;
mod folder_link;
mod folder_writes;
mod paths;
mod rules;
mod trash;

pub use file::{subscribe_file_renamed, DiskFile, FileRenamedListener};
pub use folder::DiskFolder;
pub use folder_link::DiskFolderLink;
pub use folder_writes::MovedPath;
pub use paths::{
    disk_item_id, disk_item_id_at, disk_location_key, extension_lower, round_trip_utc,
    with_extension,
};
pub use rules::{AllFiles, DiskRules};
#[cfg(unix)]
pub use trash::move_to_freedesktop_trash;
pub use trash::{
    move_to_system_trash, move_to_trash, newest_recycle_bin_match, parse_recycle_bin_record,
    restore_from_trash, set_trash_directory, TrashedItem,
};
