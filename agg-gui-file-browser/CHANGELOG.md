# Changelog

All notable changes to `agg-gui-file-browser` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [Unreleased]

## [0.0.9] - 2026-11-03

### Added

- `PlaceRules::folder_inside`: whether one folder's key is inside another's, however the dialog
  reached it. The Move dialog refuses a destination the rules place on or below a moved folder
  ("can't go inside itself"), so an app whose roots reach folders inside one another (a library
  folder on disk inside a moved folder) answers by their locations. The default answers only
  equality, as before.
- `FolderKey::place` and `FolderKey::location`: a key's parts, for an app's `folder_inside`.

### Changed

- A Move dialog's accept that finds the showing folder still held elsewhere also says so through
  the app's report (`BrowserConfig::report`), not only in the browser's status, since an app's
  own page may show no status line.

## [0.0.8] - 2026-11-03

### Added

- `PlaceRules::folder_key` and `open_folder_key`: how the Move dialog tells a folder row and an
  open folder apart, defaulting to their place and location key. An app whose folders have no
  location key (remote folders told by server IDs) answers keys of its own (`FolderKey::new`),
  so "already in" and "can't go inside itself" hold for them. `MoveRequest` carries the rules
  (`places`, `with_places`); the browser's own Move hands it the config's.
- A remembered folder still being found shows its trail: `BrowserMemory::last_location_trail`
  names the remembered folders (root first), and while `last_location` finds them `FileBrowser`
  and `BrowserPage` show that trail as the breadcrumbs over the "Loading..." line, never the
  root's rows nor its header markdown. Crumb clicks (`BrowserState::go_to_crumb`) and Back (`go_back`) move along the
  trail, the user's own navigation ends the restore, and `BrowserState::retry_restore` runs it
  again (call it once the user signs in). `restoring_trail`, `is_restoring` and `shown_trail`
  read it. The default memory names no trail, so nothing changes for an app that adds none.
- `FileBrowser::remembering(key)`: restores the place remembered under `key` and remembers each
  folder the user moves to, as `BrowserPage::remembering` does.
- `FileBrowser::with_overlay_child`: an app's widget (a context menu host) kept across rebuilds
  at zero size at the browser's origin, over the rest; `with_painted_rect`: the browser's window
  rectangle from its last paint, for menu and drop hooks.
- `BrowserLook::list_banner` (a banner over the list, asked again whenever the list shows its
  content anew) and `BrowserLook::header_markdown` (the folder's header drawn with the app's
  markdown widget).
