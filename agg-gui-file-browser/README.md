# agg-gui-file-browser

The widgets of agg-gui's file browser, over the folder and item model in
[`agg-gui-files`](../agg-gui-files). A crate of its own, so agg-gui apps that show no file browser
don't compile it. It builds for `wasm32-unknown-unknown`.

| Item | What it is |
|---|---|
| `FileBrowser` | The whole browser over one `BrowserState`: the tree-and-list form (one bar with breadcrumbs, search, Undo and Redo, view, sort and "File", over the tree, splitter and list) or the list-only form of a popup (with its "Folders Toolbar"), the undo notice, and the drag's picture drawn once for the whole browser |
| `BrowserLook`, `DefaultLook` | A browser's look: its style, icons and widget names; the default is agg-gui's theme with Font Awesome glyphs |
| `BrowserStyle` | The colours and sizes a browser draws with (`from_visuals` for agg-gui's theme) |
| `BrowserIcon` | The icons a browser draws, each with a Font Awesome fallback glyph |
| `BrowserNames` | The names a browser's widgets carry, which automation scripts and help articles find them by |
| `fit_name`, `fit_text`, `fill_text_within`, `centered_text_baseline` | Names fitted to their room (a file keeps its extension: "Bad+Batch+Hun….zip") and drawn inside it |
| `FolderBreadCrumbWidget`, `folder_bread_crumb_for` | The breadcrumb trail: up button, a link per folder, eliding the crumbs after the root when it is too wide |
| `BrowserIconButton`, `LinkLabel` | The trail's square icon button and its text links |
| `BrowserConfig`, `BrowserOptions` | Everything one browser view takes from its app, every hook defaulted (`BrowserConfig::default()` works with none; without the app's report, errors show on the browser's status line) |
| `ItemPictures`, `BrowserActions`, `OutsideDrops`, `PlaceRules`, `BrowserMemory`, `BrowserDialogs` | The hooks an app extends the browser through: pictures and tile overlays, the ordered menu and activation, drags onto its own views, move or copy, what is remembered, its dialogs |
| `OwnPictures`, `NoPictures`, `NoActions`, `DefaultPlaces`, `NoMemory`, `NoDialogs` | The defaults: each item's own picture (`NoPictures`: the look's icons only), the built-in menu, the place rule, nothing remembered, no dialogs (the actions needing one unavailable) |
| `BrowserList` | The list views: rows or tiles of the showing folder (`BrowserRow`, `BrowserTile`), or its message in their place (`FolderMessage`, with the app's Sign In), a search's results under their places' headers (`SectionHeaderRow`), the app's overlay on a picture reached through its row (`OverlayRoute`), with clicks, Shift ranges, Ctrl/Cmd toggles, the rubber band, double clicks through the app's hook, the arrow keys, and the selection's and folder's menus from a row's menu button or a right press |
| `BrowserTree`, `TreeModel` | The folder tree: the root's folders with expand arrows (none on a folder known to hold no folders), the browser's folder opened down to and drawn selected, rows known by their folders' location keys, and the folder's menu from a right press |
| `SearchBox`, `SearchController` | Searching from the browser: typing (after a short pause) lists the results below the folder searched, in sections as they arrive; clearing returns to the folder |
| `view_options_button`, `sorting_button`, `HistoryButtons`, `BrowserSplitter`, `ProviderMessageBar` | Navigation: the view and sorting menus, Undo and Redo over the history, the tree\|list splitter that remembers the tree's width, and a list-only browser's provider message |
| `DropManager`, `drop_onto`, `SpringOpen` | Drag and drop onto the browser's folders (list rows, tiles, tree rows): highlight and label, move or copy with its undo step, copies offered to the app first, folders opening under a held drag, the drag's picture, and drops onto the app's own views (`OutsideDrops`) |
| `BrowserHistory`, `HistoryRecorder`, `BrowserNotice` | The undo history shared by an app's browser views: steps recorded at once or when a server answers, undo and redo (from the keys or the notice's button) that may finish later, and the notice announcing each change |
| `ListingMessage`, `BrowserState::listing_message` | A folder listing only a message ("Loading...", a refusal, a sign-in to renew), shown in place of its rows |
| `BrowserState`, `BrowserEntry`, `OpenFolder` | What a browser is looking at: the open path (shared folders), the sorted listing, view and sort remembered per folder, the selection with its clicks, Shift ranges, Ctrl/Cmd toggles and rubber band, search results in sections, activation through the app's hooks, the status line and the app's own data (`app_slot`) |
| `Entry`, `BrowserRef` | The snapshots hooks are handed, so no hook runs while the browser's state is borrowed; a `BrowserRef` also reaches back to the live browser and its app slot |
| `default_drop_copies`, `copy_modifier_held` | Move or copy: items move within a place, and are copied between places, out of folders that keep them, or with Option (Mac) or Ctrl held |
| `LocalSpawner` | Background work for apps without an executor, run a step at a time from agg-gui's UI-queue drain |
| `BrowserCommand`, `ActionScope`, `BuiltinAction`, `run_builtin` | Menu rows: an app's commands with icons (through the look), shortcuts, scopes, submenus and rows left out while they can't run, and the browser's own (placed bare, or dressed with the app's tooltip through `BrowserCommand::builtin`) Add, Create Folder..., Open File Location, Rename, Move and Remove, which run, recorded for Undo, asking through the app's dialogs |
| `SheetDialogs`, `DialogSheet` | Ready dialogs on agg-gui's `ModalSheet` for an app without its own: confirming a change and asking for a name |
| `OverflowButton` | The navigation bar's "File" button with the folder's whole menu |

```rust
use std::rc::Rc;
use agg_gui_file_browser::{folder_bread_crumb_for, BrowserLook, DefaultLook};

fn trail(font: &std::sync::Arc<agg_gui::text::Font>) -> impl agg_gui::Widget {
    let look: Rc<dyn BrowserLook> = Rc::new(DefaultLook::default());
    let names = vec!["Home".to_string(), "Downloads".to_string()];
    folder_bread_crumb_for(&names, Rc::new(|depth| println!("open level {depth}")), &look, font)
}
```

```rust
use std::rc::Rc;
use agg_gui_file_browser::{BrowserConfig, BrowserLayout, BrowserOptions};

fn popup_config() -> BrowserConfig {
    BrowserConfig::builder()
        .report(Rc::new(|message| eprintln!("{message}")))
        .options(BrowserOptions { layout: BrowserLayout::ListOnly, ..BrowserOptions::default() })
        .build()
}
```

Text the user reads goes through `agg_gui_files::tr`; `ENGLISH_TEXTS` lists every English literal
this crate shows, so an app can check its translations cover them.

The widgets come from MatterCAD's library (mattercad-rust's `mattercad-app`, same author). MIT
licensed, as the rest of agg-gui.
