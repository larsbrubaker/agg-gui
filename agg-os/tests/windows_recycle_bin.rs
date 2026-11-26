//! `agg_os::windows::recycle_bin`: a path with a NUL character is refused by
//! all three calls, and the temp directory's long form is a directory. Only
//! what holds on any Windows machine, a headless CI runner included, is
//! asserted, and nothing is left behind. The recycle itself is exercised by
//! agg-gui-files' `tests/disk/trash.rs::system_trash_round_trip` (recycle,
//! find, restore), which removes what it recycles.

#![cfg(all(windows, feature = "recycle-bin"))]

use std::path::{Path, PathBuf};

use agg_os::windows::recycle_bin::{long_path_name, recycle, recycle_bin_item_count, RecycleError};

/// A path in the temp directory whose second component holds a NUL.
fn path_with_nul() -> PathBuf {
    std::env::temp_dir().join("agg-os\0other")
}

#[test]
fn a_path_with_a_nul_is_refused() {
    assert_eq!(recycle(&path_with_nul()), Err(RecycleError::InteriorNul));
    assert_eq!(long_path_name(&path_with_nul()), None);
    assert_eq!(recycle_bin_item_count(Path::new("C:\\\0")), None);
}

#[test]
fn the_temp_directory_has_a_long_form_that_exists() {
    let long = long_path_name(&std::env::temp_dir()).expect("the temp directory's long form");
    assert!(long.is_dir(), "{} is not a directory", long.display());
}
