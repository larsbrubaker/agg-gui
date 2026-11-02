//! Reading what items hold into a tree of [`CopyNode`]s, and writing such a tree on disk: the two
//! halves of a copy that keeps the structure of a folder that is not on disk (another place's
//! folder, a read-only one). A file gives its bytes (awaited, so content that has to be
//! downloaded is), a folder on disk is read straight from the disk, and any other folder is
//! opened and listed ([`crate::folder_listing`], which waits for a folder that lists later, a
//! server's), giving its folders and then its files. A part that can't be read is named in the
//! failures and the rest is still gathered.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::rc::Rc;

use super::folder_link::DiskFolderLink;
use crate::folder::Folder;
use crate::folder_listing::folder_listing;
use crate::item::Item;
use crate::translate::tr;

/// One file or folder a copy carries, with everything it holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyNode {
    /// A file: its file name (with its extension) and its bytes.
    File {
        file_name: String,
        content: Rc<Vec<u8>>,
    },
    /// A folder and what is in it.
    Folder {
        name: String,
        children: Vec<CopyNode>,
    },
}

/// A name made safe to use as a file name: the characters no file name may hold (and a trailing
/// run of dots) become `_`. Another place may allow names a disk does not.
pub fn sanitize_file_name(name: &str) -> String {
    const INVALID: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    let mut out: String = name
        .chars()
        .map(|c| {
            if INVALID.contains(&c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = out.trim_end_matches('.').len();
    if trimmed < out.len() {
        out.truncate(trimmed);
        out.push('_');
    }
    out
}

/// Reads `items` into copy trees, in order: the trees of the items that could be read and a line
/// for each part that couldn't (a folder keeps whatever of it could be read).
pub async fn gather(items: &[Rc<dyn Item>]) -> (Vec<CopyNode>, Vec<String>) {
    let mut nodes = Vec::new();
    let mut failures = Vec::new();
    for item in items {
        if let Some(node) = gather_item(Rc::clone(item), &mut failures).await {
            nodes.push(node);
        }
    }
    (nodes, failures)
}

/// One item's tree, or `None` (with its failure noted) when nothing of it could be read. Boxed:
/// a folder gathers its folders through it.
pub fn gather_item<'a>(
    item: Rc<dyn Item>,
    failures: &'a mut Vec<String>,
) -> Pin<Box<dyn Future<Output = Option<CopyNode>> + 'a>> {
    Box::pin(async move {
        if let Some(dir) = item
            .as_any()
            .and_then(|a| a.downcast_ref::<DiskFolderLink>())
        {
            return match read_dir_node(dir.file_path()) {
                Ok(node) => Some(node),
                Err(e) => {
                    failures.push(format!("{}: {e}", item.name()));
                    None
                }
            };
        }
        if let Some(link) = item.as_container_link() {
            let folder = match link.get_container() {
                Ok(folder) => folder,
                Err(e) => {
                    failures.push(format!("{}: {e}", item.name()));
                    return None;
                }
            };
            let Ok(folder) = folder_listing(folder).await else {
                failures.push(format!(
                    "{}: {}",
                    item.name(),
                    tr("the folder couldn't be listed")
                ));
                return None;
            };
            return Some(gather_folder(item.name(), folder.as_ref(), failures).await);
        }
        let Some(stream) = item.as_asset_stream() else {
            failures.push(format!("{}: {}", item.name(), tr("it can't be copied")));
            return None;
        };
        let file_name = stream.file_name();
        match stream.get_stream_for_person().await {
            Ok(Some(content)) => Some(CopyNode::File {
                file_name,
                content: Rc::new(content),
            }),
            Ok(None) => {
                failures.push(format!(
                    "{}: {}",
                    item.name(),
                    tr("its content couldn't be downloaded")
                ));
                None
            }
            Err(e) => {
                failures.push(format!("{}: {}", item.name(), String::from(e)));
                None
            }
        }
    })
}

/// A listed folder's tree: its folders first, then its files, each in listing order.
async fn gather_folder(name: &str, folder: &dyn Folder, failures: &mut Vec<String>) -> CopyNode {
    let mut children = Vec::new();
    let all = folder
        .child_containers()
        .into_iter()
        .map(|f| f as Rc<dyn Item>)
        .chain(folder.items());
    for child in all {
        if let Some(node) = gather_item(child, failures).await {
            children.push(node);
        }
    }
    CopyNode::Folder {
        name: name.to_string(),
        children,
    }
}

/// The folder on disk at `path` as a tree, its entries in name order (an explicit sort: the
/// disk's own order differs between systems and reaches the copies' order).
pub fn read_dir_node(path: &Path) -> std::io::Result<CopyNode> {
    let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(path)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    let mut children = Vec::new();
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        // The Finder's hidden folder settings are not the user's work.
        if name == ".DS_Store" {
            continue;
        }
        if entry.file_type()?.is_dir() {
            children.push(read_dir_node(&entry.path())?);
        } else {
            children.push(CopyNode::File {
                file_name: name,
                content: Rc::new(std::fs::read(entry.path())?),
            });
        }
    }
    Ok(CopyNode::Folder {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        children,
    })
}

/// Writes the copy tree `node` at `to` (a folder's contents under their own names: the folder is
/// new, so nothing in it collides). A name another place allows but a disk doesn't is made safe.
pub fn write_node(to: &Path, node: &CopyNode) -> std::io::Result<()> {
    match node {
        CopyNode::File { content, .. } => std::fs::write(to, content.as_slice()),
        CopyNode::Folder { children, .. } => {
            std::fs::create_dir(to)?;
            for child in children {
                let name = match child {
                    CopyNode::File { file_name, .. } => file_name,
                    CopyNode::Folder { name, .. } => name,
                };
                write_node(&to.join(sanitize_file_name(name)), child)?;
            }
            Ok(())
        }
    }
}
