//! `agg-gui-files`: the folder and item model behind agg-gui's file browser, with no drawing code.
//! It depends on std and `web-time` alone (no agg-gui, agg-rust or wgpu; the `disk` feature adds
//! the operating system's own trash calls on macOS and Windows), so a crate with no UI can use
//! it, and it builds for `wasm32-unknown-unknown` with or without `disk`.
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
//! - **Providers** are the folders and items any app can list: [`MessageItem`] (a row that only
//!   shows a message), [`DynamicFolder`] and [`DynamicFolderLink`] (a folder filled by a callback,
//!   a row whose folder is made when opened), [`FolderData`] (the state a plain folder keeps),
//!   [`SearchResultsFolder`] (a search across folders, in sections, its deadline a
//!   [`WakeTimer`]) and [`MemoryFolder`] (a writable folder in memory, for tests and demos).
//! - **Disk** (the `disk` feature): [`disk::DiskFolder`] (a folder on the computer's disk, which
//!   lists, takes changes and copies, records them as [`disk::DiskStep`]s, searches itself and
//!   relists after an outside change), its
//!   rows [`disk::DiskFolderLink`] and files [`disk::DiskFile`], following the app's
//!   [`disk::DiskRules`]; [`disk::ComputerFolder`] (the user's folders and the drives); the
//!   directory watch an app registers; and the system trash a remove sends items to. It compiles
//!   for wasm32, where every disk read fails and nothing is watched.
//! - **Stored values keep their shape.** The sort key numbers ([`SortKey`]) are the flags that
//!   stored view states hold.
//!
//! Modules: `item` (items, assets, streams, links and their futures), `folder` (folders, the
//! content-changed event, listing failures), `folder_listing` (a folder's listing awaited),
//! `writable` (writable folders and the default
//! move), `create_folder_item` (the item that asks a writable folder for a new folder),
//! `history` (recorded changes), `error` (why content could not be given), `search`,
//! `search_terms`, `sort`, `picture`, `spawner`, `place` and `translate`; the providers
//! `message_item`, `folder_data`, `dynamic_folder`, `dynamic_folder_link`, `search_results`
//! (with `search_walk` and `wake_timer`) and `memory_folder`; and `disk`.
//!
//! The model comes from mattercad-rust's `mattercad-model` (`src/library/`), the same author's,
//! published here under agg-gui's MIT licence. MatterCAD keeps its own names for these types by
//! re-exporting them (`LibraryItem` is [`Item`], `LibraryContainer` is [`Folder`]), the providers
//! included (`DynamicContainerLink` is [`DynamicFolderLink`], `LibrarySearchResultsContainer` is
//! [`SearchResultsFolder`]).

mod create_folder_item;
#[cfg(feature = "disk")]
pub mod disk;
mod dynamic_folder;
mod dynamic_folder_link;
mod error;
mod folder;
mod folder_data;
mod folder_listing;
mod history;
mod item;
mod memory_folder;
mod message_item;
mod picture;
mod place;
mod search;
mod search_results;
mod search_terms;
mod search_walk;
mod sort;
mod spawner;
mod translate;
mod wake_timer;
mod writable;

pub use create_folder_item::CreateFolderItem;
pub use dynamic_folder::{DynamicFolder, FolderInitializer};
pub use dynamic_folder_link::{DynamicFolderLink, FolderCreator, OptionalFolderCreator, NO_FOLDER};
pub use error::LoadError;
pub use folder::{ContentChangedEvent, ContentChangedListener, Folder, ListingFailure};
pub use folder_data::FolderData;
pub use folder_listing::{folder_listing, FolderListing};
pub use history::{HistoryStep, Recorded, StepDone, StepOutcome};
pub use item::{Asset, AssetStream, FolderLink, Item, PictureFuture, StreamFuture, Thumbnail};
pub use memory_folder::{MemoryFile, MemoryFolder, MemoryFolderLink};
pub use message_item::MessageItem;
pub use picture::Picture;
pub use place::PlaceId;
pub use search::{yield_now, SearchCancel, SearchFuture, SearchProvider, SearchReport};
pub use search_results::{ContentFileTest, SearchOrigin, SearchResultsFolder, SearchSection};
pub use search_terms::name_matches;
pub use search_walk::{live_load_wait_timer_threads, ASYNC_LOAD_TIMEOUT, MAX_FALLBACK_DEPTH};
pub use sort::{culture_compare, sort_items, system_time_ticks, SortBehavior, SortKey};
pub use spawner::{LocalTask, Spawner};
pub use translate::{set_translator, tr, ENGLISH_TEXTS};
pub use wake_timer::WakeTimer;
pub use writable::{
    add_then_remove, names_of_items_gone_from_source, ContainerActions, CopyDone, CopyOutcome,
    WritableFolder,
};
