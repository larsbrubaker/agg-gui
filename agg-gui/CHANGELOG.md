# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [Unreleased]

### Added

- Popup menu styling: `MenuStyle` gains `row_h` (item row height,
  touch-floored), `shadow` (drop shadow on/off), `background` /
  `border_color` (panel fill and outline, `None` = theme colours),
  `border_width` (`0.0` = no outline), `width` / `min_width` (fixed or
  content-fitted width and its floor) and `shortcut_format`
  (`ShortcutFormat::Platform`, today's `⌘X` on macOS / `Ctrl+X` elsewhere, or
  `ShortcutFormat::Plain`, `Ctrl+X` everywhere as agg-sharp shows it).
  `set_menu_style` / `current_menu_style` / `reset_menu_style` install a
  thread-local default every `PopupMenu` created afterwards starts from
  (context menus, menu bars, the text-editing menus); `PopupMenu::with_style`
  / `set_style` restyle one popup. Defaults are unchanged. `MenuStyle` is now
  `Copy + Debug + PartialEq`; `FitMeasure` gained a `shortcut_format` field.

- File drag-and-drop beyond native path drops. `Event::FileDataDropped
  { pos, files: Vec<DroppedFileData> }` carries dropped files as name + bytes
  (`DroppedFileData`, bytes shared through an `Arc<[u8]>`) for the browser,
  which never reveals paths; native shells keep sending `Event::FileDropped`.
  `Event::FileDragHover { pos, paths }` reports a file drag over the window
  (paths where the platform reveals them, empty in the browser) and
  `Event::FileDragLeave` ends it — delivered to every widget, once per drag,
  when the drag leaves, is cancelled or ends in a drop (then before the drop
  event). Hovers and byte drops route like `FileDropped`: the widget under
  `pos` (local coordinates), then the rest of the tree if that path ignored
  it. Entry points: `App::on_file_data_dropped`, `App::on_file_drag_hover`,
  `App::on_file_drag_leave`, `App::file_drag_active`. Code that matches
  `Event` exhaustively needs arms for the three new variants.

- Pointer enter/leave for every widget on the hovered chain:
  `Event::MouseEnter` / `Event::MouseLeave` (agg-sharp's `MouseEnterBounds` /
  `MouseLeaveBounds`). Whenever the pointer moves, `App` sends `MouseLeave` to
  each widget that is no longer under it (deepest first) and `MouseEnter` to
  each that now is (shallowest first) — the root and every ancestor of the
  hit widget, not just the deepest — before the `MouseMove` is dispatched,
  so a composite whose child handles the moves (a wrapper around a
  `TextField`, a tab strip) still learns the pointer arrived and left. They
  are delivered directly (no bubbling); returning `Consumed` only schedules a
  repaint. While pointer capture holds, the captured widget's ancestors stay
  entered and only the captured widget leaves and re-enters as the pointer
  crosses it; the rest settles when capture ends. Leaving the window sends
  `MouseLeave` up the whole chain. Who handles `MouseMove`, and the
  `(-1, -1)` hover-clear move, are unchanged; widgets that ignore unknown
  events need no change (code matching `Event` exhaustively must add the two
  variants).
- Widget code can clear keyboard focus: `focus::request_blur()` clears it
  whatever holds it; `focus::release_focus(id)` clears it only if the widget
  whose `focus_id` is `id` still holds it (a no-op otherwise, so a stale
  release never steals focus from another widget). Both are serviced on the
  next `App::layout` like `request_focus`: the widget that had focus gets
  `FocusLost`, and later keys take the unconsumed-key path. The latest of
  `request_focus` / `request_blur` wins; `release_focus` cancels only a
  pending `request_focus` for its own id. Lower level:
  `focus::BlurRequest::{Any, Owner(id)}`, `take_blur_request`,
  `clear_blur_request`.
- Adjustable floor for content-fitted popups: `PopupMenu::with_min_width` /
  `set_min_width(f64)`, `MenuBar::with_menu_min_width(f64)`, and the lower-level
  `PopupMenuState::set_min_width` / `min_width` and
  `FitMeasure::popup_width_with_min`. The default stays
  `fit_width::FIT_MIN_W` (156 px), so existing menus are unchanged; `0.0`
  drops the floor so each `MenuWidth::FitContent` panel is exactly its widest
  row rounded up (agg-sharp menus that reset `row.MinimumSize` to `(0, …)`).
  The value is desktop logical px, touch-grown like the default, and applies
  to the root and every submenu alike. It has no effect on `MenuWidth::Fixed`.
