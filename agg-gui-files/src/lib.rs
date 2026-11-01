//! `agg-gui-files`: the folder and item model behind agg-gui's file browser, with no drawing code.
//! It depends on std alone (no agg-gui, agg-rust or wgpu), so a crate with no UI can use it, and
//! it builds for `wasm32-unknown-unknown`.
//!
//! A [`Folder`] lists [`FolderLink`]s (rows that open another folder) and [`Item`]s. An item says
//! what kind it is by answering `as_container_link`, `as_asset` or `as_asset_stream`, and an app
//! reaches its own item types back through `as_any`. A folder's parent is not stored on the
//! folder: whoever navigates keeps the chain of folders it walked (a browser's breadcrumb path),
//! and that chain is what a parent is. A folder the user can change answers `as_writable`
//! ([`WritableFolder`]): add, remove, rename and move, each also in a recorded form whose
//! [`HistoryStep`] an undo stack keeps. [`SearchProvider`] is a folder searching itself,
//! [`name_matches`] what a query matches wherever it is asked, and [`sort_items`] the order a
//! [`SortBehavior`] gives.
//!
//! Rules every module follows:
//!
//! - **Single-threaded and local.** Everything is `Rc` and `!Send`. A folder's `load` is
//!   synchronous: it lists at once, or starts the work and raises its content-changed callbacks
//!   ([`ContentChangedEvent`]) when the listing lands. Streams, pictures and searches are local
//!   futures run on the caller's executor; a folder's own copy (`WritableFolder::copy_in`) is a
//!   [`LocalTask`] the caller hands to its [`Spawner`], where work nobody awaits goes; an undo or
//!   redo ([`HistoryStep`]) reports through a callback when it finishes. There is no async
//!   runtime here.
//! - **Places.** A folder or link may say which storage it lives in ([`PlaceId`]): items dropped
//!   between two places are copied, within one place moved. `location_key` names where a folder
//!   is (a disk path, a zip file and the folder inside it), the identity a folder tree keeps
//!   rows by.
//! - **Pictures** are [`Picture`]s: straight-alpha RGBA, top row first, as agg-gui draws them.
//! - **Text the user reads** goes through [`tr`], which an app points at its own translations
//!   with [`set_translator`]; every English literal handed to `tr` in this crate is listed in
//!   [`ENGLISH_TEXTS`] so the app can check its translations cover them.
//! - **Stored values keep their shape.** The sort key numbers ([`SortKey`]) are the flags that
//!   stored view states hold.
//!
//! Modules: `item` (items, assets, streams, links and their futures), `folder` (folders, the
//! content-changed event, listing failures), `writable` (writable folders and the default
//! move), `create_folder_item` (the item that asks a writable folder for a new folder),
//! `history` (recorded changes), `error` (why content could not be given), `search`,
//! `search_terms`, `sort`, `picture`, `spawner`, `place` and `translate`.
//!
//! The model comes from mattercad-rust's `mattercad-model` (`src/library/`), the same author's,
//! published here under agg-gui's MIT licence. MatterCAD keeps its own names for these types by
//! re-exporting them (`LibraryItem` is [`Item`], `LibraryContainer` is [`Folder`]).

mod create_folder_item;
mod error;
mod folder;
mod history;
mod item;
mod picture;
mod place;
mod search;
mod search_terms;
mod sort;
mod spawner;
mod translate;
mod writable;

pub use create_folder_item::CreateFolderItem;
pub use error::LoadError;
pub use folder::{ContentChangedEvent, ContentChangedListener, Folder, ListingFailure};
pub use history::{HistoryStep, Recorded, StepDone, StepOutcome};
pub use item::{Asset, AssetStream, FolderLink, Item, PictureFuture, StreamFuture, Thumbnail};
pub use picture::Picture;
pub use place::PlaceId;
pub use search::{yield_now, SearchCancel, SearchFuture, SearchProvider, SearchReport};
pub use search_terms::name_matches;
pub use sort::{culture_compare, sort_items, system_time_ticks, SortBehavior, SortKey};
pub use spawner::{LocalTask, Spawner};
pub use translate::{set_translator, tr, ENGLISH_TEXTS};
pub use writable::{
    add_then_remove, names_of_items_gone_from_source, ContainerActions, CopyDone, CopyOutcome,
    WritableFolder,
};
