# Changelog

All notable changes to `agg-gui-node-editor` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [Unreleased]

### Added

- The add menu opens at the pointer on Shift+A as well as on a right-click
  on empty canvas, from the first source that answers:
  `NodeEditor::with_add_menu(|AddMenuRequest| -> Option<Box<dyn Widget>>)`,
  a host widget (a search box over a list) placed top-left at the pointer
  and kept inside the editor, handed to the `overlay_sink` or shown over the
  canvas (a press outside it, Escape or its `close` flag closes it);
  `NodeGraphModel::add_menu(canvas_pos) -> Option<Vec<MenuEntry>>`, entries
  shown in the editor's popup with each pick handed to
  `on_add_menu_action(action, canvas_pos) -> Option<NodeEditorCommand>`;
  or the built-in category submenus. `open_add_menu(local)`,
  `is_add_menu_open()` and `close_add_menu()` drive it from outside. An
  empty add menu no longer opens an empty popup.
- `NodeEditor::with_right_drag_pan(true)`: a right-drag on empty canvas
  pans, and a right-click released within 3 px of its press opens the add
  menu at the release (NodeDesigner). Off by default, keeping the menu on
  the press.
- `NodeGraphModel::on_escape() -> bool` (default `false`): Escape with no
  editor menu open goes to the host, and is consumed when it returns `true`;
  an open add or context menu closes first.
- `NodeEditor::screen_to_canvas` / `canvas_to_screen` (editor-local ↔
  canvas), `app_origin()` and `app_to_canvas` (app-absolute → canvas, for a
  drag from another panel).
- `NodeEditor::set_drop_feedback(bool)` / `drop_feedback()`: a 2 px
  theme-accent ring round the editor while an external drag will drop.
- `ViewAnchor` and `NodeEditor::with_view_anchor`: `ViewAnchor::Center`
  measures `pan()`, `set_view` and `on_canvas_pan_changed` from the
  editor's centre, as NodeDesigner does, and a resize keeps the centre
  fixed; the default `ViewAnchor::Origin` is the old bottom-left.
- `NodeEditor::center_nodes_in_view() -> bool`, NodeDesigner's
  `CenterNodesInView` (fit at most at 100 %, instant; `false` for an empty
  graph or a panel too short), and `request_center_on_draw()` /
  `cancel_center_on_draw()` / `is_center_pending()`: centre on the next
  layout that can, retried while the panel is too short, cancelled when the
  user pans, zooms or wheels (`cancel_view_animation` drops it too).

- `NodeEditor` implements `Widget::as_any` / `as_any_mut`, so a host holding
  it as a `Box<dyn Widget>` child can downcast it and call
  `open_context_menu()`, `select_node()` and the rest.
- `NodeGraphModel::socket_visible(node, side, socket) -> bool` (default
  `true`): a hidden socket is neither drawn (simplified card dot, hosted card)
  nor hit-tested (hover, press, drop snap), while a noodle wired to it still
  draws to its laid-out place. `SocketLayout` gains the matching
  `hidden: bool`.
- `NodeEditor::socket_position(node, side, socket) -> Option<Point>`: a
  socket's laid-out centre in editor-local coordinates at the current pan
  and zoom (hidden sockets included).
- Connection semantics for noodle drags, all defaulted so today's behaviour
  stays (new module `connection`, `SocketRef` re-exported):
  `NodeGraphModel::can_connect(from, to) -> Result<(), String>` is asked on
  every pointer move of a drag and at the drop (`from` is the end that stays
  put, so a drag backwards from an empty input has it on the input side);
  `Ok` rings and snaps, `Err(reason)` refuses, and a non-empty reason is
  shown beside the pointer in a note kept inside the editor
  (`connection::refusal_note_position`, MatterCAD's `RefusalPosition`;
  `NodeEditor::noodle_refusal()` reads it). The default
  (`connection::default_can_connect`) is the old rule: an output to a
  `sockets_compatible` input on another node, refused silently.
  `auto_pick_socket(node, from, candidates) -> Option<String>` picks the
  socket for a drop on a card body among the card's accepting sockets
  (default `None`, nothing connects; `connection::node_designer_auto_pick`
  is NodeDesigner's same type → any type → free input order).
  `NodeEditor::with_deferred_noodle_pickup(true)` keeps a noodle picked up
  off its input in the model, undrawn, until the drop, then calls the new
  `NodeGraphModel::move_noodle(picked_up, to)` once (one undo step): to the
  socket or auto-picked card dropped on, or `None` on empty canvas, which
  deletes it; a drop back in place or where nothing accepts it changes
  nothing. Off by default (the press removes the noodle at once). A press
  on a multi-input picks up the noodle landing nearest the press.
- Hosts can supply a node's right-click menu:
  `NodeGraphModel::node_context_menu(node) -> Option<Vec<MenuEntry>>`
  (default `None`, the built-in "Delete" + Add Node menu; an empty list opens
  no menu) and `on_node_context_action(node, action) -> Option<NodeEditorCommand>`,
  called with the chosen item's `action` string (which hosts use as the
  item's automation name, e.g. MatterCAD's `"Delete Node Menu Item"`); the
  returned command is applied after the model lock is released.
  `NodeEditor::open_context_menu()` exposes the open `PopupMenu` (editor-local
  coordinates) so automation can find a row.
