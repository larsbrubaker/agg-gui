//! A temporary folder removed when dropped, and an archive of named text entries written with
//! the `zip` crate.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A fresh, empty folder under the system's temporary folder.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("agg-gui-files-zip-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).expect("the temporary folder is made");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An archive holding each `(name, text)` entry, in order.
pub fn zip_of(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, text) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .expect("an entry");
        zip.write_all(text.as_bytes()).expect("its text");
    }
    zip.finish().expect("the zip").into_inner()
}