- `BrowserLook::view_mode_name`, the name `FileBrowser` reports as `view_mode` (default the
  mode's Rust name, as before), and a `restoring` property; `bread_crumb_trail` reads the trail
  being found while restoring.
- `BrowserLook::page_nav_bar_padding` and `page_nav_bar_rule`: the dialog page bar's padding
  and whether it draws its rule (defaults `Insets::symmetric(3, 1)` and a rule, as before).
- `BrowserState::add_restore_finished(listener)`: each restore's answer tells its listeners how
  it went, as a `RestoreOutcome` (`Landed`, `Failed` when the memory could not reach the
  location, `Abandoned` when the user went elsewhere meanwhile, `Superseded` when a newer restore
  began; `LandedShortened` when it opened down to the folder crumb the user clicked meanwhile), outside any borrow of the state (`RestoreFinishedListener = Rc<dyn Fn(RestoreOutcome)>`).
- `LIVE_REFRESH_WAKE` (100 ms): while the showing folder's live relist is pending, the list asks
  for one more frame that long after each layout, rather than a frame after every frame, so an
  idle browser waiting on a relist no longer draws continuously.
- `FileBrowser` stands its pieces `PIECE_GAP` (agg-gui's column gap, 4 units) apart: bar from
  bar, the bars from the body, the body from the message line, and the header, banner and list
  from each other, as MatterCAD's Library laid them out (they stood flush).
- `ProviderMessageBar` reports its line's colour as its `color` property. `FileBrowser`'s
  list-only form draws that line in the look's light text colour, as the bar is documented to
  (it took the plain text colour).
- A `FileBrowser` made `remembering` remembers where the user stayed when they ended its restore
  without moving (Home clicked while it was found), once the late answer comes.
- `BrowserState::remember_location` remembers nothing while search results show: they are a
  view of a query, not a place, so the last real place stays remembered.
- `ItemPictures::before_layout(state)`: the pictures hook's turn at the start of each list
  layout, the state lent unborrowed (default nothing).

### Fixed

- A `FileBrowser` made `remembering` no longer writes where a restore landed back to the memory
  (the nearest folder a memory could reach, with the remembered one on a drive not mounted, used
  to replace it), and a restore a retry superseded no longer writes the root when its late
  answer comes; the user's own moves, a folder crumb that shortened the restore, and Home
  clicked while a restore is found, still are.
- `FileBrowser` keeps the bar holding its search box when the header is built again (a folder
  opened, a search's results showing): the bar, its row and the search slot stay the same
  widgets and only what stands beside the box is new, so the box and its clear button keep
  their parents, and a pointer (or a GUI automation handle) on its way to them still finds them.
  `SearchSlot::new_or_empty`, `holds_search_slot` and `keep_search_bar` (in `browser::parts`)
  are how.

## [0.0.7] - 2026-11-03

### Added

- Look hooks for the navigation bars' pieces: `BrowserLook::menu_button` (the view options and
  sorting buttons, handed the rows their menus show as `NavMenuRows`), `overflow_button` (handed
  `OverflowRows`, the overflow menu's rows as the browser's own `OverflowButton` shows them),
  `search_field` (the search box's text field before the box gives it its text and handlers) and
  `clear_search_button`. An app draws them as its other bars' widgets while the browser keeps
  what they show and do; each defaults to the browser's own widget.
- `view_options_control`, `sorting_control` and `overflow_control`: the button a bar places,
  the look's own when it draws one. `FileBrowser` and `BrowserPage` place them.
- `overflow_rows`: the overflow menu's rows for a browser, asked for when needed.

### Fixed

- Undo and Redo stood flush together, 4 units narrower than the bar's other buttons placed side
  by side; they now stand the bar's gap apart (`HISTORY_GAP`), as MatterCAD's Library has them.

## [0.0.6] - 2026-11-03

### Changed

- Rows and tiles ask the pictures hook for their pictures only near the visible part of the
  list, so opening a folder of hundreds of pictures makes a screenful of them, and scrolling makes
  the rest as they come into view. `BrowserRow::new` and `TileStack::for_state` now build each row
  and tile with placeholder art (the look's folder or item icon), asking the hook nothing; after
  each layout `BrowserList` has the rows and tiles within `list::visible_art::READ_AHEAD` (120
  units) of its viewport ask, once. `refresh_art` (a picture arrived) skips rows and tiles that
  have not asked. A consumer that builds rows or tiles itself, outside a `BrowserList`, must call
  `ask_art` on them to get their pictures.

### Added

- `BrowserRow::ask_art` / `BrowserTile::ask_art` (ask for the picture once; later calls do
  nothing) and `BrowserRow::asked_art` / `BrowserTile::asked_art` (whether it has asked).
- `list::visible_art::{ask_visible_art, READ_AHEAD}`: the pass `BrowserList` runs after layout.

## [0.0.5] - 2026-11-02

### Added

- `BrowserOptions::list_takes_keys` (default `true`): `false` makes a list that never takes the
  keyboard (a press leaves the focus where it was) and takes none of its keys (Delete and
  Backspace for Remove, undo and redo, select all, the arrows, Escape), for a dialog whose name
  field keeps typing while a click on a row fills it in. A list whose app menu gives no rows opens
  no menu. `dialog_options()` turns it off, so the built-in Save, Move and Open dialogs' lists
  leave the keyboard to the dialog.
- `CopyTakeover::Refused`: the app refused a copy and told the user why; the drop answers that it
  copied nothing.
- `PlaceRules::take_over_move`, handed a `MoveCx` (the items, the target and source folders, the
  target's name): an app runs a drop's or the Move dialog's move its own way (a move between two
  folders of a remote place, whose undo step can only be recorded once the server answers) and
  answers `MoveTakeover::Taken` with how it went, which the browser reports as it reports its own
  moves; `MoveTakeover::Declined` hands the move back to the folders' recorded move. The default
  hands every move back, so nothing changes for an app that doesn't override it.
- `OutsideDrops::cancel_on_return` (default `true`): `false` keeps the app's view as a drag left
  it when the drag comes back into the browser (a browser panel lying over that view); the drag's
  end then tells the hook `cancel` after a drop on a folder, or hands it the `release` over no
  folder.
- `BrowserLook::message_button`: the app's own button for a folder message's Sign In (named as the
  message asks, enabled and pressed through what it is handed); the default is agg-gui's `Button`.
- The drag picture reports the art it shows as the list's `drag_picture_image` property (the
  carried row's `thumbnail`).
- `BrowserList::with_offset_cell`: the list scrolled by an offset its host keeps across the lists
  it builds again (a dialog page rebuilding its list on each change), which the host puts back to
  the top itself.

### Fixed

- A new display scale left the list's pictures at the old one (a window moved to a sharper
  screen); the list builds its rows again, asking for its pictures at the new scale.
- The folder message's Sign In button carries `BrowserNames::sign_in_button` itself, so what
  finds it by name reads whether it takes a press (it was a wrapper that always read enabled).
- A picture arriving built every row and tile of the list again, so the one under the pointer was
  a new widget mid-press; the rows and tiles now ask for their art again in place
  (`BrowserRow::refresh_art`, `BrowserTile::refresh_art`, `ListCx::art`).
- A move into a folder open above the source (the Move dialog's into the parent) left that folder
  listing what it held before, when the app's folders answer one ID for all: the open folder is
  now told by its location (`FolderKey`), else its ID, and the source is never the one found.
- A drag dropped on a folder tree row kept no keys: the list gave them up for any drop outside
  itself. Only a drag ending outside the browser (list and tree) gives them up now, so the undo
  keys take back a drop on the tree.
- In the Save dialog, Backspace or Delete after a click on a file asked to remove that file (the
  list had taken the keyboard); they now edit the name. The Save dialog of `SheetDialogs` gives
  its name field the keyboard as it opens (the request's focus id, or the sheet's own), so typing
  names the file at once.
- F5 in the Save dialog lists the folder again whatever has the keyboard: `SavePage` hears it as
  a key nothing focused took (`on_unconsumed_key`), which a showing `ModalSheet` offers its
  content from agg-gui 0.5.5 on (the floor this crate now needs).

## [0.0.4] - 2026-11-02

### Added

- `BrowserList`'s `picture_arrivals` property: how many times the pictures hook said a picture of
  its rows changed, each building the rows again.
- `SaveController`, `MoveController` and `OpenController`: the Save, Move and Open dialogs'
  logic without their widgets, so an app can wrap them in its own dialog page (its heading,
  field, buttons, footer and names); `SavePage`, `MovePage` and `OpenPage` are the default pages
  over them (`SavePage::over`, `MovePage::over`, `OpenPage::over` take a controller; `new` is
  unchanged). A page calls the controller's `update` at each layout, before its browser lays out
  (it opens the dialog's place, remembers the user's moves and takes the list's clicks), and
  reports what the controller exposes (`name`, `placeholder`, `can_save`, `refusal`,
  `accept_enabled`, `summary`, `accept_text`, `properties`). Each opens in a starting folder
  (`starting_in`, a `LocationFuture`) or where it was last left (`remembering(key)`).
- `SaveController::with_folder_rule` (`SaveFolderRule`): the app's own rule for which folders
  take a save (a cloud folder only while signed in), so Save is disabled up front, with the
  rule's reason as `refusal`; `with_question_host` (`QuestionHost`): the replace question and the
  app's own question asked in the app's modal message box (confirming saves, cancelling stays),
  the in-page question being the default (`SaveMode`); `asking_names_through`: Create Folder's
  name asked through `BrowserDialogs::ask_name` (the app's input box) rather than in the page.
- `SaveRequest::with_placeholder` and `with_focus_id`: the name field's grey hint while empty,
  and the focus id it is given focus through as the dialog opens.
- `BrowserNames::dialog_confirm`, `dialog_cancel`, `save_name_field`, `save_note`,
  `create_folder_button` and `folder_name_field`: the dialog pages' widget names, defaulting to
  the names they always had.
- `tree_menu_rows`, the rows a tree row's right-click menu shows, each running with the row's
  folder as the temporary selection, as `menu_actions` gives the list's.
- `BrowserMenus` (`BrowserConfig::menus`, builder `menus`): the list and the folder tree offer
  each right-click menu's rows to the app's own menu host, at the press in window coordinates
  with its `MenuSource` (`List(scope)` or `Tree`); `OwnMenus`, the default, declines and the
  browser opens its own menu.
- `BrowserLook::section_toggle` and `working_indicator`: a look may build a search section
  header's expand toggle and the search's working indicator as its app's own widgets; without
  them the browser draws its caret toggle and agg-gui's small spinner.
- `BrowserCommand::as_remove` and `is_remove`: an app's own command marked as the menus' Remove
  is what the Delete and Backspace keys run, in the list for the selection and in the tree for
  the folder drawn selected (the built-in Remove is one); a Remove row that can't run runs
  nothing.
- A row or tile reports its overlay's properties after its own
  (`OverlayRoute::properties`).
- `BrowserLook::folder_tile` is asked before a link's own tile image (`FolderLink::tile_icon`),
  so a look draws that image its own way; the tile reports `BrowserLook::folder_tile_name` as
  its `thumbnail`, by default the image's key or the folder icon's name.
- `ItemPictures::tile_overlay` is handed an `OverlayFrame` (how far inside the row's or tile's
  edge the picture square is: a tile's border), and a carried row or tile paints its overlay
  faded with it (the lift ends in `finish_paint`, after the children). Migration: add the
  `frame` argument to `tile_overlay` implementations.
- `PictureState::Shown { image, kind, name }`: the app's own image for an entry standing for its
  picture, its generating marker or its item icon (`PictureKind`), reported by the row as
  `picture` and `thumbnail`, for an app whose icons depend on the item or whose pictures carry a
  name of their own.
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
  selection in the folder above it, each command run with that selection set again. The rows as data are `TreeModel` (`TreeNode`,
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
- The browser's own menu actions run (`builtins`): Create Folder... asks "Folder Name" and makes
  the folder, Rename asks "Name" over the current name (from the tree, the folder renamed while
  it shows is shown again by its new name), Remove confirms ("Move to Trash?" or "Move to Recycle
  Bin?" for a folder on the computer, "Remove Items?" elsewhere) and removes, each recorded for
  Undo through `BrowserState::write_recorded`; Move asks the app's Move dialog
  (`BrowserDialogs::pick_move_target`, `MoveRequest`, `DialogKind::PickMoveTarget`) and moves as a
  drop onto the folder picked does; Add adds the files the app's picker gives; Open File Location
  shows where `BrowserActions::locate` says the item lives (`can_locate`, `ItemLocation`). Each
  runs only when it can: a writable folder that allows the change, one row for Rename and Open
  File Location, no protected rows, and the dialog it needs offered. `run_builtin` runs one.
- `BrowserCommand` carries an icon key the look resolves (`with_icon`, through
  `BrowserLook::icon_for_key`), a shortcut shown on its row (`with_shortcut`), a hover help
  (`with_tooltip`), its menus (`with_scope`, `ActionScope::{Selection, Folder}`), `only_when_enabled`
  (left out rather than greyed) and submenus (`BrowserCommand::submenu`); built-ins carry an icon
  key of their own (`BuiltinAction::icon_key`) and a scope (`BuiltinAction::scope`).
- `menu_actions` and `select_rows`: the rows a menu shows. The selection's menus (a row's, the
  tree's) show the rows of their scope that can run now; the list's own menu the rows of its
  scope, greyed when they can't run; the overflow menu (`MenuScope::Overflow`) every row; a
  separator only between two rows kept. `BrowserList::menu_entries` and
  `BrowserTree::menu_entries` give the open menu as built.
- `OverflowButton`, the navigation bar's "File" button (`BrowserIcon::Overflow`, named
  `BrowserNames::overflow_menu`), opening the open folder's whole ordered menu.
- `SheetDialogs` and `DialogSheet`: dialogs on agg-gui's `ModalSheet` for an app without its own,
  confirming a change and asking for a name (an empty name accepts nothing), and with
  `with_browse_root` the Move, Save and Open dialogs; they offer no picker of files to add.
- Delete and Backspace run Remove, with its question, in the list and in the tree (the tree's
  selected folder).
- `FileBrowser`, the whole browser composed over one `BrowserState`: with
  `BrowserLayout::TreeAndList` one bar (breadcrumbs, the search box 200 wide, Undo and Redo, the
  view, sorting and overflow buttons) over the tree, the splitter and the list; with
  `BrowserLayout::ListOnly` the breadcrumbs and overflow button, a "Folders Toolbar" (the search
  box stretching, the view and sorting buttons), the list and the folder's header text in a
  `ProviderMessageBar`; in both the undo notice near the bottom. The header is built again when
  the folder changes, the tree, list and notice kept and the search box moved into the new
  header (`SearchSlot`, `take_search_box`) with its text and focus. The drag's picture is drawn
  once by the browser, in the global overlay; the mouse's back button goes up a folder. Its
  properties give the path, trail, selection, counts, view, search and drag picture.
  `FileBrowser::with_search` takes an app's `SearchController`.
- `ListColumn`: the list under the showing folder's header markdown (agg-gui's `MarkdownView`,
  named `BrowserNames::header_markdown`, left out when the folder has none); both forms of
  `FileBrowser` show it.
- `BrowserList::without_drag_picture`, for a host that draws the drag's picture itself.
- `SheetDialogs::with_browse_config` (the Move dialog's browser in the app's look, spawner and
  memory) and `with_move_location_key` (the dialog opens where it was last left, remembered
  through `BrowserMemory` under the app's key). `BrowserPage::remembering(key)` and
  `BrowserState::restore_location(&state, key)` do it for any page; `MovePage::new` takes the key.
- `BrowserLook::unkeyed_folder_icon`: the icon of a tree row whose link names no icon key
  (default none); `DefaultLook` answers a folder.
- `DefaultLook` draws the "folder" icon key (`BrowserIcon::Folder`'s key, a link's
  `micro_icon`) as a folder in the tint asked for, so a tree's rows naming it look like folders.
- The Move dialog (`move_dialog`): `MovePage` ("Select a new Destination:", a line saying what
  accepting does, a `BrowserPage` of its own, Cancel and Move or Copy) over `move_verdict`, which
  moves within a place and copies between places or out of a folder that keeps its items, and
  refuses a read-only folder or the top, the items' own folder ("... is already in ..."), and a
  moved folder or one inside it ("... can't go inside itself."), folders told apart by
  `FolderKey` (place and location key). `SheetDialogs::with_browse_root` shows it; the browser's
  Move then moves or copies into the folder picked with `drop_into`, recorded for Undo.
  `BrowserPage` and `dialog_options` are a dialog's browser (nav bar named
  `BrowserNames::page_nav_bar`, 75-unit pictures, read-only folders hidden).
- `FileBrowser::with_compact`: the list-only form as a phone's narrow panel while the app's flag
  is set: `COMPACT_VIEW` tiles (70) with their names, whatever view the folder remembers, no view
  button, and one tap activates. `BrowserList::with_view` fixes a list's view.
- Feature `disk` (off by default): agg-gui-files' disk folders and `register_directory_monitor`,
  a polling directory monitor that wakes the UI on each report (`WakingDirectoryMonitor`), so a
  browser on a disk folder relists when the folder changes outside the app.
- `BrowserCommand::builtin(action)`: one of the browser's own actions placed in an app's list
  with the app's tooltip, icon key or shortcut (`with_tooltip`, ...); the browser still runs it
  and decides when it can run.
- `BrowserNames::folders_toolbar` ("Folders Toolbar"); `BottomBorder`, a bar over a one-pixel
  rule.
- The Save dialog (`save_dialog`): `SavePage` over a `BrowserPage` of its own, asked through
  `BrowserDialogs::pick_save_target` (`DialogKind::PickSaveTarget`) and answering a `SavePick`
  (the folder, handed over, the file name and whether it replaces a file). `SaveRequest` says how
  a typed name becomes the file name (`FileNamer`, `append_extension`) and how a file's name
  shows (`ShownName`, `without_extension`); `with_extension(".mcx")` sets both. A click on a file
  fills its name in, a double click saves over it, Create Folder makes and opens a folder, F5
  lists the folder again (natively), and a file already there asks "Confirm Save As" first; the
  app's own question (`with_replace_question`, a `ReplaceQuestionHook` handed a `SaveTarget`,
  answering a `ReplaceQuestion` or a `refusal` with only its going-back button) is asked instead.
  A folder something still holds saves nothing and says so. `SheetDialogs` shows it, opening
  where it was last left (`with_save_location_key`).
- The Open dialog (`open_dialog`), for an app with no system one: `OpenPage`, asked through
  `BrowserDialogs::pick_open_target` (`DialogKind::PickOpenTarget`) with an `OpenRequest` (title,
  several files or one, a file filter such as `extension_filter(&[".stl"])`) and answering an
  `OpenPick` (the files and their folder). Every folder lists, read-only ones too; Open opens the
  selected files, or a selected folder; a double click opens a file. `SheetDialogs` shows it,
  opening where it was last left (`with_open_location_key`).
- `BrowserOptions::item_filter` (`ItemFilter`: the item rows a browser lists, a search's too, a
  folder's message never hidden) and `BrowserOptions::multi_select` (on by default; off, a press
  with any keys selects only its row, the band never draws and select all does nothing).
- `BrowserState::relist`, loading the showing folder again and showing it anew, and
  `BrowserState::can_hand_over_active_container`, whether `take_active_container` would hand the
  folder over alone: the Save and Move dialogs ask it first, so a folder something still holds
  stays showing and the dialog says it was busy.
- `BrowserState::put_back_active_container`: an open folder `take_active_container` took goes
  back on the path; the Save and Move dialogs put the folder back when a hand-over can't
  complete, so they stay in it.
- The folder tree scrolls when its rows are taller than it: the wheel moves them, and the row of
  each folder the browser goes to is scrolled into sight (`TreeScroll`, kept in the model,
  `TreeModel::scroll_offset`).
- `DropManager::set_source_key`: the folder a drag came from (`FolderKey`), which takes no drop.

### Changed

- A selection's menu (a row's, the tree's) holds only the rows that can run now, as a
  right-click menu should; the built-ins in it run rather than showing greyed. `MenuScope` has
  `Overflow`. Migration: a test that read a greyed built-in in the selection's menu gives the
  browser dialogs (`SheetDialogs`) or reads the overflow menu.
- `PickedMoveTarget` answers with a `MovePick` (the folder picked, opened, and whether the items
  copy) and the browser runs the move into it; `MoveRequest` carries the source folder's name,
  `FolderKey` and copy rule (`with_source`). Migration: an app's Move dialog answers
  `Some(MovePick { target: link.get_container()?, copy })` where it answered the link.
- Requires `agg-gui-files` 0.0.4 (memory folders that record undo).
- A drag never drops on the folder its items are already in (the same place and location key):
  no highlight or label, and `drop_into` refuses such a target, so nothing is renamed.
- A drop holding a protected item copies it where the copy rule copies, and otherwise moves
  nothing and says "Protected items can't be moved"; such a folder isn't highlighted.
- A tree row takes a drop only inside the tree's rectangle.
- Save's "already there" ignores case (`SaveTarget::exists` agrees), so the replace question is
  asked for `Report.mcx` over `report.mcx`. A question asked in the page goes when the showing
  folder changes, and an answer arriving after a change saves nothing.
- Move from the tree's menu, like Remove, shows the folder above before the showing folder
  leaves. `tree_menu_rows` shows the row's folder first when it isn't the one showing, so it
  is right for any row.
- The history: a step recorded while an undo is finishing leaves the redo list empty when the
  undo lands (a redo landing goes below the new step); a step that drops its callback uncalled
  frees the history, the notice saying "Undo didn't finish: {0}" (or Redo).
- Translated lines are whole, with placeholders: "Create Folder...", "Undone: {0}",
  "Redone: {0}", "Couldn't put back: {0}", "Couldn't move to \"{0}\": {1}", "Couldn't copy to
  \"{0}\": {1}", "(Moved: {0})", "Couldn't find the location of \"{0}\"" and "Select a new
  Destination:" replace the pieces they were joined from (`ENGLISH_TEXTS` lists them).
  Migration: an app's translations add the new lines.

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
