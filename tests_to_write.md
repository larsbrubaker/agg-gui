# Tests to write — regression + missing coverage

Failing-test-first backlog.  Each entry = bug we fixed without a regression
test, OR behavior worth asserting.  Target crate in parens.

## Framework — layout primitives

1. **`SizedBox::new().with_width(8.0)` returns height 0, not `available.height`.** (agg-gui)
   - Previously inflated row/column height to full available, pushing siblings
     off-screen.  `layout(Size::new(100.0, 400.0))` must return `Size(8.0, 0.0)`.
2. **`SizedBox::new().with_width(W).with_child(X)` returns child's natural height.** (agg-gui)
   - Assert with a fixed-height child widget.
3. **`FlexColumn` with one flex(1.0) child + fixed siblings gives flex child the remainder.** (agg-gui)
   - Sum of fixed heights + gaps < inner_h.  Flex child gets `inner_h - fixed - gaps`.
4. **`FlexColumn` when `sum(fixed) > inner_h` does not make flex child height negative.** (agg-gui)
   - Clamp to 0.
5. **`Button::layout` returns fit width = `label_w + font_size*1.4`, not `available.width`.** (agg-gui)

## Framework — text centering

6. **`Label` baseline Y = `h/2 - (ascent - descent)/2`.** (agg-gui)
   - Not `... + descent`.  Measure by rendering "Ay" at known metrics and
     asserting baseline pixel position.
7. Same formula in `DragValue`, `Hyperlink`, `Markdown`, `ProgressBar`,
   `TextField` paint — assert by mocking `DrawCtx` and capturing the
   `fill_text(text, x, y)` calls.

## Framework — scroll

8. **`ScrollView::with_style` marks `style_explicit=true`; subsequent layout
   does NOT read `current_scroll_style()`.** (agg-gui)
9. **`ScrollView` with no explicit style reads `current_scroll_style()` every layout.** (agg-gui)
10. **`set_scroll_visibility(AlwaysVisible)` causes every unbound ScrollView to
    paint its bar even without hover.** (agg-gui)
11. **`ScrollBarKind::Floating` + `VisibleWhenNeeded` = bar hidden unless
    `hovered_bar || dragging`.** (agg-gui)
12. **`ScrollBarKind::Solid` + `VisibleWhenNeeded` = bar always drawn when
    content overflows.** (agg-gui)
13. **Horizontal scroll thumb width = `track_w * (viewport/content)`, min
    `handle_min_length`.** (agg-gui)
14. **Track-click above thumb pages up by `viewport - 16`, not snap-to-cursor.** (agg-gui)
15. **Track-click below thumb pages down by `viewport - 16`.** (agg-gui)
16. **Wheel while `stick_to_bottom=true` detaches; returning to bottom re-attaches.** (agg-gui)
17. **`ScrollView.layout` with `horizontal=true` passes `f64::MAX/2` as child
    available_w.** (agg-gui)
    - And: natural canvas widgets must NOT `.max(available.width)` that value.
      Add test + a doc lint or a helper `canvas_width(available) -> f64` that
      handles this correctly.

## Framework — Window widget

18. **`Window::layout` does NOT mutate `bounds.x/y` via `clamp_to_canvas`.** (agg-gui)
    - Set bounds at x=1500, layout with canvas 800 wide, x stays 1500.
19. **Window drag-release DOES clamp.** (agg-gui)
    - Simulate MouseDown on title bar → MouseMove to x=-9999 → MouseUp.
      Assert bounds.x clamped to 0.
20. **`Window::with_position_cell` cell is updated each layout, reflecting
    current (possibly off-canvas) bounds.** (agg-gui)

## Framework — event dispatch

21. **`dispatch_event` returns `Ignored` when path index out of bounds (stale
    path after tree change).** (agg-gui)
    - Build tree with CollapsingHeader open, capture path to child, collapse
      header (drops child), dispatch event along old path — no panic.
22. **`claims_pointer_exclusively` short-circuits `hit_test_subtree` —
    children not visited.** (agg-gui)
23. **`show_in_inspector() = false` excludes entire subtree from
    `collect_inspector_nodes` output.** (agg-gui)

## Framework — MouseWheel

24. **`Event::MouseWheel` has `delta_x` and `delta_y` fields.** (agg-gui)
25. **`App::on_mouse_wheel_xy` delivers both axes; `on_mouse_wheel` sets `delta_x = 0`.** (agg-gui)

