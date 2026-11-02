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
- The app slot: `app_slot`, `app_slot_mut`, `set_app_slot`, `app_slot_or_insert_with` (the
  value there, or a new one of the type asked for), the app's own data per browser.
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
- `BrowserList`, the list views over a `BrowserState`: the showing folder's rows (`BrowserRow`,
  `RowStack`: a 50-unit picture, the name fitted beside it and an item's detail line under it) or
  tiles (`BrowserTile`, `TileStack`: 18, 70, 100 or 256 units, folders on rows of their own, names
  under tiles of 75 and up or under all with `with_tile_names`), built again whenever the state
  changes, a picture arrives or the look's art changes; clicks, Shift ranges, Ctrl/Cmd toggles,
  the rubber band and Control+A, Escape and the arrow keys; double clicks and taps through the
  app's `BrowserActions::activate`; a press that moves 40 units is a drag and never clicks
  (`ItemPress`, `DRAG_THRESHOLD`; what a drag carries comes with the browser's drag and drop); the
  selection's menu from a right press or a hovered row's menu button (`MenuButtonState`), the
  folder's from a right press on empty space or the list's top-right corner, each the app's
  `BrowserActions::menu` with the built-ins offered there (`MenuScope`; the built-ins show greyed
  until the browser runs them); a row the app selects scrolled into view; and a folder whose
  listing is only a message shows it in place of the rows (`FolderMessage`), under the look's
  warning for a refusal, with the app's Sign In (`BrowserActions::sign_in`, `SignInOffer`).
- `BrowserState::menu_button`, `menu_button_mut`, `item_press` and `item_press_mut`: the rows'
  menu button and press state, kept with the browser so they survive the rows being rebuilt under
  the pointer.
- A search's results list under their places' headers in both views (`SectionHeaderRow`: a
  "Searching for '{0}'" line while the search runs, then per place an expand toggle with the
  place's name and "(n)", "No results" or its error; a click on the header rolls the section up
  or opens it).
- The app's overlay on a picture (`ItemPictures::tile_overlay`) is reached through its row or
  tile (`OverlayRoute`): it hears the row's hover and every move over it, takes only the presses
  its `hit_test` says are its own (a press it takes is never the row's), and its tooltip is the
  row's over its own shape.
- A drag that ends outside the list gives the keys back (`ItemPress::set_list_rect`,
  `list_contains`).
- Rows and tiles report `name_right`, `thumbnail` (the art's name) and `thumbnail_center` (its
  centre pixel); `BrowserRow::thumbnail_pixel_size` and `name_right`; `BrowserTile::label_band`,
  `label_text_origin`, `label_printed_height` and `name_max_width`.
- `BuiltinAction::title` and `acts_on_selection`.
- `BrowserTree`, the folder tree over a `BrowserState`: the root's folders by name, an expand
  arrow on each folder that may have folders of its own (asked through
  `FolderLink::has_child_containers` where that is cheap, kept until first opened elsewhere, and
  dropped for the session once opening finds none), a "Loading..." row while an opened folder's
  listing is on its way, the folder's icon from `BrowserLook::icon_for_key` tinted in the accent,
  and its name fitted to the row with the whole name as the tooltip. A press on the arrow opens or
  folds the folder; a press elsewhere on a row shows the folder in the browser and opens it; the
  browser's folder is opened down to and drawn selected; after a write the tree lists itself again,
  and the showing folder's content changes list its row again; a right press shows the folder and
  opens the tree's menu, the app's `BrowserActions::menu` for the folder as the temporary
  selection in the folder above it (the selection's built-ins greyed until the browser runs them),
  each command run with that selection set again. The rows as data are `TreeModel` (`TreeNode`,
  `for_state`, `sync_to`, `toggle`, `relist`, `content_changed`, `expand_to`, `showing_row`,
  `selected_link`, `indented_names`); rows are known by their folders' location keys, else by
  name (`TreePlace`, `TreeKey`, `path_keys`, `PathLevel`, `BrowserState::path_levels`), so two
  folders of one name in different places stay two rows.
- `BrowserNames::tree` ("Library TreeView") and `tree_row` ("{name} Tree Node", `tree_row_for`).
- `BrowserState::snapshot_at`: the snapshot a hook is handed from a folder up the open path.
- `BrowserHistory`, the undo history (`BrowserConfig::history`, set with the builder's
  `history`; views given the same one share it): `record`, `record_with_note`,
  `record_saying`, `announce`, `keep` (a `Recorded` write's step), `can_undo`, `can_redo`,
  `undo_description`, `redo_description`, `depths`, `is_busy`, `generation` and `clear`;
  `undo`/`redo` (also `BrowserState::undo`/`redo`) run the latest step, which may finish later:
  until it does, no other undo or redo starts. When it finishes the step goes on the other list
  with what came back, the notice says "Undone: ..." or "Redone: ..." and "Couldn't put back:
  ..." for what failed, and the writable folders on the browser's path list themselves again.
  `HistoryRecorder` (`BrowserHistory::recorder`) records a change whose outcome arrives later
  (a server move) from a callback, on top of the history when it lands, and does nothing once
  the history is gone.
- `BrowserState::write_recorded`: runs a recorded write on a folder of the open path, records
  its step, and lists the browser again with any failure on its status line; a folder that takes
  no changes refuses with "This folder can't be changed".
- The list and the folder tree take Control+Z or Command+Z to undo and Control+Shift+Z,
  Command+Shift+Z, Control+Y or Command+Y to redo while they have the keyboard, even with
  nothing to undo (`history_key_event`).
- `BrowserNotice`, the notice announcing each change with its Undo or Redo button
  (`NoticeButton`), held five seconds and faded over one (`notice_alpha`, `NOTICE_HOLD_MS`,
  `NOTICE_FADE_MS`) unless hovered or its button has the keyboard; `Notice` and `NoticeAction`.
- Drag and drop (`drag`): a press in the list that moves past the threshold carries the
  selection; folder rows, tiles and tree rows record themselves as targets as they paint
  (`DropManager`, `register_painted`, `DropKey`, `DropTargetKind`), light up with the accent
  under the drag and label the drop ("Move to {0}", "Copy to {0}", the "+" badge;
  `DropLabel`, rows' and tiles' `drop_label`/`lifted`, tree rows' `drop_highlighted`/
  `drop_label`); read-only folders and the dragged folders themselves take nothing. The drop
  moves through `move_in_recorded` (its step on the history; a failure reported with what did
  move) or copies when the drag asks (the copy modifier, a source that keeps its items) or the
  folders are in two places: offered to `PlaceRules::take_over_copy` first, else through the
  target's `copy_in` on the spawner (its step recorded on landing) or `add`
  (`drop_onto`, `drop_into`). A drag held over a folder for `SPRING_DELAY` opens it, in the
  tree in place and in the list as the showing folder, where a drop on the empty space lands
  (`SpringOpen`). The drag's picture (`DragPicture`, 64 units, the count and copy badges)
  follows the pointer in the list's global overlay (`drag_picture` properties); outside the
  list and tree the app's `OutsideDrops` is asked (`over`, hiding the picture when it says so),
  handed a release there, and told `cancel` when the drag comes back or is lost to another
  window. Carried rows paint at half opacity.
- The list lights up as a folder does while a drop on its empty space would land in a folder the
  drag opened (`LIST_DROP_KEY`, `DropTargetKind::List`), and reports `dragging` and that drop's
  `drop_label`. `OutsideOver::NoTarget` carries `hides_picture` as `Target` does
  (`OutsideOver::hides_picture`), so an app may hide the drag's picture anywhere outside the
  browser; without a hook it follows the pointer everywhere. A row ignores agg-gui's hover-clear
  move and consumes its own press's moves, so each pointer move reaches the drag once.
- Navigation pieces (`nav`): the view options and sorting menu buttons (`view_options_button`,
  `sorting_button`, `NavMenuButton` with `NavMenuRow` check and radio rows, opened anew each
  time with the state's choices marked; a pick changes the state and the folder's remembered
  view), the Undo and Redo buttons over the history (`HistoryButtons`: greyed with nothing to
  do, "Undo - {step} ({key})" tooltips, the key as a hint, rebuilt on the history's generation;
  `history_key_text`), the tree|list splitter keeping the tree's width in `BrowserMemory`
  (`BrowserSplitter`, `SPLITTER_WIDTH`, `DEFAULT_TREE_WIDTH`, `MIN_TREE_WIDTH`) and the provider
  message bar (`ProviderMessageBar`). `BrowserNames::splitter` ("Library Splitter") and
  `provider_message` ("Provider Message").
- `BrowserIconButton::with_hint` (shown through the new `BrowserLook::show_hint` while hovered,
  cleared on leaving), `with_hover_fill`, `with_glyph_color`, `is_hovered`, and `tooltip` and
  `hint` properties.
- Search (`search`): `SearchController` (typing debounced by `DEBOUNCE`, 300 ms on the UI clock,
  through a queued layout the view's `poll` runs it in; Enter runs it at once; a new query
  disposes the one before; refining searches the folder the results came from; clearing returns
  to the folder searched with what was selected there; walking away from the results ends the
  search; `SearchMaker` lets an app make its own searches, `with_maker`) and `SearchBox` (a text
  field, "Search" while empty, with a clear button, `BrowserIcon::ClearSearch`, "Clear"; Escape
  clears too; it polls its controller as it lays out). `BrowserNames::search_panel` ("Library
  Search Panel"), `search_input` ("Library Search Input") and `close_search` ("Close Search").
- A drag's spring wait and the history notice's hold and fade count from the instant they
  began on the UI clock (`Notice::shown_at` is that instant), never as time since the clock's
  epoch, which a virtual clock started before it holds at zero. `web-time` is a dependency (the
  UI clock's `Instant`, as agg-gui's).
- `CopyCx` gains `source` (the open folder the items come from), `recorder` (for the copy's
  history step) and `report`.
- `BrowserNames::notice` ("Library Notice") and `notice_button` ("Library Notice Button");
  `BrowserStyle::notice_font_size` (10 pt).
- `OwnPictures`: each item shows the picture its folder or the item itself makes of it, made on
  the spawner and kept, the look's item icon until then; the default pictures hook. It keeps at
  most `KEPT_PICTURES` (1024) pictures, the least recently asked for leaving first and made again
  when asked for (`with_capacity`, `kept`), but never one the list's current build asked for: a
  folder with more items than that grows the kept pictures rather than making them again on
  every build. A picture that lands while the list builds shows at the next layout.
- `BrowserLook::tinted_icon` (an icon in a colour: a folder's accent, an item's text colour),
  `icon_for_key` (the art an item names by key: a folder link's `micro_icon` and `tile_icon`) and
  `icon_generation` (changes when the look's art does: the list builds again and
  `BrowserIconButton` asks for its image again).
- `BrowserStyle::tile_font_size` (the name under a tile, 9 pt) and `section_rule`, and the names
  `BrowserNames::list_area` ("LibraryView"), `folder_message`, `folder_message_text`,
  `sign_in_button`, `search_status` ("Search Status") and `search_section` ("Search Section
  {name}").

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
- `BrowserConfig::default()` shows each item's own picture (`OwnPictures`) where it showed the
  look's icon (`NoPictures`, still there for an app that wants icons only).
- `BrowserStyle` and `BrowserNames` have new fields. Migration: a look building either with a
  struct literal adds them or ends with `..Default::default()`.

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
