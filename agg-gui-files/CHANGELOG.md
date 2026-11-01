# Changelog

All notable changes to `agg-gui-files` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [Unreleased]

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
