# Changelog

All notable changes to `agg-gui-node-editor` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [Unreleased]

### Added

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