- `NodeGraphModel::can_delete_from_keyboard()` (default `true`): returning
  `false` leaves Delete / Backspace unconsumed, so the host can send the key
  elsewhere.
- `NodeEditor::select_node(id, reveal)` and
  `NodeEditorCommand::SelectNode { id, reveal }`: make one node the selection
  and primary selection (MatterCAD's `NodeEditor.SelectNode`), optionally
  panning so its card is centred when it is not wholly on screen.

- `NodeEditor::with_raise_on_click(false)` keeps hosted cards in model order,
  as MatterCAD's NodeDesigner keeps its node windows: a press on a card
  selects it without raising it, and where two cards overlap the one later in
  model order stays on top and takes the press. On by default (a pressed card
  is raised, as before).
- Socket and noodle presentation, all opt-in (the existing look stays the
  default). New `NodeGraphModel` methods, each with a default:
  `socket_shape(node, side, socket, ty) -> SocketShape` (`Circle`, `Bar`,
  `Diamond`, `DiamondDot`), `socket_multi_input(node, socket)` (two or more
  noodles stretch the socket into a pill and land 10 units apart in
  `noodles()` order, the first highest), `noodle_dashed(&NoodleView)`
  (8 on, 8 off), `noodle_color(&NoodleView) -> Option<Color>` (overrides the
  source socket's colour) and `socket_hover_text(node, side, socket)`.
  `NodeEditor::with_noodle_style(NoodleStyle::NodeDesigner)` draws
  NodeDesigner's noodles (a 3-unit core over a 7-unit `#444` edge with a
  5-unit dot at the middle) and sockets (radius 6 with a 1-unit `#444`
  outline round every shape) and rings the drop target in the theme's text
  colour; `with_socket_hover(true)` rings the socket under the pointer and
  names it with `socket_hover_text` (`hovered_socket()` reports it). The
  drawing helpers are public in `socket_style` (`draw_socket`,
  `draw_socket_ring`, `draw_noodle`, `multi_input_stretch`,
  `landing_offset`), and `SocketLayout` carries `shape` and `landed`.
  Shapes reach the widget-tree cards, hosted cards and `draw_node`.
- A press on a widget inside a hosted card body (a slider, a text field)
  selects and raises the card, as a press anywhere in a MatterCAD node card
  does, while the widget still gets the press. The editor does this in its
  `Widget::preview_event`; Shift adds to the selection, a press on an already
  selected card keeps the selection, and sockets and the resize band are left
  to the editor's own handlers.
- Hosted cards: a host can supply each node card's body as real agg-gui
  widgets. `NodeEditor::with_body_factory(factory)` takes a `NodeBodyFactory`
  (any `FnMut(&NodeView) -> Option<HostedNodeBody>`, called with no model lock
  held); a `HostedNodeBody` is the body widget plus an optional
  `with_socket_anchor(|body, socket| Some((SocketSide, distance_from_body_top)))`
  resolver (sockets without an anchor stack under the title bar, outputs
  first). The editor keeps noodles, sockets, gestures, selection and menus and
  draws the card chrome and sockets; the body sits under the title bar in a
  canvas layer whose `child_transform` is the pan/zoom, so it paints, takes
  clicks and focus at the right place at any zoom. Bodies are cached per node
  and rebuilt only when `NodeGraphModel::body_epoch()` changes. Card width
  comes from `NodeGraphModel::node_width(id)` (default `NODE_WIDTH`), the right
  edge drags it (`set_node_width(id, width)`, at least
  `MIN_HOSTED_CARD_WIDTH`), and the height fits the body
  (`Widget::measure_min_height`), growing down from the fixed top and reported
  through `on_node_measured(id, height)` when it changes. The pressed card is
  painted on top with its sockets (`hosted_card_order`, `raise_card`); a
  socket or the resize band over a body belongs to the editor. Nodes the
  factory declines keep the simplified card; without a factory nothing
  changes. All new `NodeGraphModel` methods have defaults.
- `NodeEditor::with_collapse_enabled(bool)` and `with_snap_guides(bool)` turn
  off the collapse toggle and the node-drag snap guides (both on by default).
- `SocketSide` is re-exported at the crate root.

### Fixed

- Dashed noodles stroke with butt caps, as MatterCAD / NodeDesigner do; the
  default round cap nearly closed their 8-unit gaps.
