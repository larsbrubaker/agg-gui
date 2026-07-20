# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [Unreleased]

### Added

- **First-class tooltip system.** Any widget can now declare hover help with no
  wrapper: `WidgetBase` carries an optional `tooltip` string, exposed through the
  universal `Widget::with_tooltip("…")` builder (and `set_tooltip_text` on a
  `&mut dyn Widget`) — every widget that embeds a `WidgetBase` opts in for free,
  and the controller reads it back via `Widget::tooltip_text`. A single central
  controller in the `App`'s per-frame pass finds the deepest hovered tipped
  widget, runs one app-wide timing state machine (so only one tip is ever
  visible), and paints the tip in the global-overlay pass with edge-aware
  placement (prefer below-right of the pointer, flip/clamp at the viewport edge).
  Timing is system-driven where available: an initial hover delay (seeded on
  native Windows from `SPI_GETMOUSEHOVERTIME`), a much shorter *reshow* delay when
  moving directly between adjacent tipped controls, hide-on-press / hide-on-leave,
  and an autopop timeout that dismisses a tip left sitting under a still pointer —
  configure via `set_tooltip_timings(TooltipTimings { .. })`. The `Tooltip`
  wrapper is now the **rich-content** path (a custom child widget tree as the tip,
  à la egui `on_hover_ui`); it shares the same timing state, so simple text tips
  should use `with_tooltip`. The `RichTextToolbar` and the demo toolbar were
  migrated to `with_tooltip` (every control still self-documents; opt out with
  `RichTextToolbar::with_tooltips(false)`, and Undo/Redo still include the
  editor's real key binding). `Tooltip` also forwards its child's size
  constraints, so wrapping a constrained control (e.g. a `ComboBox`) no longer
  drops its `max_size`.
- **`touch_emulation` module** — the primary-finger touch→mouse emulation
  (tap = left click, drag past 8 px = middle-button pan) moved from the web
  shell's JS into core as the unit-tested `TouchMouseEmu` state machine.
  `App::on_touch_start/move/end/cancel` now replay its commands through the
  `on_mouse_*` pipeline automatically, so every platform shell gets identical
  single-finger behaviour by forwarding raw touches only.

### Breaking / behavioural changes

- **Platform shells must no longer synthesize mouse events from touches.**
  Shells that mirrored the old JS contract (manually calling `on_mouse_*` for
  the primary finger around `on_touch_*`) will double-fire events — delete the
  shell-side emulation and forward raw touches.
- `touch_state::note_touch_event` now runs *before* the synthetic mouse events
  a touch produces, so `last_touch_event_age()` reliably identifies them as
  touch-synthesised (previously the very first touch's mouse-move looked like
  desktop input).

### Changed

- **Rich-text colour pickers no longer show "No Color (Pass Through)".** The
  checkbox confused the rich-text flow; the core `ColorWheelPicker` keeps the
  feature behind `with_allow_none` for other hosts. The Highlight picker's
  seed over un-highlighted text is now a visible default instead of
  transparent (an alpha-0 seed would have stranded the picker emitting `None`
  with the checkbox gone). The rich-text toolbar gained a **Remove-highlight**
  button (Font Awesome eraser) that issues `SetHighlight(None)` — the sole UI
  route to un-highlight text now that the checkbox is gone.

### Fixed

- **Nested modal windows snap in canvas space.** A modal dialog hosted in an
  overlay slot (e.g. the rich-text colour panel) fed its slot-local bounds to
  the snap engine and registry, so it snapped to phantom edges and corrupted
  other windows' snap targets. The window now caches its canvas-absolute
  offset at paint time and snaps/registers in canvas space.
- **Two-finger twist no longer jumps wildly at the ±π seam.** The gesture
  recogniser normalised rotation only after averaging per-finger deltas, so a
  finger sweeping through the atan2 seam (every half-turn of a real twist)
  corrupted that frame's `rotation_delta` by nearly ±π. Each finger's delta is
  now wrapped into `[-π, π]` before averaging.
- **Pinch gestures no longer fire a phantom left click** when the first finger
  moved less than the tap threshold before release.
- **A second finger now releases the in-flight middle-drag pan immediately**,
  so two-finger zoom/rotate no longer scrolls the surrounding view through the
  first finger.

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
- **`Window::on_close` now receives a `CloseReason`.** The callback signature
  changed from `FnMut()` to `FnMut(CloseReason)` so a host can react to *how*
  the window closed (× button, Escape, or click-away). `on_close` shipped only
  in the never-tagged 0.2.2-era code, so this lands as part of 0.3.0's new
  public surface rather than a break against a published release; update any
  `.on_close(|| …)` call to `.on_close(|_reason| …)`.

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
- **RichEditHandle / RichTextEdit programmatic APIs** — `select_all`,
  `set_caret`, `set_selection` (clamped), `selection`, `plain_text`, and `load`
  (replaces the document and resets the undo history and any in-flight colour
  preview) for driving an embedded editor from code.
- **`RichTextToolbar`** — a configurable, fully self-contained formatting toolbar
  widget driven by a `RichEditHandle`: bold/italic/underline/strike (Bold/Italic
  gateable through an injected `Variant` check), alignment, ordered/bullet lists,
  outdent/indent, undo/redo, a font-size dropdown, and text/highlight colour
  swatches. Every control group toggles off through the builder; the font-family
  dropdown is opt-in via `with_families` (the library takes no font catalog).
  Colours open a floating, modal `color_wheel_picker_dialog` that the toolbar
  hosts internally — it paints through the global-overlay pass, so no companion
  layer or top-level `Stack` is needed. Colour edits drive a live preview through
  the handle's preview session (`begin/commit/cancel_preview`): the selection
  recolours as the wheel is dragged, commits on Select, and restores on
  Cancel / × / Escape.
- **Modal window click-away + `CloseReason`** — `CloseReason`
  (`CloseButton` / `Escape` / `ClickAway`), `ClickAwayAction`
  (`None` / `Close`), and `Window::with_click_away(...)`. A modal window can now
  opt into dismissing on a pointer press outside its bounds (the press is
  swallowed, never activating the widget underneath). The floating colour dialog
  enables this: pressing outside it **commits** a live colour change as one undo
  step when the user recoloured (Ctrl+Z reverts it), or closes silently when
  nothing changed — while Escape / × still cancel. A modal window is also now
  draggable across the whole app viewport rather than being caged to the small
  overlay slot it nests in.
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
  numbers never clip, `ProgressBar` animate (brightness pulse), and a horizontal
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
