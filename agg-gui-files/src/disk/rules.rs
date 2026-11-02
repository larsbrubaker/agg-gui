//! [`DiskRules`]: what an app decides about its folders on disk. A [`super::DiskFolder`] lists
//! only the files its rules accept (folders always list), and refuses every change when its rules
//! are read-only. The rules travel with the folder: each folder row it lists carries them, and
//! the folder that row opens is made with them, so one rule covers a folder and everything inside
//! it. [`AllFiles`] lists every file and allows every change.

/// What an app decides about its folders on disk.
pub trait DiskRules {
    /// Whether a file named `name` (its last path segment) is listed.
    fn lists_file(&self, name: &str) -> bool;

    /// Whether the folder refuses every change: its `allow_action` answers no to all of them,
    /// and adding, renaming, removing and moving in fail without touching the disk. False by
    /// default.
    fn read_only(&self) -> bool {
        false
    }
}

/// The rules that list every file and allow every change.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AllFiles;

impl DiskRules for AllFiles {
    fn lists_file(&self, _name: &str) -> bool {
        true
    }
}
