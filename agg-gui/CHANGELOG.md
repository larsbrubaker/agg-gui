# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [0.3.0] - 2026-07-16

Large feature release centred on rich-text editing, multiline text input, a
pan/zoom Scene container, a first-class Popup API, and a broad round of widget
parity work against egui 0.34.3. Includes a few semver-relevant changes — see
**Breaking / behavioural changes** below before upgrading.

### Breaking / behavioural changes

- **`EventResult` gained a `ConsumedQuiet` variant.** Consumed events now
  auto-request a redraw by default; `ConsumedQuiet` stops propagation *without*
  scheduling a repaint. Any `match` on `EventResult` that assumed the previous
  variant set is now non-exhaustive. Migrate consumption checks to
  `EventResult::is_consumed()` (returns `true` for both `Consumed` and
  `ConsumedQuiet`) instead of matching `EventResult::Consumed` directly.
- **Default flex spacing is no longer zero.** `FlexRow`/`FlexColumn` built via
  `::new()` now start with `DEFAULT_ROW_GAP = 8.0` and
  `DEFAULT_COLUMN_GAP = 4.0` (mirroring egui's `item_spacing`) instead of `0.0`.
  Adjacent controls now breathe by default; restore the old joined/segmented
  look with `.with_gap(0.0)`. The constants are re-exported from the crate root.

### Added

- **`TextHAlign` / `TextVAlign`** — new public alignment enums controlling the
  horizontal and vertical placement of a `TextArea`'s wrapped text block,
  mirroring egui's `TextEdit` alignment options.

- **RichTextEdit widget suite** — a full rich-text stack: document model,
  command engine, layout engine, a read-only `RichTextView`, and an interactive
  `RichTextEdit` editor with a formatting toolbar (bold/italic gated by the
  family's real variants, block alignment, lists). Enter on an empty list item
  outdents/exits the list. Backed by a cached LCD backbuffer that invalidates on
  async font arrival. A dedicated RichTextEdit demo window exercises the toolbar.
- **Scene pan/zoom container** — a `Scene` widget that hosts content as a
  first-class child under a `child_transform`, added via the new
  `Widget::child_transform` hook for scaled subtrees (focus, click-to-focus, and
  the inspector all reach hosted children correctly). Rebuilt Scene demo.
- **Popup API** — `RectAlign` placement plus configurable close behaviors built
  atop the menu system (overflow flip, Escape-to-close, egui-matching defaults).
  The Popups demo was rebuilt as a live Popup-API configurator.
- **TextArea multiline editing** — `on_change` callback, internal vertical
  scrolling with scroll-to-cursor, Home/End/PageUp/PageDown navigation, hint
  text, content alignment, and a shared-state selection API. Renders through the
  LCD backbuffer pipeline. Powers the rebuilt TextEdit and Code Editor demos.
- **Double-click word / triple-click line selection** across the text editors.
- **`DrawCell<T>`** — shared UI state that invalidates the frame on change, for
  cell-driven runtime flags.
- **`agg_gui::undo`** — the time-coalescing `Undoer<State>` (plus `Settings`) is
  promoted into the library from the demos, alongside the existing `UndoBuffer`.
- **Window runtime flags** — `Window::with_modal`, plus cell-driven `resizable`,
  `collapsible`, `auto-size`, and `title` that track live state.
- **Smaller widget features** — `Checkbox` indeterminate/tri-state, password
  reveal, `DragValue` suffix, value-cell binding, and intrinsic minimum width so
  numbers never clip, `ProgressBar` animate (spinner), and a horizontal
  `RadioGroup`.
- **`Font::characters()`** — enumerate a font's real glyph coverage; the Font
  Book demo was rebuilt on top of it.
- **`Slider` full option set** — egui-parity options (step, logarithmic,
  clamping, custom formatting, etc.) plus a `slider_math` module; Sliders demo
  rewritten as a live configurator.
- **Interactive tooltips** — an overlay tooltip mode supporting nested tips.
- **`Label::with_strong`** and **`ComboBox::is_open`** helpers.
- **Confetti particle overlay** primitive.
- **HiDPI per-subtree opacity** — compositing layers honour `global_alpha` at
  any device scale.
- **Font-preview family dropdown** for the RichText toolbar (each family shown
  in its own face) plus a `snap_baseline_y` line-level hinting helper.

### Changed

- **Consumed events auto-invalidate.** The dispatcher now schedules a redraw
  when an event is consumed (see the `ConsumedQuiet` opt-out above); the
  interactive `Container` requests a draw on background click/hover/press.
- Menus enforce touch-safe minimum sizes so they can't ship accidentally tiny
  on mobile.
- Demo overhaul toward egui 0.34.3 parity: Widget Gallery Visible/Interactive/
  Opacity scopes and striping, Manual Layout Test with real absolute placement,
  Flex Layout Test (renamed from Layout Test), Misc Demos sections including
  Resize, Input Event History with dedup/movement toggle, live Window Options,
  unified Undo/Redo, Code Example source context, and a shared `source_link`
  helper. Retired several low-value demos and the Grid Test window; folded LCD
  Subpixel into System. Removed user-visible "egui" mentions from demo-ui.
- `egui-reference/` synced to 0.34.3.

### Fixed

- **LCD/subpixel text inside compositing layers** now renders correctly on both
  the software and wgpu backends (alpha-writing text pipelines; the wgpu path no
  longer washes out LCD text in layers, and `pop_layer` composites are clipped
  to the parent scissor).
- HiDPI compositing layers and `global_alpha` in the software backbuffer blits.
- `Slider` no longer produces `NaN`/`inf` on infinite (logarithmic) ranges, and
  reversed ranges no longer panic on clamp.
- Color-wheel picker dialog no longer leaks clicks through to widgets beneath it
  (modal click-through fixed).
- TextArea: first-line ascender clipping at scroll top; caret clipping on
  scroll; external cursor moves scroll into view.
- Scene: false double-click reset when a click straddles a hosted child; the
  reset decision is now made at release, not press.
- Disabled-scope focus leak and container hover over nested buttons.
- Cross-thread async wake-up (markdown SVG badge painted at the wrong scale
  until an unrelated event arrived).
- Highlighter double-paint, a stale attribute, and the painting stroke default.
- `Stack` no longer panics when a raise-requesting child sits beyond the
  parallel `aligned` vec.

[0.3.0]: https://github.com/larsbrubaker/agg-gui/releases/tag/v0.3.0
