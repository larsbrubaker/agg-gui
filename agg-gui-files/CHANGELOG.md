# Changelog

All notable changes to `agg-gui-files` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [Unreleased]

## [0.0.3] - 2026-11-02

### Added

- The `disk` feature: folders on the computer's disk, from MatterCAD's library. `disk::DiskFolder`
  lists a folder's sub-folders by name and the files its `DiskRules` accept by path, reads its
  `index.md` as the header, lists a message row for a folder it cannot read, and answers
  `place()` (the computer) and `location_key()` (`disk_location_key`, its path). It is a
  `WritableFolder`: `add` (content copied in under a free name, `CreateFolderItem` folders made),
  `remove` (to the system trash), `rename`, and `move_items` (renamed across between two folders
  on disk, copied in with the originals kept from anywhere else), with `move_from_disk`,
  `rename_on_disk` and `remove_to_trash` answering what they did (`MovedPath`, `TrashedItem`)
  and `copy_items_in`. `disk::DiskFolderLink` is a sub-folder's row, carrying the rules to the
  folder it opens; `disk::DiskFile` a file, with `set_name` and `subscribe_file_renamed`.
- `disk::DiskRules` (`lists_file`, `read_only`, default false) and `disk::AllFiles`. A read-only folder's `allow_action` refuses everything and each write
  fails before touching the disk.
- Live refresh: a showing `DiskFolder` (`activate` to `deactivate`/`dispose`) watches its folder
  through the registered `DirectoryMonitor` (`set_directory_monitor`, `directory_monitor`;
  `PollingDirectoryMonitor` polls each `POLL_INTERVAL` natively and watches nothing in the
  browser) and relists half a second after the last outside change, at `pump_live_refresh`
  (`directory_contents_changed`, `pending_refresh`, `RefreshInterval`). Its own writes run with
  the watch off.
- The tree arrow's quick look: `has_child_containers` on `DiskFolder` and `DiskFolderLink`
  (`folder_has_child_containers`; `DiskFolderLink::with_child_containers_unknown`; a network share
  answers `None`).
- Copies that keep the originals: `DiskFolder::copy_in_async` (awaiting content, copying a folder
  that is not on disk down whole) and `WritableFolder::copy_in` (that copy as a task; the folder
  it was started from relists at its next `pump_live_refresh`), `DiskFolder::add_async`, and the
  copy tree: `CopyNode`, `gather`, `gather_item`, `read_dir_node`, `write_node`,
  `sanitize_file_name`. A folder from another place that lists later is awaited through
  `folder_listing`.
- Recorded writes on `DiskFolder`: `move_in_recorded` (the moves from another folder on disk,
  the copies from anywhere else), `rename_recorded`, `create_folder_recorded` (only a new folder)
  and `remove_recorded` hand back `disk::DiskStep`, a `HistoryStep` that undoes and redoes on disk
  (moves back under the old name, renames back, removes the new folder while it is empty,
  restores from the trash; never overwriting, keeping only what came back). `copy_in` records its
  copies, which an undo sends to the trash.
- Disk search: `DiskFolder` is a `SearchProvider` walking every folder below it for the files its
  rules list (`file_name_contains_filter`, `SEARCH_BATCH_SIZE`, `MAX_SEARCH_DEPTH`).
