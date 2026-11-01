# Changelog

All notable changes to `agg-gui-files` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [Unreleased]

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
