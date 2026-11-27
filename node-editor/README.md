# agg-gui-node-editor

A reusable, model-agnostic node-graph editor widget for
[agg-gui](https://crates.io/crates/agg-gui).

`NodeEditor` is an agg-gui `Widget` that draws and edits a graph it never owns. The host implements
`NodeGraphModel` over its own graph store; the editor reads owned snapshots from it (`nodes()`,
`noodles()`) as it lays out, paints and handles events, writes every change back through it, and
never keeps the model locked past one event or paint.

| The editor does | The host decides |
|---|---|
| Panning (middle drag, or Ctrl/Cmd and a left drag on empty canvas) and zooming (the wheel, about the pointer, or Ctrl/Cmd and Alt and a left drag on empty canvas) | What nodes and noodles exist, and where they are stored |
| Dragging cards (with snap guides when one card is dragged), selecting with clicks, Shift and a rubber band, Delete or Backspace for the selection, Escape to cancel a drag | Whether a node can be added (`add_node`) and how a removal is recorded (`remove_nodes`, one undo step for a multi-node delete) |
| Dragging a noodle from an output, picking one up off a connected input, dropping it on a socket | Whether a connection is made, replaces one or is refused (`try_add_noodle` returns a `NoodleResult`) |
| Editing property rows on the card: `Number` by dragging, `Bool` by toggling, `Color` in a popup picker when the row's `editor` is `EditorHint::Color`, `Text` in a single-line editor | What a property is and what a new value does (`set_property`); anything else is `PropertyValue::Other`, shown but not edited |
| The right-click menu: Add Node on empty canvas, a node's menu on a node | The node types in that menu (`node_types_by_category`) and a node's own menu (`node_context_menu`) |
| Colours from agg-gui's theme each paint, error and warning badges on a card's title bar | Socket and category colours, socket compatibility, and the error or warning text on a `NodeView` |

`NodeEditor::with_body_factory` turns on hosted cards: the host builds each node's body as real
agg-gui widgets, and the editor keeps the card's chrome, sockets, noodles and gestures around it.
`arrange()` lays a graph out in tidy columns, and `NodeEditor::arrange_plan` plans that layout for
the live graph and returns the moves for the host to apply. `NodeEditor::set_palette` replaces the theme's colours with a `CanvasPalette` of the host's.

## Usage

```toml
[dependencies]
agg-gui-node-editor = "0.0.8"
```

```rust,ignore
use std::sync::{Arc, Mutex};
use agg_gui_node_editor::{NodeEditor, NodeGraphModel, SharedModel};

struct MyGraph { /* the host's own graph */ }

impl NodeGraphModel for MyGraph {
    // nodes, noodles, node_types_by_category, set_node_position, add_node,
    // remove_node, try_add_noodle, remove_noodle and set_property; every other
    // method has a default.
    /* ... */
}

let model: SharedModel = Arc::new(Mutex::new(MyGraph { /* ... */ }));
let editor = NodeEditor::new(model); // a Widget: put it in the tree like any other
```

`SharedModel` is `Arc<Mutex<dyn NodeGraphModel>>`, deliberately not `Send`: agg-gui's event loop
is single-threaded, so a host that evaluates its graph on another thread keeps that data behind
its own lock inside the model. Two editors in one tree need distinct ids (`NodeEditor::with_id`).
The editor calls the trait's methods with the model locked, so a method must not lock the same
`Arc` again: the mutex is not reentrant and it would deadlock or panic.

## Features

`reflect` (on by default) forwards to agg-gui's `reflect` feature. The editor has no reflection
code of its own and depends on agg-gui with its default features off, so a consumer that sets
`default-features = false` (a web build keeping its wasm small, say) leaves `bevy_reflect` out,
and one that keeps the defaults gets a single, consistently-featured agg-gui.

## License

MIT
