//! [`ZipEntry`]: one file inside a `.zip` as an item. Its dates are the archive file's, and its ID
//! hashes the archive's path and the entry's path inside it (`disk_paths::utf16_hash`), the same
//! text-for-text as it has always been, since an app's thumbnail cache is keyed by it.
//! `get_stream` opens the archive and reads that entry's bytes; an entry that is no longer in the
//! archive gives an empty stream. The folder listing these is `folder`'s [`super::ZipFolder`].

use std::any::Any;
use std::path::{Path, PathBuf};

use crate::disk_paths::{file_dates, utf16_hash};
use crate::error::LoadError;
use crate::item::{Asset, AssetStream, Item};

/// One file inside a `.zip`.
#[derive(Debug, Clone)]
pub struct ZipEntry {
    file_path: PathBuf,
    relative_path: String,
    id: String,
    name: String,
    content_type: String,
    file_size: u64,
    dates: (i64, i64),
}

/// The last segment of a path inside an archive (either separator).
fn last_segment(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

impl ZipEntry {
    /// The entry at
    /// `relative_path` in the archive at `file_path`, `file_size` bytes uncompressed.
    pub fn new(
        file_path: impl Into<PathBuf>,
        relative_path: impl Into<String>,
        file_size: u64,
    ) -> Self {
        let file_path = file_path.into();
        let relative_path = relative_path.into();
        let name = last_segment(&relative_path).to_string();
        // The extension, lower case, without its dot.
        let content_type = match name.rfind('.') {
            Some(dot) => name[dot..].to_lowercase().trim_matches('.').to_string(),
            None => String::new(),
        };
        Self {
            id: utf16_hash(&format!("{}/{relative_path}", file_path.to_string_lossy())),
            dates: file_dates(&file_path),
            name,
            content_type,
            file_size,
            file_path,
            relative_path,
        }
    }

    /// The archive.
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// The entry's full name inside the archive.
    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    /// The entry's uncompressed length.
    pub fn file_size(&self) -> u64 {
        self.file_size
    }
}

impl Item for ZipEntry {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn is_protected(&self) -> bool {
        false
    }

    fn is_visible(&self) -> bool {
        true
    }

    fn date_created(&self) -> i64 {
        self.dates.0
    }

    fn date_modified(&self) -> i64 {
        self.dates.1
    }

    fn as_asset(&self) -> Option<&dyn Asset> {
        Some(self)
    }

    fn as_asset_stream(&self) -> Option<&dyn AssetStream> {
        Some(self)
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

impl Asset for ZipEntry {
    fn content_type(&self) -> &str {
        &self.content_type
    }

    fn category(&self) -> &str {
        ""
    }

    fn file_name(&self) -> String {
        self.name.clone()
    }

    /// There is no asset path.
    fn asset_path(&self) -> &str {
        ""
    }
}

impl AssetStream for ZipEntry {
    /// The entry's bytes, read out of the archive (empty when the archive no
    /// longer has the entry).
    fn get_stream(&self) -> Result<Option<Vec<u8>>, LoadError> {
        let fail = |e: &dyn std::fmt::Display| {
            LoadError::Failed(format!("{}: {e}", self.file_path.display()))
        };
        let file = std::fs::File::open(&self.file_path).map_err(|e| fail(&e))?;
        let mut archive = ::zip::ZipArchive::new(file).map_err(|e| fail(&e))?;
        let mut bytes = Vec::new();
        if let Ok(mut entry) = archive.by_name(&self.relative_path) {
            std::io::copy(&mut entry, &mut bytes).map_err(|e| fail(&e))?;
        }
        Ok(Some(bytes))
    }
}