- Content-fitted popup menus: `MenuWidth::{Fixed, FitContent}` (re-exported
  from `widgets::menu`). `PopupMenu::with_width` / `set_width` and
  `MenuBar::with_menu_width` opt in; with `FitContent` each popup level (root
  and every cascaded submenu) is as wide as its widest row's natural width —
  label from `MenuStyle::label_x`, the shortcut `fit_width::SHORTCUT_GAP`
  (20 px) after it, then `MenuStyle::shortcut_right` — rounded up to a whole
  px and floored at `fit_width::FIT_MIN_W` (156 px, agg-sharp's 150 px row
  minimum plus its 3 px row inset each side; touch-grown with the other
  metrics). Rows are measured exactly as their `Label`s lay out (system font
  override and font-size scale included). `PopupMenu::set_measure_font` gives
  a fitted popup its font before the first paint so early events hit-test
  the fitted panel; `paint` refreshes it every frame. Lower-level pieces:
  `PopupMenuState::set_width` / `width` / `set_fit_measure`, `FitMeasure`, and
  `geometry::stack_layout_with_width`. `MenuWidth::Fixed` stays the default,
  so existing menus keep their width.

- `Button::with_focus_on_click(bool)` — opt a button out of click-to-focus.
  With `false`, a click still fires `on_click` but clears keyboard focus
  instead of taking it, so a following Space / Enter reaches the
  unconsumed-key / global key handling rather than re-clicking the button
  (toolbar buttons). Tab traversal and programmatic focus still focus the
  button, and Space / Enter activate it when focused that way. Backed by the
  new `WidgetBase::focus_on_click` field (default `true`), which the `App`'s
  click-to-focus rule reads for any widget embedding a `WidgetBase`.
  Default behaviour is unchanged.

- `animation::request_layout()` — ask for another layout *and* paint pass.
  Unlike `request_draw`, the request survives `App::paint`'s
  `clear_draw_request()` and is consumed only at the start of `App::layout`,
  so a widget calling it from `layout` (or `paint`) gets one more laid-out
  frame. `animation::wants_draw()` stays `true` while it is pending;
  `animation::layout_requested()` peeks it and
  `animation::take_layout_request()` consumes it. `request_draw` is unchanged.

- `ComboBox::with_style(ComboBoxStyle)` — optional per-instance overrides for
  the closed box: `fill`, `border`, `hover_fill`, `radius` and `height`
  (`height` also drives hit-testing and where the popup attaches). Every
  field defaults to `None`, which keeps the existing `Visuals`-driven look.
  `ComboBox::style()` reads it back.
- `Slider::with_style(SliderStyle)` — optional per-instance `track` color,
  `track_radius` and `thumb` color (a thumb override applies in every
  interaction state). `None` fields keep the current appearance.
  `Slider::style()` reads it back.

### Fixed

- A content-fitted child (a `FlexColumn` with `fit_width` or no flex rows)
  keeps its rows inside its own box. `AbsoluteLayout` lays a child out again
  whenever its final box differs from the space it was measured in (it used
  to compare with the size the child reported, so a fitted column kept rows
  placed from the top of, and centred in, the larger offered space), and
  `FlexColumn` lays a child out again at its final width when that differs
  from the slot (a centred or fit-width child). The `Widget::layout` docs
  state the contract.

- `ColorPicker` no longer paints its "No Color (Pass Through)" checkbox over
  the swatch while the panel is closed; the panel's sub-widgets paint only
  with the open panel (and no longer twice in the inline panel).

- `MenuBar` paints its background over its laid-out bounds instead of
  re-reading `effective_metrics().bar_h` at paint time, so an input-profile
  or touch-latch flip between layout and paint no longer fills a strip of the
  wrong height for a frame (and the fill always covers the bounds, as the
  `LcdCoverage` backbuffer mode requires).

- The text context-menu tests (`TextContextMenu`, `TextField`, `TextArea`,
  rich-text editor) that click popup rows hold `profile_test_lock()` and pin
  `InputProfile::Desktop`, so a sibling test flipping the profile to touch
  can no longer move a row out from under the click.

- Pointer capture, hover, focus and gesture capture now follow their widget
  when a parent reorders its children. `App` stored them as child-index
  paths, so a container that moved children during a drag (a tab strip
  reordering the dragged tab past its neighbours) sent the rest of the drag,
  including the release, to whichever widget took the old index. Each stored
  path is now anchored to the identities of the widgets along it (their heap
  addresses, which a `Vec<Box<dyn Widget>>` reorder never changes) and
  re-resolved after every layout pass and at the start of each input entry
  point. No API change: containers may reorder `children` at any time
  (in `layout`, in `on_event`, or through `App::root_mut`).