## Framework — SegRow / Button fit

26. **SegRow (in demo-ui `helpers`) natural_width = `per_button * n + gap*(n-1)`,
    capped at `available.width`.** (demo-ui)

## Framework — theme

27. **`current_visuals()` returns what was last `set_visuals(...)` on the
    same thread.** (agg-gui)
28. **Every widget with hardcoded colours in paint is theme-aware — regression
    test that switches to dark theme and asserts distinct output.** (agg-gui)

## Framework — inspector

29. **`InspectorPanel::saved_state()` → `apply_saved_state()` round-trip
    restores expand bits + selected + props_h.** (agg-gui)
30. **`pending_expanded/pending_selected` apply on FIRST layout after
    `apply_saved_state`, not on the second.** (agg-gui)
31. **Every `InspectorNode.properties` contains a `("backbuffer", ...)` entry
    as first element.** (agg-gui)

## Framework — screenshot / image_view

32. **`ScreenshotHandle::take()` sets `request` flag.** (agg-gui)
33. **`ImageView` placeholder text shown when source is `None`.** (agg-gui)
34. **`ImageView` paints image via `draw_image_rgba` when source is `Some`.** (agg-gui)
    - Mock DrawCtx, assert `draw_image_rgba` called with expected dst rect.
35. **GL `draw_image_rgba` correctly Y-flips uv so top-down data renders right-way-up.** (demo-gl)
    - Integration test: upload 2x2 known pattern, read back after render,
      compare.

## Framework — app_state

36. **`OsWindowState::serialize` / `deserialize` round-trip.** (agg-gui)
37. **`OsWindowState::deserialize` accepts legacy 3-field form (no maximized).** (agg-gui)

## Demo-ui — SavedState

38. **`SavedState::serialize` / `deserialize` round-trip through all current
    fields including `inspector` + `backend_open` + `window_maximized`.** (demo-ui)
39. **`SavedState::deserialize` of an older file without `inspector=` /
    `backend=` / `window=` lines parses with sensible defaults.** (demo-ui)
40. **`StateAccessor::current_state` reflects mutations of its cells.** (demo-ui)

## Demo — scroll-appearance

41. **Dragging any Details slider in Appearance tab updates global scroll
    style (`current_scroll_style`).** (demo-ui integration)
42. **Preset buttons write their respective `ScrollBarStyle::solid/thin/floating`
    to global.** (demo-ui integration)
43. **`LoremCanvas` (Bidirectional) reports content_width = text_w + 2*pad,
    NOT `f64::MAX/2`.** (demo-ui)
44. **`VirtualCanvas` (Large canvas) reports content_width = `CONTENT_WIDTH`
    constant, not inflated.** (demo-ui)

## Native harness

45. **`Window::layout`'s `clamp_to_canvas` removal means saved position
    (x=1500) survives a startup render at smaller transient canvas.** (integration)
46. **Init clear uses theme `bg_color`, not `(0.1, 0.1, 0.1)`.** (demo-gl)
47. **Window created hidden; `set_visible(true)` not called until after first
    render.** (demo-native)  — code-grep test, not runtime.

## Auto-save

48. **`serialize_state` substitutes `last_windowed_*` when fullscreen OR maximized.** (demo-native)
49. **Save-on-change only writes when `mouse_buttons_down == 0`.** (demo-native)
50. **State hash comparison skips write when unchanged.** (demo-native)

## WASM

51. **`needs_repaint()` = true after any `on_*` handler, cleared by `render()`.** (demo-wasm)
52. **`needs_repaint()` = true while `cube_visible.get()` is true.** (demo-wasm)
53. **`needs_repaint()` = true while `app.has_focus()` is true.** (demo-wasm)

## Approach notes

- Pure logic (layout math, serialize, clamping) → plain `#[test]` in the same
  file.  Fast.
- Paint assertions → implement a `MockDrawCtx` that records calls into a
  `Vec<DrawCall>`; assert structural shape.  One helper per crate.
- GL integration (texture cache, screenshot) → headless GL via `glow` +
  off-screen framebuffer; slow, keep in `tests/` not inline.

## Policy going forward

- New bug report → write failing `#[test]` first, commit it red, then fix.
- New feature → at least one happy-path unit test + one edge-case test before merge.
- Rename / extract refactors → no test required (compiler checks).
