# Changelog

All notable changes to `agg-gui-file-browser` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [Unreleased]

## [0.0.2] - 2026-11-02

### Added

- `BrowserConfig`, one per browser view, and `BrowserConfigBuilder`: the spawner, look, hooks,
  error report and `BrowserOptions` (tree or list only, who hears a double click, tap
  activation, Show Folders, a folder filter, the opening picture size). `BrowserConfig::default()`
  works for an app with no hooks.
- The hooks, each with a default: `ItemPictures` (`NoPictures`; `PictureCx`, `PictureState`),
  `BrowserActions` (`NoActions`; `Builtins`, `BuiltinAction`, `BrowserAction`,
  `BrowserCommand`, `Activation`), `OutsideDrops` (`OutsideOver`), `PlaceRules`
  (`DefaultPlaces`; `CopyCx`, `CopyTakeover`), `BrowserMemory` (`NoMemory`; `ViewState`,
  `ViewMode`, `LocationFuture`) and `BrowserDialogs` (`NoDialogs`, which offers none;
  `Confirmation`, `NameRequest`, `DialogKind`).
- `Entry` and `BrowserRef`, the snapshots hooks are handed, so no hook runs while the browser's
  state is borrowed.
- The move-or-copy rule: `default_drop_copies`, `default_crosses_places`, `places_differ` and
  `copy_modifier_held` (Option on the Mac, Ctrl elsewhere).
- `LocalSpawner`, an `agg_gui_files::Spawner` for apps without an executor, run from agg-gui's
  UI-queue drain (`run_ready_local_tasks`, `unfinished_local_tasks`).

## [0.0.1] - 2026-11-01

### Added

- The browser's look, from MatterCAD's library: `BrowserLook` (style, icons with
  `icon`/`icon_name`, `folder_tile`, names), `DefaultLook` (agg-gui's theme, Font Awesome
  glyphs), `BrowserStyle` (with `from_visuals`), `BrowserIcon` (each with its Font Awesome 4.7
  glyph and a name) and `BrowserNames` (the long-standing widget names, with templates for
  breadcrumb links and rows).
- Fitting names to their room: `fit_name`, `fit_name_with`, `fit_keeping_extension`,
  `name_extension`, `fit_text`, `fill_text_within` and `centered_text_baseline`.
- The breadcrumb trail: `FolderBreadCrumbWidget` and `folder_bread_crumb_for`, with
  `BrowserIconButton` (its up button) and `LinkLabel` (its links).
- `ENGLISH_TEXTS`, the English text the crate hands to `agg_gui_files::tr`.