- `TextField` with `select_all_on_focus`: the click that focuses the field now
  selects all of its text on release, so typing replaces the value instead of
  inserting at the click point (previously the focusing click's caret
  placement cleared the focus selection, turning "20" into "2030" when typing
  "30"). A drag during that click selects the dragged range instead, and
  later clicks position the caret as before. Tab focus still selects all
  immediately. Matches agg-sharp `InternalTextEditWidget`.
- Software rendering of paths that run far off screen no longer costs time
  proportional to their length. `GfxCtx` fills and strokes, `fill_text_gsv`
  and the LCD coverage mask now also clip in AGG's cell rasterizer
  (`RasterizerScanlineAa::clip_box`, set to the active scissor intersected
  with the target plus a 1 px margin), not only at the renderer. A 1 px
  stroke from (−200000, −20000) to (40000, −4000) on a 1300×800 buffer drops
  from ~6 ms to ~0.14 ms per frame (release build), the cost of an empty
  frame. Paths inside the clip render bit-for-bit as
  before; edges that cross it can differ by one unit in a channel, since
  AGG restarts them at the clip boundary on its 1/256-pixel grid.

## [0.5.1] - 2026-09-28

### Added

- `DrawCtx::clip_path` — intersect the clip with the current path
  (anti-aliased) until the matching `restore()`; `DrawCtx::supports_clip_path`
  reports whether a backend implements it. The software `GfxCtx` rasterises an
  8-bit coverage mask applied when the clip layer pops.
- `DrawCtx::ellipse` — trait default built from cubic Béziers, with canvas-2D
  angle semantics (`draw_ctx::defaults::ellipse_ops` / `EllipseOp`).
- `animation::clear_immediate_draw_request` — clears only the immediate draw
  flag, leaving scheduled deadlines and pending cross-thread async wakeups in
  place. For a host whose surface refused a frame: the frame never reaches
  `App::paint`, so the flag would otherwise stay set and keep a reactive host
  polling.

### Changed

- `wheel::WheelNormalizer` no longer banks sub-notch travel: fractional
  deltas pass straight through (a trackpad on the web scrolled in 40 px jumps;
  now it is continuous, matching the native shell), while a classic wheel
  still arrives as whole notches. It is now a stateless unit struct;
  `WheelNormalizer::reset` is kept as a no-op so 0.5.0 callers still
  compile, and will be removed in 0.6.
- `DrawCtx::draw_image_rgba_corners` documents that corner quads are always
  sampled with linear filtering.

### Fixed