- `disk::ComputerFolder`: Downloads, Desktop and Documents (from the app's `ComputerFolders`) and
  the drives (`drive_roots`, `ComputerFolders::with_this_computers_drives`); a search covers the
  user's folders only.
- Disk item IDs (`disk_item_id`, `disk_item_id_at`, `round_trip_utc`) are MatterCAD's, text for
  text, so a thumbnail cache keyed by them keeps working.
- The system trash: `move_to_trash`, `restore_from_trash`, `set_trash_directory` (tests),
  `move_to_system_trash`, the freedesktop trash (`move_to_freedesktop_trash`) and the Recycle Bin
  record reader (`parse_recycle_bin_record`, `newest_recycle_bin_match`).
- `folder_listing` (`FolderListing`): loads a folder and awaits its listing, waking on
  content-changed until its load is no longer pending; answers the folder or its
  `ListingFailure`.
- `ENGLISH_TEXTS` lists the disk provider's texts.

- The `zip` feature: a `.zip` file browsed as a read-only folder, from MatterCAD's library.
  `zip_folder::ZipFolder` lists one directory level of an archive (files in archive order,
  folders in the order first seen; a missing archive lists a missing-file row, an unreadable one
  an error row); `zip_folder::ZipFolderLink` is the row of an archive or a directory inside it
  (read-only, `tile_icon` `zip_folder.png`); `zip_folder::ZipEntry` one file, read out on demand,
  with MatterCAD's IDs. Both answer no place and `zip_location_key` (the archive, a NUL, the
  directory). `lists_as_zip_folder`: with `disk` too, a folder on disk lists its zip files as
  folders, the tree's quick look counts them, and a search does not report them as files.

### Changed

- With `zip` on, depends on `zip` 2.4 (Deflate only, pure Rust) and `indexmap` 2.
- With `disk` on, depends on `objc2-foundation` on macOS and `windows-sys` on Windows (the system
  trash); neither compiles C.

## [0.0.2] - 2026-11-01

### Added

- Providers any app can list, from MatterCAD's library: `MessageItem` (a row that only shows a
  message, with `loading` and `missing_file`), `FolderData` (the state a plain folder keeps),
  `DynamicFolder` (filled by a `FolderInitializer` when it loads), `DynamicFolderLink` (a row
  whose folder a `FolderCreator` or `OptionalFolderCreator` makes when opened, with `NO_FOLDER`;
  its `place()` makes that folder on each call), and `SearchResultsFolder` with `SearchSection`
  and `SearchOrigin` (a search across folders in sections; `with_content_files` takes the app's
  `ContentFileTest`; `MAX_FALLBACK_DEPTH`, `ASYNC_LOAD_TIMEOUT`, `live_load_wait_timer_threads`).
- `WakeTimer`: the deadline that bounds a wait, a thread natively and nothing in the browser.
- `MemoryFolder`, `MemoryFolderLink` and `MemoryFile`: a writable folder in memory for tests and
  demos, raising content-changed after each change.
- `Spawner::log_error` (default: logs nothing): a failure work already handled, for the app's
  log; a failed search section is logged through it.
- `ENGLISH_TEXTS` lists the providers' texts.

### Changed

- Depends on `web-time` (the search's deadline reads the clock, which std cannot in the browser).

## [0.0.1] - 2026-11-01

### Added

- The folder and item model of agg-gui's file browser, from MatterCAD's library:
  `Folder`, `FolderLink`, `Item`, `Asset`, `AssetStream`, `Thumbnail` (whose
  `get_thumbnail` yields a `Picture`), `ContentChangedEvent`, `ListingFailure`
  and `LoadError`.
- `WritableFolder` with `ContainerActions`, the default move
  (`add_then_remove`), `names_of_items_gone_from_source`, `CreateFolderItem`,
  and the recorded forms `move_in_recorded`, `rename_recorded`,
  `create_folder_recorded` and `remove_recorded` (defaults record nothing) and
  `copy_in` (default: no copy of its own).
- `HistoryStep`, `Recorded`, `StepOutcome` and `StepDone`: recorded changes
  that take themselves back now or later.
- `Item::as_asset`, and `place()` / `location_key()` on `Folder` and
  `FolderLink` with `PlaceId` (`PlaceId::COMPUTER`).
- Search (`SearchProvider`, `SearchCancel`, `SearchReport`, `SearchFuture`,
  `yield_now`, `name_matches` with the app's content-file test as a parameter)
  and sort (`SortBehavior`, `SortKey` with the stored flag numbers,
  `sort_items`, `culture_compare`, `system_time_ticks`).
- `Picture` (straight-alpha RGBA, top row first), `Spawner` and `LocalTask`.
- The translation hook: `tr`, `set_translator` and `ENGLISH_TEXTS`.
