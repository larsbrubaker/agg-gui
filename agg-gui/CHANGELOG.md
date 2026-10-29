# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [Unreleased]

### Added

- In a browser the right-click menu's Paste in `TextField`, `TextArea` and `RichTextEdit` reads
  the system clipboard too, as the touch toolbar's does (`navigator.clipboard.readText()` inside
  the click, pasted when the read resolves), so text copied in another app pastes. The app's own
  last copy keeps its rich flavour.

- In a browser the touch toolbar always offers Paste in an editable field and pastes what the
  phone's system clipboard holds: the tap reads it with `navigator.clipboard.readText()` and the
  text lands in the field when the read resolves (a refused or empty read pastes nothing and is
  logged once). Native builds still read the clipboard directly.

- Touch text selection in `TextField` and `TextArea`, as on Android and iOS: a long press selects
  the word under the finger and sliding on extends by whole words; a handle at each end (Android's
  teardrop, iOS's lollipop, by input profile) drags that end by character, never past the other,
  scrolling a `TextArea` when dragged past its edge; a floating Cut / Copy / Paste / Select all
  toolbar (the text menu's actions) shows above the selection on release and hides while a handle
  moves; a tap on the selection toggles it, a tap elsewhere places the caret, which on Android gets
  a draggable caret handle for four seconds; a long press past the text places the caret and offers
  Paste (offered only with text on the clipboard). A long press takes the character under the
  finger; handles move by whole characters (emoji with skin tones, flags, ZWJ sequences and
  combining accents stay whole) and a long press on an emoji selects just it; a password field is
  selected whole and never offers Cut or Copy. Handles whose caret a ScrollView scrolled away are
  neither drawn nor hit; a handle held past a `TextArea`'s edge keeps scrolling, faster the
  further out; the toolbar stays above the on-screen keyboard and inside the window, and a swipe
  that starts on one of its buttons presses nothing. Typing, a mouse press, losing focus and being
  disabled hide it all. The mouse is unchanged.

- `claim_long_press_drag()`: a widget handling a long press's right press keeps the finger, so the
  right button stays down, the slide arrives as moves and the lift as the right release
  (`TouchMouseEmu::hold_long_press`).

- `animation::schedule_draw_after(delay) -> ScheduledDraw`: a scheduled draw that can be withdrawn.
  `ScheduledDraw::cancel()`, or dropping the handle, takes the deadline back, so a watchdog for
  work that finished early no longer paints an idle window once more for nothing. Several can be
  pending at once, each with its own deadline; the host wakes for the earliest of them and of the
  `request_draw_after` deadline (`peek_next_draw_deadline`, `App::next_draw_deadline`), and
  cancelling the earliest hands over to the next. Scheduling and cancelling the earliest call the
  `frame_demand` hook, so a sleeping web loop re-arms its timer or sleeps outright. A due one is
  served like a `request_draw_after` deadline (`wants_draw` promotes it, `is_pending()` turns
  false); a paint before it is due keeps it; `reset_draw_signals` withdraws every one.
  `request_draw_after` is unchanged: fire and forget.

- `frame_demand`: a per-thread hook every draw, layout and idle-queue request
  calls (`set_hook` / `notify`), so a shell whose loop sleeps while idle
  wakes for requests made outside its loop. `ui_thread::next_due()` /
  `UiQueue::next_due()`: when queued work needs a drain.
  `animation::reset_draw_signals()`: clear every draw signal, a future
  deadline included. `tilt::enable_requested()` /
  `fullscreen::requested()`: peek at a pending request.

- `Widget::modal_hover_passes_outside` (default `false`): a modal that answers `true` lets pointer
  moves outside its bounds hover the widgets under the pointer as usual, so a light-dismiss panel
  with no backdrop leaves the cursors beside it showing (a splitter's resize pointer), including
  after `App::refresh_cursor`. Presses, releases, the wheel and keys still go to the modal, and a
  drag it captured keeps every move.

- Long press: a finger held still for `LONG_PRESS_DELAY` (500 ms) within `LONG_PRESS_SLOP` of where
  it went down, with no second finger, is a right press and release at the touch-down point, so
  every widget that opens a context menu on a right click opens it on a long press. The finger is
  then spent (its moves and lift emit nothing); a shorter tap is still a left click and a drag is
  still a drag. An idle app is woken for it. While the hold fills, `long_press_cue()` gives the
  point (logical, Y-up) and progress from `LONG_PRESS_CUE_DELAY` (150 ms), and `App::paint` draws
  a thin accent-coloured ring around the finger (`set_long_press_ring_drawn(false)` leaves the
  drawing to the app). `last_press_was_touch()` says whether the latest press came from a finger,
  and `on_long_press(listener)` is told each time one fires (agg-gui-web-shell vibrates for
  10 ms there). A hold on the on-screen keyboard types its key instead. `TouchMouseEmu` gains
  `poll`, `long_press_deadline`, `long_press_progress`, `take_long_press` and `cancel_long_press`.

- `ShortcutKey::F(n)`: a menu shortcut can be a function key. `MenuShortcut::parse` reads `"F1"`
  ... `"F24"` (either case; a lone `"F"` is still the letter), it matches `Key::F(n)`, and it is
  written by its cap's name (`F1`, `⇧F2` on a Mac). agg-gui-shell's native menu bar shows it with
  AppKit's function-key character; like every chord without Command it is shown but left to the
  window. A crate that matches `ShortcutKey` exhaustively needs an arm for it.

- `MarkdownView::with_end_spacing(false)`: drops the gap a paragraph, heading or list leaves below
  itself after the document's last block, so a view boxed by its own padding (a tooltip) has as
  much space under its last line as over its first. The default (`true`) keeps the gap, as before.

- `MarkdownView::with_paragraph_spacing(lines)` and `DEFAULT_PARAGRAPH_SPACING` (0.65): the gap a
  paragraph, heading or list leaves below itself, in lines of the body font, so a tooltip can keep
  its help line right under the name (MatterCAD passes 0.25). The default is unchanged.

- `MarkdownView::with_emphasis_color(color)` (and `emphasis_color()`): `*emphasis*` draws in that
  colour instead of the body text colour, so a secondary line such as a tooltip's help text reads
  as secondary; links keep the link colour. Without it emphasis draws as before.

- `Opacity` widget: wraps one child and paints it at an opacity read on every paint from a shared
  `Rc<Cell<f64>>` (`Opacity::new`) or a closure (`Opacity::from_fn`), so an animation drives a
  fade without rebuilding the tree. At 1 it adds nothing (no layer); between 0 and 1 the subtree
  renders through one compositing layer at that alpha and takes input as usual; at 0 it paints
  nothing, its global overlays included, the pointer passes through to what is behind and keyboard
  focus leaves it, while layout keeps its space.

- `Widget::suppresses_paint()` (default `false`): while true the paint traversal skips the widget
  and its whole subtree, inline and global overlays, without touching layout. `Opacity` uses it at
  opacity 0.

- `claim_web_shortcut(key)`, `release_web_shortcut(key)` and `is_web_shortcut_claimed(key)`: an app
  claims more Ctrl/Cmd shortcuts (with or without Shift, never with Alt) from the browser, so the
  web keyboard bridge `preventDefault()`s them after forwarding them; claiming `'s'` keeps "Save
  Page As" from opening when the app saves. The clipboard and undo set (C, X, A, Z, Y) stays
  claimed as before; nothing else changes until an app claims a key.

- `ModalSheet::centred_room(available)`: the largest panel a centred sheet draws within a host of
  that size (the host less the edge margin on every side, never negative), and the margin itself
  as `modal_sheet::EDGE_MARGIN` (24). A host sizing something to match the panel, such as a
  remembered dialog size it wants the sheet to draw at exactly, no longer copies the number.

- `web_adapter::install_keyboard_listeners_while(belongs, on_key)`: the web keyboard and clipboard
  bridge for a canvas that shares its page. While `belongs()` is false a key or clipboard event is
  the page's: not forwarded and never `preventDefault()`ed. `install_keyboard_listeners` is
  unchanged (it owns every key).

### Fixed

- A press whose widget is rebuilt away mid-drag (a list rebuilt with fewer children than the
  pressed one's index) keeps its pointer capture on the deepest ancestor still in the tree, which
  now receives the remaining moves, the release and a later `MouseCaptureLost`, wherever the
  pointer is. The capture used to be dropped silently, so the release went to whatever was under
  the pointer and nothing that saw the press start heard it end. A two-finger gesture capture
  follows the same rule. The capture is still dropped when only the root would be left.

- `input_profile_from_hint` and `platform_from_name` recognise a platform named `iOS` or `iPadOS`
  (Chrome's `userAgentData.platform` on an iPhone), which used to read as an unknown touch device;
  the demo passes the user agent along as a fallback.

- A password `TextField` no longer copies or cuts its text: Ctrl+C / Ctrl+X, Ctrl+Insert,
  Shift+Delete and the right-click menu (whose Cut and Copy are now disabled there) all leave
  the clipboard alone.

- A paint no longer discards a future scheduled draw: `clear_draw_request`
  (which `App::paint` starts with) clears a `request_draw_after` deadline only
  once it is due. The idle queue's drain arms a timed draw for its next
  delayed action or interval just before the paint; dropping it left a
  reactive host asleep with that work queued until the next input.

- On-screen keyboard: a tapped key's preview goes 100 ms after the lift, and a held key's alternates
  open at 380 ms, even when nothing else repaints. `App::paint` clears every scheduled draw as a
  frame starts, and the wakes the press and lift asked for were cleared by the very frame they
  also asked for, so an idle shell slept until the next caret blink or other repaint (the preview
  stayed up for up to a second on a phone). The keyboard now asks again on every paint until the
  moment passes. On the Gboard keyboard the letter preview is a circle 1.4 key widths across,
  centred over the key, with a soft shadow (`Style::preview_shadow`); the iPhone balloon is
  unchanged.

- A wheel over a markdown code block or table that scrolls sideways passes its vertical part
  to the page: the block takes only the sideways part, and only when the gesture is mostly
  sideways. A Mac trackpad's vertical two-finger scroll carries a little sideways drift, and
  over a wide block it used to scroll nothing.

- A `ScrollView` at the bottom of a travel that ends on a half pixel keeps following growing
  content under stick-to-bottom: the offset rounds to a whole pixel, half a pixel past the end,
  which no longer counts as away from the bottom.

- A `ScrollView` moves its content at once when input scrolls it (the wheel, a scrollbar drag
  or a click on the track, a middle-button pan, a keyboard lift, `scroll_rect_into_view`), not
  at the next layout: a press that arrives after such a scroll and before the next painted
  frame lands on what the scroll put under the pointer, and widget lookups report where the
  scroll put a row. On a slow GPU the next frame (and with it layout) can be many input events
  away. `set_scroll_offset` and writes to an offset cell from outside still take effect at the
  next layout.

- The on-screen keyboard's keys show their letters again: it paints after the widget tree with a
  context that has no font (every widget scopes its font to its own save/restore, and a GPU frame
  starts with none), so every text label drew nothing while the shapes and glyph keys showed. The
  labels now use the app's UI font (`font_settings::set_system_font`) and are centred on their
  measured width.

- The web keyboard bridge delivers the keyup of every key whose keydown it delivered, even when
  the keyup targets a DOM editor or focus left the canvas in between, so no key stays held in the
  app.

### Changed

- A long press in a `TextField` or `TextArea` selects text (see Added) instead of opening the
  right-click menu; a right click still opens it.

- `TOUCH_SCROLL_THRESHOLD`, the finger's tap tolerance, is 10 points (was 8). It is also the long
  press's slop, so a held finger that drifts never starts a scroll; a tap may now wobble 10 points.

- The rich editor's formatting toolbar draws vector icons (`toolbar_icons`) instead of Font Awesome
  glyphs: one stroke thickness for the whole set, every straight edge on a whole device pixel at
  1x, 1.5x and 2x.  `MarkdownFormatButtonKind::icon` still names the Font Awesome glyph, for
  callers that want it, but the toolbar no longer draws it; `ICON_DESIGN_SIZE` is now the icon
  box (16).  New `Button::with_icon_painter` draws an icon with a closure.

- **Breaking for apps whose pictures use other formats:** `image` is built with only the picture
  formats a web page shows (PNG, JPEG, GIF, WebP, BMP, ICO), which is what Markdown images,
  pictures inside SVGs and animated GIFs decode. The other formats `image` decodes by default
  (TIFF, OpenEXR, HDR, PNM, TGA, DDS, farbfeld, QOI) are the new opt-in `image-extra-formats`
  feature (not a default feature, so it applies with or without `default-features = false`); an
  app that shows such pictures must enable it. Together with image's multi-threaded JPEG decoder
  they were about 1 MB of a wasm.

- The tumble cube's face labels take their font from the caller when no system font is installed:
  `TumbleCube::new_with_label_font`, `CubeFaces::with_label_font` and
  `face_textures::get_face_image_with_font` (and `bundled_label_font`, the Noto Sans that
  `TumbleCube::new` keeps using). An app that passes its own font does not link Noto Sans.

- `LineBox::EmSnapped` (`Label`, `TextField`) snaps on the effective scale (device scale times the
  Text Size multiplier) instead of the device scale alone.  The toolbar's pixel grid and em-box
  icons do the same.

- The software `GfxCtx::fill_text` outline path snaps a hinted baseline to a device pixel row, not
  a logical unit: at 2x a baseline of 3.5 units is device row 7 (as a coverage-mask composite
  puts it), no longer row 8.  A backbuffered `Label` now lands on the same row as direct text.

- The wgpu backend's tessellated-outline text path (`fill_text` on a backend without a coverage-mask
  composite; the wgpu backend always has one, so it is driven directly in tests) snaps a hinted
  baseline to a device pixel row, as the software outline fill and the coverage-mask path do, not
  to a logical unit: at 1.5x a baseline of 10.5 units is device row 16, no longer row 16.5.
  `text::device_baseline_snap` is now public for this. A rotated or sheared transform keeps the
  logical-unit snap.

- `lcd_coverage::collapse_lcd_pixel` (and so `LcdBuffer::to_rgba8_top_down_collapsed` and the
  default `DrawCtx::draw_lcd_backbuffer_arc`) collapses to the ink's own straight colour
  (`c_i / a_i`) at the Rec.709-weighted alpha, instead of dividing the colour plane by that alpha.
  Non-black text no longer keeps each channel's own coverage through the collapse (a 0.2-gray
  label showed a channel spread of 29 inside a layer, black showed none), and the alpha lift for
  light ink is gone. Luminance over a neutral destination is unchanged. A channel with less than
  `LCD_COLLAPSE_CONFIDENT_ALPHA` of alpha blends toward the pixel's weighted mean colour.

- `font_settings::lcd_enabled()` no longer refuses LCD subpixel text above an effective scale of
  1.25: an explicit `set_lcd_enabled(true)` is honoured at every scale (a 150% ClearType laptop
  gets subpixel text, as in agg-sharp).  The 1.25 threshold remains only as the *default* when
  nothing was pinned (`font_settings::LCD_DEFAULT_MAX_SCALE`), since a platform without display
  detection cannot tell a striped LCD from a Retina panel.  The opaque-destination rule and the
  collapse of LCD coverage over transparent pixels are unchanged.

### Added

- `SliderStyle::full_length_track` (default `false`) draws a `Slider`'s rail as agg-sharp's
  `SlideView` does: across the whole length the thumb occupies, so the thumb sits flush with the
  rail's ends at the minimum and maximum and its centre travels the drawn rail less one thumb
  width.  The thumb's travel and the pointer mapping are unchanged; the trailing fill starts at
  the rail's start.  Horizontal and vertical sliders both honour it.

- `Slider::set_style` and `Slider::set_trailing_fill` restyle a live slider in place (each
  requests a redraw when it changes something), with `Slider::trailing_fill` to read the flag
  back; the consuming `with_style` / `with_trailing_fill` builders are unchanged.

- `LineBox::EmSnapped`: agg-sharp's `TextWidget` line box under `DefaultEnforceIntegerBounds = true`.
  A `Label` is `Floor(bottom)` to `Ceiling(top)` device pixels tall (one em, its bottom
  `LineBoxBottomInPixels` below the baseline) with its width `Ceiling`'d, and a `TextField` is that
  line plus its insets; the text sits on the box's baseline (`-Floor(bottom)` above its bottom).
  The rule is public as `text::snapped_line_box` / `snapped_text_width`. Opt-in; `Em` and
  `Standard` are unchanged.
- `TextField::with_enabled`, `with_enabled_cell`, `set_enabled` and the `Widget` enabled hooks: a
  disabled field takes neither focus nor input and draws its hint faded to alpha 50 of 255, as
  agg-sharp's disabled `ThemedTextEditWidget` does.
- `TextArea::with_enabled`, `with_enabled_cell`, `set_enabled` and the `Widget` enabled hooks, as
  `TextField`'s: a disabled area takes neither focus nor input and draws its hint faded to alpha 50
  of 255; an area an owner's cell disables while focused releases focus at the next layout.

- A widget hosted in a popup-menu row shows its `tooltip_text` on hover, as a text row shows
  its `MenuItem::tooltip` (the deepest tipped widget under the pointer wins).

- `MenuItem::keyboard_focusable` lets Up / Down reach a `widget_row`: the widget gets
  `Event::FocusGained` / `FocusLost` as the highlight arrives and leaves, and Left, Right, Enter
  and Space go to its `on_event` first (`MenuWidgetRow::keyboard_focus`).

- `MenuStyle` covers the rest of agg-sharp's `PopupMenu` chrome, all
  defaulting to the existing look: `separator: MenuSeparatorStyle`
  (`line_height`, `margin_y`, `inset_x`, `color`; agg-sharp's
  `CreateSeparator` is 1 / 1 / 8 in its theme's `RowBorder`),
  `submenu_arrow: SubmenuArrow` (`Chevron` or agg-sharp's `FilledTriangle`,
  with `paint::submenu_triangle_points` / `paint_submenu_arrow` /
  `paint_separator_styled`) and `window_margin` (gap kept from the window
  edge, `0.0` flush).  `MenuMetrics` gains `window_margin`;
  `geometry::metrics_with_style` and `PopupMenuState::set_separator_height` /
  `set_window_margin` carry the layout values.

- Finger drags decide what they are, as agg-sharp's `TouchPressDeferral`
  does: a drag past the tap tolerance pans the nearest `ScrollView` that can
  scroll along the drag (`touch_pan_travel()` reports the finger's travel
  while its middle press is dispatched); a drag no `ScrollView` takes is the
  widget's own left drag, pressed where the finger went down and ended with
  `Event::MouseCaptureLost` (new `EmuCmd::CancelPress`), so it never clicks.
  A press on a visible scroll bar is the bar's drag; a faded-out floating bar
  is panned over, and a floating bar shows while a finger pans
  (`ScrollbarAxis::touch_panning`, read through `scroll_bar_properties()`'s
  `fade_target`).
  A second finger landing during a widget's own drag (or after a dropped
  release) cancels that press.

- Popups close when the window deactivates, as agg-sharp's `PopupMenu` and
  `PopupWidget` close on `SystemWindow.Deactivated`: `PopupMenuState` closes
  on `Event::WindowDeactivated` and answers `MenuResponse::Closed` (so
  `MenuBar` drop-downs, text and markdown context menus and every other
  `PopupMenu` host that forwards events tear down as for any dismissal), as
  do `ComboBox`'s list and `ColorPicker`'s popup panel (keeping the working
  colour, as clicking away does). `Popup::on_window_deactivated()` closes a
  `Popup` controller for its host unless it is `IgnoreClicks` (C#'s
  `AlwaysKeepOpen`). Text suggestion lists stay open, as agg-sharp's do.
- Drag widgets end their drag on `Event::MouseCaptureLost` (the window
  deactivated mid-drag) where it was, as if released there: `Splitter`,
  `ScrollView` thumb and middle-button drags, `Window` title-bar moves and
  edge resizes (clearing snap guides), `Resize`, `DragValue` (a press that
  never became a drag does not enter edit mode), `TextField` and `TextArea`
  selection drags (a pending click-to-select-all is dropped), `MarkdownView`
  selection and code-block scroll-bar drags, and `ComboBox` list scroll-bar
  drags. Before, each kept dragging until its next `MouseUp`.
- Parent-first event preview: `Widget::preview_event(&mut self, &Event) ->
  EventResult` (default `Ignored`). Routed dispatch (`dispatch_event`,
  `dispatch_event_dyn`, and so every `App` pointer and key path) calls it on
  every ancestor of the target, root first, in the ancestor's local
  coordinates, before the target's `on_event` and the bubble. A container can
  react to a press anywhere inside it (select or raise itself) while the
  slider or text field under the pointer still gets the press, as agg-sharp's
  `MouseDown` fires on every widget under the pointer. A consuming preview
  intercepts the event: nothing below it previews and no `on_event` runs.
- An enabled state for `ToggleSwitch`: `ToggleSwitch::with_enabled_fn(|| ...)`
  (the `Button` / `ComboBox` convention). While the predicate is `false`,
  `is_enabled()` and `is_focusable()` are false, pointer and keyboard input
  are ignored (hover and press state are dropped), and the switch paints as
  agg-sharp's `SelectionControlStyle.DrawSwitch` does when disabled: bar and
  knob at `TOGGLE_DISABLED_OPACITY` (0.4) of their alpha, no hover tint, no
  press ripple. `ToggleSwitchStyle::disabled_color` selects MatterCAD's
  `RoundedToggleSwitch` disabled look instead: a 1 px bar outline and a filled
  knob, both in that colour.
- An enabled state for `ComboBox`: `ComboBox::with_enabled_fn(|| ...)` (the
  same live-predicate convention as `Button` and `SegmentedControl`). While
  the predicate is `false`, `is_enabled()` and `is_focusable()` are false, the
  combo refuses to open, drops pointer and keyboard input, holds no modal, and
  closes its list if it was open. It paints as agg-sharp's `DropDownList`
  does: outline at alpha 30, closed label and arrow at alpha 50, fill
  unchanged, overridable with `ComboBox::with_disabled_style(
  ComboBoxDisabledStyle { border_alpha, label_alpha })`.
- Window activation (agg-sharp's `SystemWindow.Deactivated`):
  `App::on_window_deactivated` / `App::on_window_activated` send
  `Event::WindowDeactivated` / `Event::WindowActivated` to every widget, once
  per change, and `agg_gui::window_is_active()` (also
  `App::window_is_active()`) reports the state. Deactivation leaves keyboard
  focus alone (no `FocusLost`). A widget holding pointer capture first gets
  the new `Event::MouseCaptureLost` and the capture is cleared: the release of
  its press goes to the app the user switched to, so before this the drag kept
  following the pointer, button up, until the next click. No `MouseUp` is made
  up, so a pressed button does not click. `Slider` ends such a drag with its
  one `on_release`, as it does for focus loss. Shells forward
  `ForwarderEvent::WindowActivated` / `WindowDeactivated` (which also clears
  the forwarder's held buttons): `agg-gui-shell` from winit's
  `WindowEvent::Focused`, `agg-gui-web-shell` from the page's `focus` / `blur`.
  Both are platform input, dropped while an automation run has real input
  off. `HeadlessWindow::on_deactivated` / `on_activated` simulate them. Code
  matching `Event` or `ForwarderEvent` exhaustively needs arms for the new
  variants.

- Caret suggestions for `TextField` (agg-sharp's `TextSuggestionController`):
  `TextField::with_text_suggestions(TextSuggestionController::new(provider))`
  shows a provider's `TextSuggestionList` below the word being typed. Up /
  Down move the highlight (wrapping), PageUp / PageDown page it, Enter or Tab
  accept (replacing the list's span as one undo step), Escape closes, a click
  on a row accepts, the wheel scrolls the rows, Ctrl+Space asks on demand;
  focus never leaves the field. `with_font`, `with_background_color` and
  `with_highlight_color` set the list's look (defaults: the field's font,
  `Visuals::bg_color`, the accent at alpha 128). Offsets are UTF-8 bytes.
  Also `TextField::replace_range` (one undo step; none for a no-op; skips
  read-only fields; normalizes line endings and applies the char filter) and
  `TextField::line_bounds_at`, and `Widget::on_ancestor_scrolled` (default
  no-op), which a `ScrollView` delivers to its subtree when it scrolls while a
  suggestion list is open, so the list closes instead of floating.

- `clipboard::has_image()`: whether the clipboard holds a picture, without
  copying or decoding it (for a menu that enables "Paste picture" as it opens).
  macOS asks the pasteboard for TIFF and Windows for "PNG" / `CF_DIBV5`
  (`objc2-app-kit` / `clipboard-win`, the crates arboard already links, now
  direct optional dependencies of the `clipboard` feature); Linux has no
  arboard format query, so there it reads the picture. The in-process,
  simulated and browser clipboards answer from what they hold.

- `Color::to_rgba8` is public: the 8-bit conversion the software renderer
  blends with, so a GPU backend can quantise a colour the same way.

- `text::VerticalMetrics` and `Font::with_vertical_metrics`: an opt-in,
  per-font override of the ascent, descent, line gap and cap height (font
  units) a face reports. Everything that centres or baselines text with that
  font follows it (`ascender_px` / `descender_px` / `line_height_px`, so
  `TextMetrics` and every `DrawCtx::measure_text`: `Label`, `Button`,
  `TextField`, the line boxes and the ellipsized paths). Lets an app place a
  face where another renderer placed it (agg-sharp's Liberation Sans 1.07 SVG
  metrics on the 2.x outlines). `Font::vertical_metrics` reports the metrics
  in force and `Font::cap_height_px` the cap height (OS/2 `sCapHeight` when
  not overridden). `Font::ink_extent_px` is how far the ink can reach: the
  larger of the override and the face's own ascender and descender. `Label`
  reaches its clip and backbuffer that far, so an override tighter than the
  glyphs (an em-high span, whose descenders hang below its descent) never cuts
  them off. Fonts without an override are unchanged.

- `lcd_display_detection`: agg-sharp's `LcdDisplayDetection`, the policy that
  picks a default for LCD subpixel text from what the OS reports about the
  display (`LcdDisplayEnvironment`: font smoothing on, ClearType style, RGB
  stripes, local session, upright display). `is_subpixel_appropriate` decides
  from the facts; `is_subpixel_appropriate_for` asks an
  `LcdDisplayEnvironmentProvider`, and no provider or one that cannot read the
  display means grayscale. agg-sharp's 9 `LcdDisplayDetectionTests` are ported.

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

- `widget::visible_root_rect(viewport)` (also re-exported at the crate root):
  the on-screen part of root logical space — `(0, −lift, w, h)` while the
  on-screen keyboard lifts the tree, `(0, 0, w, h)` otherwise. Clamp or flip
  a root-space rect against it to keep it on screen. Also
  `widgets::popup::clamp_rect_in(rect, bounds)`, `clamp_rect` for arbitrary
  bounds (`clamp_rect` itself is unchanged).

### Changed

- `TOUCH_SCROLL_THRESHOLD` is 8 logical points (times `device_scale()`), not
  8 physical pixels, so finger jitter on a dense phone stays a tap.
- **Native clipboard requires an opt-in.** With the `clipboard` feature, the
  `clipboard` functions (and `screenshot::copy_rgba_to_clipboard`) now use a
  process-wide in-process clipboard (text or a picture, holding one clears
  the other) until the app calls `clipboard::use_system_clipboard()`, so a
  test binary that links the feature, often only through Cargo feature
  unification, no longer overwrites the developer's real clipboard.
  `agg-gui-shell`'s `run` opts in, so shell-based apps need no change; an app
  with its own event loop calls `use_system_clipboard()` at startup. After the
  opt-in one `arboard` connection is kept for the life of the process instead
  of being made and dropped on every call (on X11 that dropped connection
  stopped serving the copied text, and each drop could block);
  `clipboard::release_system_clipboard()` closes it at shutdown, handing the
  contents to the clipboard manager. `clipboard::simulate()` still takes
  precedence, builds without the feature still have no native clipboard, and
  the browser keeps its page-event bridge.

- A press outside an open popup menu closes it and goes no further, as native
  Mac and Windows menus behave: the widget under the press sees neither the
  press nor its release, and takes no focus from it. Menu-bar menus, the
  text-editing and markdown context menus and their submenus already worked
  this way; the `ComboBox` drop-down now does too (while open it is modal,
  `has_active_modal`, so the press that closes it and its release stay with
  the combo, and a right or middle press outside closes it as well). A press
  on the combo's own box still just toggles it closed. A `PopupMenu` host
  gets the same rule by claiming the pointer while the menu is open
  (`popup_local.rs` documents the contract). No API changes, but this is a
  behaviour break for apps that relied on the press that closes a drop-down
  also acting on the widget beneath: that now takes a second click. Pinned
  by `menu/widget/tests_conformance.rs` (agg-sharp
  `PopupMenuConformanceTests`, with
  `AnOutsidePressBothDismissesTheMenuAndReachesTheWidgetBeneath` converted).

- `logical_root_transform(ctx)` now also takes out the on-screen keyboard's
  lift, so it is the paint-time twin of `event_root_transform`. Layout,
  `current_mouse_world`, `PopupMenu` root coordinates and the overlay / popup
  request queues all share this unlifted root logical space, and `App`
  applies the lift once, at paint (to the tree and the queue drains alike).
  Root-space viewport clamps — tooltips, the `ComboBox` popup, the
  `ColorPicker` popup, modal `Window`s, `PopupMenu` and `Popup` — clamp
  against `visible_root_rect`, so they keep things on screen. At lift 0
  nothing changes.

- `Tween::tick` now requests its in-flight frames with the tag
  `"animation.tween"`, so the drained provenance trace and `draw_trace_log`
  name a running tween instead of an anonymous or `"untagged"` request.
  `request_layout` likewise now logs its `"animation.request_layout"` tag to
  `draw_trace_log` instead of `"untagged"`. Behaviour is otherwise unchanged.

### Fixed

- A `TextArea` laid out twice before it paints (a host that measures its field, then sizes it)
  keeps the invalidation the first layout made: the second layout used to clear it, so the cached
  bitmap kept showing the hint and typed text never drew.
- Removing the focused, hovered or pointer-captured widget (or an ancestor of
  it) from the tree no longer panics. `App` drops a stored focus, hover or
  capture path that no longer names a widget when it re-resolves its paths
  (after every layout and at each input entry point): focus clears (the
  dropped widget gets no `FocusLost`, and the on-screen keyboard and its lift
  let go), and hover and capture fall back to what the pointer is over, with
  no hover-clear sent to the parent in the dropped widget's place.
  `focused_widget_type_name`, `focused_is_text_input` and every other path
  lookup answer "no widget" for a stale path instead of indexing out of
  bounds.

- A modal dialog too tall to both clear the on-screen keyboard and keep its
  top on screen no longer drifts down every frame while a field inside it has
  focus. The modal viewport clamp pulled the dialog back on screen, the field
  dropped behind the keyboard, the keyboard lift rose to follow, and the two
  ratcheted against each other. While the lift exists to show a focused field
  inside a modal, that modal's vertical viewport clamp now stands down: the
  field being typed in stays visible above the keyboard, the dialog keeps its
  position, the lift settles within the slide, and the dialog's top may sit
  above the screen until the keyboard is dismissed. Other modals clamp as
  before.

- A `MenuBar` dropdown offers its rows' `MenuItem::tooltip`s wherever the bar
  sits. The row-tip hit test read the root pointer against the popup's
  layouts, but `MenuBar` runs its popup in the bar's local space, so a bar
  away from the root origin (a menu bar at the top of the window) never
  offered a tip. The pointer is now mapped through the inverse of the paint
  ctx's `logical_root_transform`, so the hit test lands in whatever space the
  host painted the menu in, with or without the keyboard lift or a scale.

- The `PerformanceView` sparkline plots longer frames higher, so spikes read
  as peaks (it mapped samples Y-down in the Y-up context), and its 16.7 ms
  budget line sits at the top of the strip while every frame is under budget.

- Popup menu keyboard navigation follows agg-sharp's `PopupMenu.OnKeyDown`:
  Right, Enter or Space on a submenu row opens it with no row highlighted, so
  the next Up / Down starts inside it (also after a hover opened it); Left
  closes only the deepest submenu and highlights its opener, and in a top-level
  menu leaves the highlight where it is.

- `DragValue::on_change` fires only when the value actually changes, as
  agg-sharp's `DragValue.Value` setter raises `ValueChanged`: a drag step that
  snaps back to the same value, or committing an edit whose text is unchanged
  or does not parse, reports nothing. A bound value cell changed by something
  else while the field is being edited replaces the stale edit text, and
  committing no longer writes the old text back over it (egui #8403).

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

- While the on-screen keyboard lifts the tree: the open `ComboBox` popup and
  widget-anchored tooltips no longer paint one lift too high; the
  `InspectorPanel` hover highlight lands on its widget; a nested modal
  `Window` snaps (and registers its snap target) without carrying the lift;
  `PopupMenu` row tooltips show for the hovered row; the `ColorPicker` popup
  places itself the same way when it opens as when it paints; and tooltips
  flip and clamp against the on-screen viewport rather than root space.

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

Older releases are in [CHANGELOG-ARCHIVE.md](CHANGELOG-ARCHIVE.md).
