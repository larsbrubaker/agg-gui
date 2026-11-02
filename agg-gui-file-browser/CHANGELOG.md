# Changelog

All notable changes to `agg-gui-file-browser` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [Unreleased]

## [0.0.3] - 2026-11-02

### Added

- `BrowserState`, what a browser is looking at and does, made with `BrowserState::new(root,
  config)` or `with_config(root, config, app_data)`: the open path (`OpenFolder`s, shared, so a
  hook can be handed them with the state not borrowed); the showing folder's rows
  (`BrowserEntry`: folders first, then items, each sorted, Show Folders and the options' folder
  filter applied); the view mode and sort, restored from `BrowserMemory::view` whenever a folder
  shows and remembered through `set_view` when picked; navigation (`open`, `go_to`,
  `push_container`, `show_location`, `take_active_container`, `replace_active_container`) with
  container-changed listeners; only the showing folder activated and listened to, its content
  changes listing it again (`sync_content`, `pump_live_refresh`); the selection and its
  gestures (`click`, `press`/`release_press`, Shift ranges, Ctrl/Cmd toggles, the rubber band,
  select all, arrow moves, `select_when_listed`); tap activation; double clicks to the app's
  `BrowserActions::activate` or, with `DoubleClickTarget::Host`, as `ItemEvent`s for the host.
- The app slot: `app_slot`, `app_slot_mut`, `set_app_slot`, the app's own data per browser.
- `BrowserRef::live` and `BrowserRef::with_app_slot`: a hook reaches the browser it was handed a
  snapshot of, borrowing it only for the call.
- Search results on the path: `push_search_results` shows a started `SearchResultsFolder` below
  the folder searched; `list_slots` gives the list's lines (`ListSlot`: the searching line,
  `SectionHeader`s, rows) from `search_listing`; the same results shown again keep their
  selection and place, and a rolled-up section (`toggle_search_section`,
  `SearchSectionCollapseState`, shared for the session) carries its selected results until it
  opens; a result's picture is read from the folder it was found in.
- `item_identity_key`, the identity a selection is matched back by.
- A folder listing only a message: `ListingMessage`, `entries_message`, `is_only_a_message` and
  `BrowserState::listing_message` (its text, whether it is "Loading...", whether signing in
  lists it); such a folder lists no rows to select.
- Until a list view marks its listings (`mark_listed`), the state keeps its own listing current
  (at its making, each navigation and each redisplay), so `listed_entry`, `is_selected` and the
  clicks work for an app or a test with no view yet.

### Changed

- `BrowserConfig::report` is an `Option`: `None` (the default) shows errors on the browser's own
  status line (`BrowserState::status`), where the user sees them, rather than on standard error.
  The default spawner still reports its faults to standard error when no report is given.
  Migration: read `config.report` as an `Option`; `BrowserState::report()` gives the report in
  use.
- `BrowserMemory::remember_location` and `remember_open_location` take the open path as
  `&[OpenFolder]`; `BrowserState::remember_location(&state, key)` hands it over with the state
  not borrowed. Migration: borrow each folder (`folder.borrow().name()`).
- The public ways to activate a row (`BrowserState::activate`, `click`, `press`, `finish_tap`)
  take the shared state and run the hooks once its borrow has ended, so no activation waits.

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
  `ViewMode`, `LocationFuture`; `remember_location` is lent the whole open path, the root
  first, through `remember_open_location`) and `BrowserDialogs` (`NoDialogs`, which offers none;
  `Confirmation`, `NameRequest`, `DialogKind`).
- `Entry` and `BrowserRef`, the snapshots hooks are handed, so no hook runs while the browser's
  state is borrowed.
- The move-or-copy rule: `default_drop_copies`, `default_crosses_places`, `places_differ` and
  `copy_modifier_held` (Option on the Mac, Ctrl elsewhere).
- The opt-in trace: `trace` sends a line, built only while on, to the tracer the app installs
  with `set_tracer` (`tracing` says whether one is on).
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