- A cross-thread async wakeup (`signal_async_state_change`) that landed between
  the host's last `wants_draw()` and `App::paint` was silently dropped — no
  dirty walk, no retained-cache invalidation, no further draw.
  `clear_draw_request` now pumps pending wakeups into the epochs before
  clearing, and `App::paint` requests one follow-up draw when a signal arrived
  after layout, so the next frame re-lays-out (larsbrubaker/agg-gui#6).

## [0.5.0] - 2026-08-25

### Added

- **Touch and multi-touch.** Multi-touch gestures route as captured events with
  a per-finger touch registry, and the web shell forwards browser touch events;
  the Lion demo gains pan. Device-tilt input (`agg_gui::tilt`) rides the same
  plumbing.
- **Gamepad support** — `agg_gui::gamepad` pad-state plumbing, polled through
  the Web Gamepad API in the browser shell.
- **Keyboard focus and window control.** `App::focus_first` seeds initial
  keyboard focus for canvas/game roots; app-requested fullscreen toggling is
  handled by both shells; `Window` gains a chromeless mode.
- **Widgets and editors.** `SegmentedControl` and `Spinner`; a segmented-strip
  enum editor for `property_row`; registered vector icons plus an `EnumIcons`
  strip; default/cancel actions; the node editor's inline value editing,
  interaction modes, animated fit-to-content, and `SplitterRatio`.
- `Font::with_tabular_digits` / `variant_with_tabular_digits` — shape with the
  OpenType `tnum` feature, sharing the face bytes with the source font.
- The shared scrollbar helpers (`ScrollbarAxis`, geometry, painter) are public
  so widgets that own their scroll offset can reuse them.
- `tree_inspector::find_widget_screen_rect` — absolute widget placement by id.
- An optional animation host waker so reactive hosts wake for async completions.

### Changed

- Wheel input normalizes DOM wheel deltas to notches; the web shell adopts it.
- `winit_adapter` round-trips unmapped named keys as `Key::Other` instead of
  dropping them.
- LCD backbuffer alpha collapses by Rec.709 weight.
- `Gutter` numbers soft-wrapped lines by their first visual row.
- `SegmentedControl` uses whole-pixel segment widths in equal-width mode.

### Fixed

- An active modal owns Enter/Escape — no root default/cancel dispatch behind it.
- Any non-left press dismisses an open menu.
- The post-layout keyboard lift is gated on `accepts_text_input`.
- LCD subpixel text inside opaque compositing layers renders correctly on the
  wgpu backend again.

## [0.4.0] - 2026-07-22

### Changed

- Depend on `agg-rust` 1.1.0 and `clipper2-rust` 1.1.0.

### Performance

- **`TextArea` scroll + edit no longer re-raster the whole LCD backbuffer.**
  Previously any change — a wheel notch or a single keystroke — invalidated the
  widget's entire viewport-sized LCD backbuffer and rebuilt it through the
  supersample + filter + plane-flip pipeline (~120 ms of fixed machinery at
  1400×1000, independent of text volume). Two changes fix this:
  - **Over-scan band backbuffer.** The widget now rasters a band covering the
    viewport plus roughly one viewport of over-scan, anchored in *content* space
    and excluded from the cache signature. Scrolling within the band is a pure
    (physical-pixel-quantized) blit-offset change — no re-raster — so the LCD
    subpixel structure is never resampled; the band re-anchors and rebuilds only
    when the view leaves it. The band blit is clipped to the widget bounds on
    every backend so the over-scan margins never paint over sibling widgets.
    Measured (release, 1400×1000): scroll frame 143 ms → ~27 ms (the warm blit
    floor).
  - **Dirty-line-strip edits.** An in-place edit with a collapsed caret now
    re-rasters only the changed line strip into the *retained* band buffer
    (background fill + LCD supersample + filter restricted to the strip; no
    realloc, no full clear) instead of rebuilding the whole band. A same-wrap
    single-line edit dirties one line; an edit that changes wrapping dirties from
    the first affected line to the band bottom. Falls back to a full band repaint
    for anything structural (resize, re-anchor, selection, width re-flow). A
    strip re-raster reproduces a full re-raster pixel-for-pixel. Measured: single
    character edit frame ~143 ms → ~81 ms.

  Plumbed through an opt-in `Widget::backbuffer_band` hook (default `None`, so
  every other widget's paint stays byte-identical) plus a retained
  `BackbufferCache` LCD buffer used only on the band path.

### Added

- **Live draw-report hotkey + runaway auto-detector in the demos.**
  `agg_gui::debug_draw_report(root)` returns a diagnostic string — raw
  `needs_draw` flag (read side-effect-free via `animation::peek_draw_signals`,
  so reading it never perturbs the runaway), the next scheduled deadline with
  remaining time, the drained draw-trace tags deduplicated with counts, and
  every visible widget whose `needs_draw()` is `true` (child-index path +
  `<- self` marker on the driver). Press **Ctrl+Shift+D** in either demo to
  emit it: native prints to stderr and appends it (timestamped) to
  `.agg-gui-draw-debug.log` next to the state file; web logs it to the browser
  console. A pure `RunawayDetector` also auto-fires the report once (tagged
  `AUTO-DETECTED RUNAWAY`) after 240 consecutive input-free reactive frames
  (~4 s), so an intermittent "reactive host never quiesces" runaway is captured
  even when unnoticed. Auto-detection + the log file are native-only (no browser
  filesystem).

- **Draw-request provenance trace for diagnosing continuous-repaint cascades.**
  `animation::request_draw_tagged(reason)` / `request_draw_after_tagged(delay,
  reason)` record a `&'static str` reason tag into a thread-local ring buffer
  (debug builds only — compiled out in release, so shipping hosts pay nothing),
  drained via `animation::drain_draw_trace()`. The ~dozen library re-arm sites
  that keep a reactive host awake (tooltip controller, tooltip wrapper,
  interactive tooltip, progress-bar pulse) now tag their requests so a test that
  catches the app failing to go idle can name *which* signal is holding it hot.
  Two permanent quiescence regression guards use this (demo-ui
  `app_builder_tests`): the reactive demo with every window closed must reach
  `wants_draw() == false` with no scheduled deadline armed, and every animated
  demo window (Multi Touch, Dancing Strings, …) must return the app to idle
  after it is closed — pinning the invariant that idle means idle and that the
  widget-tree `needs_draw()` walk excludes closed / collapsed subtrees.

- **`TextArea` standard code-editor keyboard set.** Word-wise caret movement and
  selection (Ctrl/Alt+Left/Right, with Shift to extend), word-wise deletion
  (Ctrl/Alt+Backspace/Delete, deleting an active selection first), and
  Tab/Shift+Tab line indent/outdent: a plain Tab over a multi-line selection
  indents every touched line while Shift+Tab outdents (one leading tab or up to
  four leading spaces per line), with the selection preserved across the change.
  The Code Editor demo inherits all of it.
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
  drops its `max_size`. **Host contract:** tips render with the crate-wide
  system font and follow it live — hosts must install one at startup via
  `font_settings::set_system_font` (the library ships no font; with none
  installed the timing machine runs but nothing paints).
- **`touch_emulation` module** — the primary-finger touch→mouse emulation
  (tap = left click, drag past 8 px = middle-button pan) moved from the web
  shell's JS into core as the unit-tested `TouchMouseEmu` state machine.
  `App::on_touch_start/move/end/cancel` now replay its commands through the
  `on_mouse_*` pipeline automatically, so every platform shell gets identical
  single-finger behaviour by forwarding raw touches only.

- **Tab / Shift+Tab indent in the rich-text editor.** A focused `RichTextEdit`
  now maps **Tab** → increase indent and **Shift+Tab** → decrease indent for
  every block the selection touches (a list item's level is its block indent),
  reusing the toolbar's Increase/Decrease-indent commands and landing as one
  undo step each. The keys are consumed so focus traversal no longer fires
  mid-edit; **Ctrl/Meta+Tab** still traverses focus as the escape hatch. This is
  now general: `App::on_key_down` offers plain Tab / Shift+Tab to the focused
  widget first and only advances focus when the widget ignores it.

- **Word-wise deletion in the rich-text editor.** A focused `RichTextEdit` now
  maps **Ctrl/Alt+Backspace** → delete to the previous word boundary and
  **Ctrl/Alt+Delete** → delete to the next, mirroring egui's
  `delete_previous_word` / `delete_next_word`. The span removed is exactly what a
  Ctrl+Arrow motion traverses (same word boundaries), including a merge into the
  adjacent block at a block edge; an active selection is deleted verbatim, and
  each word delete is a single undo step.

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

- **Trailing spaces didn't move the insertion caret in the text editors.**
  Typing spaces at the end of a line left the caret parked on the last visible
  glyph until a non-space character was typed. The shared text-measurement path
  (`measure_advance`) was correct — spaces carry a real advance — so the fault
  was per-editor caret geometry measuring a whitespace-trimmed line instead of
  the source text. `TextArea` trimmed trailing whitespace from each wrapped
  line's cached text and clamped the caret to that trimmed length, so
  `pos_for_cursor` now measures the untrimmed source substring
  `[line.start..byte_pos]`. `RichTextEdit`'s word-wrap dropped a dangling
  end-of-line space piece entirely (the same drop that removes a space at a wrap
  point); the layout now keeps a trailing space at the end of a line so the
  caret can advance onto it (a space at a genuine wrap point is still dropped).
  `TextField` (single-line, never trimmed) was already correct and gains a
  regression guard. All match egui, where trailing spaces advance the caret on
  the row. Caret x/rect at a trailing space, and a trailing space typed at a
  wrap boundary, are now covered by tests in each editor.
- **Scheduled draws could be silently lost in reactive hosts.** The
  scheduled-draw channel (`animation::request_draw_after`) was read
  destructively, so a reactive host armed `ControlFlow::WaitUntil` from a
  read-and-cleared deadline. Any intervening event that did not itself repaint
  (cursor jitter, `ModifiersChanged`, IME/focus chatter, spurious wake) left
  the next idle iteration with an empty cell and replaced `WaitUntil` with a
  plain `Wait`, stranding the wake — so tooltips (and historically cursor
  blink, scrollbar fades, grace-close) never fired in Reactive run mode though
  they worked in Continuous. The channel is now read non-destructively
  (`peek_next_draw_deadline`) so hosts re-arm `WaitUntil` idempotently every
  idle iteration, and a due deadline surfaces through `wants_draw()` — making
  it indistinguishable from an immediate `request_draw`, so no host can lose
  it.
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

[0.4.0]: https://github.com/larsbrubaker/agg-gui/releases/tag/v0.4.0
[0.3.0]: https://github.com/larsbrubaker/agg-gui/releases/tag/v0.3.0
