# agg-gui-files

The folder and item model behind agg-gui's file browser, with no drawing code: no agg-gui,
agg-rust or wgpu dependency, so a crate with no UI (a server, a website build, an app's model)
can depend on it. It builds for `wasm32-unknown-unknown`.

| Type | What it is |
|---|---|
| `Folder` | A folder: lists `FolderLink`s and `Item`s, loads, raises content-changed, searches, says its place |
| `Item`, `Asset`, `AssetStream`, `Thumbnail` | What a folder lists: content, content with bytes, an item with its own picture |
| `FolderLink` | A row that opens another folder |
| `WritableFolder` | A folder the user can change: add, remove, rename, move, each also recorded for undo |
| `HistoryStep`, `Recorded`, `StepOutcome` | A recorded change that takes itself back, now or once a server answers |
| `SearchProvider`, `SearchCancel`, `name_matches` | A folder searching itself, stopping a search, what a query matches |
| `SortBehavior`, `SortKey`, `sort_items` | How a listing is ordered |
| `Picture` | Straight-alpha RGBA pixels, top row first |
| `Spawner`, `PlaceId`, `LoadError` | Where work nobody awaits runs, the storage a folder is in, why content could not be given |
| `tr`, `set_translator`, `ENGLISH_TEXTS` | The translation hook for the text the crate shows |

Everything is single-threaded (`Rc`, local futures, no async runtime): a folder's `load` lists at
once or starts the work and raises its content-changed callbacks when the listing lands.

```rust
use std::rc::Rc;
use agg_gui_files::{Folder, FolderLink, Item};

struct Notes;

impl Item for Notes {
    fn id(&self) -> &str { "notes" }
    fn name(&self) -> &str { "Notes" }
    fn is_protected(&self) -> bool { false }
    fn is_visible(&self) -> bool { true }
}

struct Home;

impl Folder for Home {
    fn id(&self) -> &str { "home" }
    fn name(&self) -> &str { "Home" }
    fn is_protected(&self) -> bool { true }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> { Vec::new() }
    fn items(&self) -> Vec<Rc<dyn Item>> { vec![Rc::new(Notes)] }
    fn load(&mut self) {}
}
```

It depends on std alone.

The model comes from MatterCAD's library (mattercad-rust's `mattercad-model`, same author). MIT
licensed, as the rest of agg-gui.
