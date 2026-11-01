//! What a folder lists: [`Item`]s, and the kinds an item can also be. An [`Asset`] is content (a
//! file, a generated part), an [`AssetStream`] content with a stream of bytes, a [`FolderLink`] a
//! row that opens another folder ([`crate::Folder`], in `folder`), and a [`Thumbnail`] an item
//! that draws its own picture. An item tells which it is by answering the matching `as_*` method;
//! an app reaches its own item types through `as_any`.
//!
//! `get_stream` answers at once for content at hand, and `get_stream_async` (by default that
//! answer, ready) is what an item that downloads overrides. The futures here are local (no
//! `Send`), run on the caller's executor (see the crate header).

use std::any::Any;
use std::future::Future;
use std::pin::Pin;

use crate::error::LoadError;
use crate::folder::Folder;
use crate::picture::Picture;
use crate::place::PlaceId;

/// Anything a folder lists.
pub trait Item {
    /// Stable across sessions (the unlocalized name for catalog items).
    fn id(&self) -> &str;
    /// What the list shows.
    fn name(&self) -> &str;
    /// Cannot be renamed or removed.
    fn is_protected(&self) -> bool;
    /// Whether the list shows the item.
    fn is_visible(&self) -> bool;
    /// When the item was created, as ticks (`sort`). Items with no date of their own (made in
    /// memory as the list builds) answer 0, so a date sort keeps the folder's order: the time a
    /// list happened to build an item says nothing about the item.
    fn date_created(&self) -> i64 {
        0
    }
    /// When the item was last changed, as ticks; 0 like `date_created` for items with no date.
    fn date_modified(&self) -> i64 {
        0
    }
    /// The item as content, if it is content.
    fn as_asset(&self) -> Option<&dyn Asset> {
        None
    }
    /// The item as a link that opens a folder, if it is one.
    fn as_container_link(&self) -> Option<&dyn FolderLink> {
        None
    }
    /// The item as content with a stream of bytes, if it is one.
    fn as_asset_stream(&self) -> Option<&dyn AssetStream> {
        None
    }
    /// The concrete item, for the folders and apps that act on their own item types (a folder on
    /// disk adding, moving or removing its own file items).
    fn as_any(&self) -> Option<&dyn Any> {
        None
    }
    /// An item that supplies its own picture.
    fn as_thumbnail(&self) -> Option<&dyn Thumbnail> {
        None
    }
    /// A short line about the item under its name in the list view and in its tile's tooltip
    /// (who shared it with the user, say), already translated; `None` (the default) for an item
    /// with nothing more to say.
    fn detail_line(&self) -> Option<String> {
        None
    }
}

/// An item that supplies its own picture.
pub trait Thumbnail {
    /// The picture at this size, `Ok(None)` when the item has none to give, or the failure that
    /// made it fail (which the list row hands to the fault reporter); a future run on the
    /// caller's executor.
    fn get_thumbnail(&self, width: i32, height: i32) -> PictureFuture;
}

/// A picture being made (see [`Thumbnail::get_thumbnail`]), whose failure is the `Err`.
pub type PictureFuture = Pin<Box<dyn Future<Output = Result<Option<Picture>, LoadError>>>>;

/// An item that is content (a file or a generated part).
pub trait Asset: Item {
    /// The file extension, without the dot.
    fn content_type(&self) -> &str;
    /// The asset's category.
    fn category(&self) -> &str;
    /// The asset's file name.
    fn file_name(&self) -> String;
    /// Where the asset's content lives.
    fn asset_path(&self) -> &str;
}

/// An asset whose content is a stream of bytes (a file).
pub trait AssetStream: Asset {
    /// The content's bytes, `Ok(None)` when the item has no content to give (a missing file or
    /// one that cannot be loaded).
    fn get_stream(&self) -> Result<Option<Vec<u8>>, LoadError>;
    /// The content, as a future run on the caller's executor, for an item whose bytes may have
    /// to be downloaded first (a file in a cloud folder). Items whose content is at hand answer
    /// `get_stream`'s result, ready.
    fn get_stream_async(&self) -> StreamFuture {
        Box::pin(std::future::ready(self.get_stream()))
    }

    /// [`Self::get_stream_async`] for a person waiting on it (they opened, exported or copied
    /// the item): a download refused because their sign-in ended may ask them to sign in again.
    /// The default is `get_stream_async`; nothing else ever asks.
    fn get_stream_for_person(&self) -> StreamFuture {
        self.get_stream_async()
    }
}

/// An item's content being fetched (see [`AssetStream::get_stream_async`]).
pub type StreamFuture = Pin<Box<dyn Future<Output = Result<Option<Vec<u8>>, LoadError>>>>;

/// An item that opens a folder (a folder row).
pub trait FolderLink: Item {
    /// Makes the folder this link opens (not yet loaded).
    fn get_container(&self) -> Result<Box<dyn Folder>, String>;
    /// Whether the folder the link opens is read-only.
    fn is_read_only(&self) -> bool;
    /// Whether a read-only folder holds folders that can take items (a folder of folders shared
    /// with the user as an editor), so a save or move dialog lets the user open it to reach
    /// them, though it takes nothing itself. False (the default) for the rest.
    fn holds_destinations(&self) -> bool {
        false
    }
    /// What happens when the user opens a link whose `get_container` had nothing to give (a
    /// cloud folder launching sign-in while signed out). Runs that action and answers true, so
    /// the browser stays where it is and shows no error; false (the default) for a link with
    /// none. Only a user's open calls this, never a search's walk: a search the user typed must
    /// not open a dialog they did not ask for.
    fn open_unavailable(&self) -> bool {
        false
    }
    /// The small icon drawn over the folder tile (`computer.svg`), as a key the app's look
    /// resolves; `None` for a plain folder. The browser draws it (the model holds no images).
    fn micro_icon(&self) -> Option<&str> {
        None
    }
    /// The whole picture a link shows as its tile in place of the folder tile (a zip file's), as
    /// a key the app's look resolves; `None` (the default) for the folder tile.
    fn tile_icon(&self) -> Option<&str> {
        None
    }
    /// Whether the folder this link opens has visible folders of its own, for the folder tree's
    /// expand arrow: `Some` when that is known without slow or remote storage, `None` when only
    /// opening the folder tells. It is asked on the UI thread as rows are made, so answering
    /// must never reach the network, wake a drive, open an archive or prompt the user. The
    /// default is `None`: a link opts in only when it can answer that cheaply, so a new link
    /// backed by the network is safe without thinking about it.
    fn has_child_containers(&self) -> Option<bool> {
        None
    }
    /// Whether the folder this link opens is the user's Trash. Destination pickers (Save As,
    /// Move) leave it out: an item saved or moved into the Trash would be thrown away, and
    /// Remove is the way into it. Known from the listing that made the link, never from its name.
    fn is_trash(&self) -> bool {
        false
    }
    /// The storage the folder this link opens lives in ([`PlaceId`]); `None` (the default) for
    /// one that is in no place items can be moved or copied between.
    fn place(&self) -> Option<PlaceId> {
        None
    }
    /// Where the folder this link opens is (a disk path, a zip file and the folder inside it),
    /// the identity a folder tree keeps its row by; `None` (the default) when it has none.
    fn location_key(&self) -> Option<String> {
        None
    }
}
