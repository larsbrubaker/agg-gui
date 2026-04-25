# Egui Demo/Test Parity Audit

This file tracks the parity audit between `demo-ui` / `agg-gui` and the checked-in egui reference. The goal is to match egui demos/tests where we are incomplete, while preserving agg-gui-specific work that intentionally goes beyond egui.

## Status Legend

- `match`: close enough for the current framework surface.
- `gap`: egui has behavior, wording, layout, defaults, or edge cases we still need to port.
- `needs_test`: behavior exists but needs automated coverage or a stronger regression test.
- `ours_beyond_egui`: intentionally richer or different than egui.
- `not_applicable`: no direct egui equivalent or the feature is unsupported by design.

## Decision Rules

- Preserve agg-gui-specific demos and tests when they are intentionally beyond egui: `Lion`, `LCD Subpixel`, `System`, SVG rendering tests, inspector tooling, AGG rendering tests, and framework invariants.
- Improve our implementation when the egui reference has functionality we have not implemented or when our behavior is weaker.
- Keep Y-up coordinates, Font Awesome icons, and agg-gui widget architecture even when the egui source is Y-down or uses egui-only APIs.
- Add focused tests for behavior changes when feasible, especially event routing, popup geometry, drag/drop, scrolling, and resize.

## Demo Matrix

| Area | Our implementation | Egui reference | Status | Notes |
|---|---|---|---|---|
| Widget Gallery | `demo-ui/src/windows/gallery.rs` | `egui-reference/crates/egui_demo_lib/src/demo/widget_gallery.rs` | needs_test | Reworked toward egui's grid/doc-link gallery and added missing rows for link, selectable-label equivalent, color picker, image, image button, separator, and collapsing header. Radio/selectable/combo now share one selection state, and formerly dead links/buttons are wired. Remaining gap: visible/interactive/opacity scope controls and optional date widget. |
| Sliders | `demo-ui/src/windows/basic.rs` | `egui-reference/crates/egui_demo_lib/src/demo/sliders.rs` | gap | Egui has interactive min/max, logarithmic mode, clamping, smart aim, integer/float mode, vertical orientation, trailing fill, handle shape, and Assign PI. Our demo only shows four static ranges. |
| TextEdit | `demo-ui/src/windows/basic.rs` | `egui-reference/crates/egui_demo_lib/src/demo/text_edit.rs` | gap | Egui covers multiline editing, horizontal/vertical alignment, prefix/suffix atoms, selected text display, case toggle, and move-to-start/end. Our demo covers basic single-line fields. |
| Tooltips | `demo-ui/src/windows/basic.rs`; `agg-gui/src/widgets/tooltip.rs` | `egui-reference/crates/egui_demo_lib/src/demo/tooltips.rs` | needs_test | Global overlay and egui cases implemented; remaining gap is regression coverage for clipping, cursor placement, and scroll dismissal. |
| Popups | `demo-ui/src/windows/interaction.rs` | `egui-reference/crates/egui_demo_lib/src/demo/popups.rs` | needs_test | Floating overlay implemented; remaining gap is full egui wording/options audit and dismissal tests. |
| Modals | `demo-ui/src/windows/text_demos/dialogs.rs` | `egui-reference/crates/egui_demo_lib/src/demo/modals.rs` | needs_test | Reworked into stacked modal layers with backdrop, Escape/outside-click close, progress modal, and app-viewport overlay painting instead of window-local content. Remaining gap: editable user fields/role combo parity. |
| Misc Demos | `demo-ui/src/windows/misc/misc_demos.rs` | `egui-reference/crates/egui_demo_lib/src/demo/misc_demo_window.rs` | gap | Needs item-by-item comparison. |
| Code Editor | `demo-ui/src/windows/basic.rs` | `egui-reference/crates/egui_demo_lib/src/demo/code_editor.rs` | gap | Our demo is a simplified read-only source view. |
| Code Example | `demo-ui/src/windows/code_example.rs` | `egui-reference/crates/egui_demo_lib/src/demo/code_example.rs` | needs_test | Reworked closer to egui's code + live output pattern; name and age now both drive live output and debug-style output. Remaining gap: full CodeTheme behavior. |
| Font Book | `demo-ui/src/windows/font_book.rs` | `egui-reference/crates/egui_demo_lib/src/demo/font_book.rs` | needs_test | Improved with source link/search controls and functional filter/clear state. Remaining gaps: font-family selector, click-to-copy, and per-glyph tooltip details. |
| Frame | `demo-ui/src/windows/frame_demo.rs`; `demo-ui/src/windows/frame_demo/core.rs` | `egui-reference/crates/egui_demo_lib/src/demo/frame_demo.rs` | needs_test | Controls/defaults have been nudged toward egui; verify shadow/fill/stroke defaults with focused tests or screenshots. |
| Panels | `demo-ui/src/windows/interaction.rs` | `egui-reference/crates/egui_demo_lib/src/demo/panels.rs` | needs_test | Egui-like panel order and splitters implemented; add tests for panel order, side/bottom geometry, and resizable splitters. |
| Strip | `demo-ui/src/windows/text_demos/strip_table.rs` | `egui-reference/crates/egui_demo_lib/src/demo/strip_demo.rs` | gap | Needs layout/resize comparison. |
| Table | `demo-ui/src/windows/text_demos/strip_table.rs` | `egui-reference/crates/egui_demo_lib/src/demo/table_demo.rs` | gap | Needs sorting/striping/scroll behavior comparison if supported. |
| Scrolling | `demo-ui/src/windows/scrolling/` | `egui-reference/crates/egui_demo_lib/src/demo/scrolling.rs` | needs_test | Tab labels, Scroll-to bring-into-view behavior, and Stick-to-end timing now match egui more closely; remaining risk is Appearance/Many-lines/Large-canvas detailed control parity. |
| Window Options | `demo-ui/src/windows/text_demos/dialogs.rs` | `egui-reference/crates/egui_demo_lib/src/demo/window_options.rs` | gap | Needs close/collapse/resizable/settings comparison. |
| Text Layout | `demo-ui/src/windows/text_demos/text_layout.rs` | `egui-reference/crates/egui_demo_lib/src/demo/text_layout.rs` | gap | Needs wrapping, justification, alignment, font, and selectable text parity review. |
| Interactive Container | `demo-ui/src/windows/misc.rs` | `egui-reference/crates/egui_demo_lib/src/demo/interactive_container.rs` | gap | Needs hover/click/layering comparison. |
| Bézier Curve | `demo-ui/src/windows/animation.rs` | `egui-reference/crates/egui_demo_lib/src/demo/paint_bezier.rs` | gap | Needs controls and paint output comparison. |
| Dancing Strings | `demo-ui/src/windows/animation.rs` | `egui-reference/crates/egui_demo_lib/src/demo/dancing_strings.rs` | gap | Needs animation timing and text path comparison. |
| Painting | `demo-ui/src/windows/animation.rs` | `egui-reference/crates/egui_demo_lib/src/demo/painting.rs` | gap | Needs stroke interaction and clearing behavior comparison. |
| Rendering Test | `demo-ui/src/rendering_test.rs`; `demo-ui/src/rendering_test/blending.rs`; `demo-ui/src/rendering_test/color.rs` | `egui-reference/crates/egui_demo_lib/src/rendering_test.rs` | ours_beyond_egui | Added egui-style ColorTest coverage while preserving agg-gui pixel/blending diagnostics. |
| Lion | `demo-ui/src/windows/lion.rs` | none direct | ours_beyond_egui | AGG/tessellation showcase. |
| Screenshot | `demo-ui/src/windows/screenshot_demo.rs` | `egui-reference/crates/egui_demo_lib/src/demo/screenshot.rs` | needs_test | Updated closer to egui capture flow; needs host-level tests for capture/export behavior. |
| Highlighting | `demo-ui/src/windows/misc.rs` | `egui-reference/crates/egui_demo_lib/src/demo/highlighting.rs` | gap | Needs syntax/highlight examples audit. |
| 3D Animation | `demo-ui/src/windows.rs`; GL demo integration | none direct | ours_beyond_egui | agg-gui showcase. |
| System | `demo-ui/src/windows/system.rs` | none direct | ours_beyond_egui | Process-wide font/LCD settings are agg-gui-specific. |
| LCD Subpixel | `demo-ui/src/windows/truetype_lcd.rs` | none direct | ours_beyond_egui | AGG/C++ parity rather than egui parity. |
| Drag and Drop | `demo-ui/src/windows/interaction/drag_and_drop.rs` | `egui-reference/crates/egui_demo_lib/src/demo/drag_and_drop.rs` | needs_test | Repaint/cursor, source link, and visible-list insertion improved; add tests for same-column reorder and cross-column move. |
| Multi Touch | `demo-ui/src/windows/text_demos/multi_touch.rs` | `egui-reference/crates/egui_demo_lib/src/demo/multi_touch.rs` | needs_test | Updated closer to egui status/gesture presentation; needs multi-touch input simulation coverage. |
| Undo Redo | `demo-ui/src/windows/text_demos/dialogs.rs` | `egui-reference/crates/egui_demo_lib/src/demo/undo_redo.rs` | needs_test | Added shared UndoBuffer-style checkbox demo alongside text edit history; needs tests for undo/redo button behavior. |
| Scene | `demo-ui/src/windows/interaction.rs` | `egui-reference/crates/egui_demo_lib/src/demo/scene.rs` | gap | Needs interaction and paint parity audit. |
| Extra Viewport | `demo-ui/src/windows/misc.rs` | `egui-reference/crates/egui_demo_lib/src/demo/extra_viewport.rs` | not_applicable | Current platform does not support extra viewports; keep explanatory stub unless support is added. |

## Test Matrix

| Area | Our implementation | Egui reference | Status | Notes |
|---|---|---|---|---|
| Clipboard Test | `demo-ui/src/windows/tests/basic/controls.rs` | `egui-reference/crates/egui_demo_lib/src/demo/tests/` | gap | Needs exact reference mapping and behavior audit. |
| Cursor Test | `demo-ui/src/windows/tests/basic/controls.rs` | `egui-reference/crates/egui_demo_lib/src/demo/tests/` | gap | Verify cursor list and hover behavior. |
| Grid Test | `demo-ui/src/windows/tests/basic/controls.rs` | `egui-reference/crates/egui_demo_lib/src/demo/tests/` | gap | Needs grid behavior comparison. |
| Id Test | `demo-ui/src/windows/tests/basic/controls.rs` | `egui-reference/crates/egui_demo_lib/src/demo/tests/` | gap | Needs id/conflict behavior comparison. |
| Input Event History | `demo-ui/src/windows/tests/basic/controls.rs` | `egui-reference/crates/egui_demo_lib/src/demo/tests/` | gap | Needs event list and formatting audit. |
| Input Test | `demo-ui/src/windows/tests/basic/layout.rs` | `egui-reference/crates/egui_demo_lib/src/demo/tests/` | gap | Needs pointer/keyboard state comparison. |
| Layout Test | `demo-ui/src/windows/tests/basic/layout.rs` | `egui-reference/crates/egui_demo_lib/src/demo/tests/` | gap | Needs layout examples comparison. |
| Manual Layout Test | `demo-ui/src/windows/tests/basic/layout.rs` | `egui-reference/crates/egui_demo_lib/src/demo/tests/` | gap | Needs manual placement parity audit. |
| SVG Test | `demo-ui/src/windows/tests/svg.rs`; `demo-ui/src/windows/tests/svg/` | none direct | ours_beyond_egui | Intentionally divergent: SVG renderer coverage is agg-gui-specific and should not be forced toward egui's tessellation diagnostics. |
| Window Resize Test | `demo-ui/src/windows/tests/resize.rs`; `demo-ui/tests/window_resize*.rs` | `egui-reference/crates/egui_demo_lib/src/demo/tests/window_resize_test.rs` | match | Audited against the egui six-window source; 38 production-widget integration tests cover auto-size, scroll wrapping, embedded scroll, tight/floor fit, TextArea fill, and free resize behavior. |
| Tessellation Test | `demo-ui/src/rendering_test.rs`; `demo-ui/src/windows/lion.rs`; `demo-ui/src/windows/tests/svg.rs` | `egui-reference/crates/egui_demo_lib/src/demo/tests/tessellation_test.rs` | ours_beyond_egui | Intentionally divergent: agg-gui validates tessellation through AGG/GL rendering, Lion, and SVG diagnostics rather than egui's `RectShape` tessellation UI. |
| Library tests | `agg-gui/src/tests/**`; `agg-gui/tests/file_line_count.rs` | none direct | ours_beyond_egui | Keep as framework/product invariants, not egui ports. |

## Current Priority Backlog

1. Add production-widget tests for recently touched high-interaction behavior:
   - ComboBox popup direction, capped height, wheel scrolling, and opaque global overlay. Direction and wheel scrolling now have focused tests.
   - Tooltip global overlay escaping clips and closing on scroll.
   - Drag and Drop same-column reorder and cross-column move.
   - Panels layout order and splitter resizing.
2. Audit `demo-ui/src/windows/scrolling/` against egui `scrolling.rs`; this has the broadest remaining parity risk.
3. Audit `demo-ui/src/windows/tests/resize.rs` and `demo-ui/tests/window_resize*` against current egui `window_resize_test.rs`.
4. Work through the remaining `gap` rows in demo registry order.

## Intentional Divergences

- `Lion` is an AGG/tessellation showcase and should not be reduced to an egui demo shape.
- `LCD Subpixel` and `System` expose process-wide font, LCD, hinting, gamma, and bundled font controls that are specific to agg-gui’s renderer.
- `Rendering Test` should keep agg-gui pixel/blending coverage; egui rendering cases can be added only when they reveal missing behavior.
- SVG tests and demos are renderer/product coverage, not egui ports; this also covers the tessellation diagnostics area in an agg-gui-specific way.
- Inspector behavior and file-line-count tests are project tooling and should stay independent of egui.
- `Extra Viewport` is intentionally a support-status stub until the platform layer supports extra native/web viewports.
- Y-up geometry and Font Awesome icon choices are project constraints even when egui source examples assume Y-down or use different iconography.

## Verification Log

- `cargo test -p agg-gui test_combo_popup -- --nocapture` passed after adding ComboBox popup direction and wheel-direction regressions.
- `cargo test -p demo-ui --test window_resize -- --nocapture` passed: 38 resize parity tests.
- `cargo check -p demo-ui` passed after the audit changes.
- `cargo test -p demo-ui scrolling::scroll_to -- --nocapture` passed after adding `BringIntoView` scroll-to behavior.
- `cargo test -p demo-ui -- --nocapture` passed: demo-ui unit/integration tests, including new modal, drag/drop, strip, screenshot, panel, rendering color, and scroll-to tests.
- `cargo test -p agg-gui --lib -- --nocapture` passed: 151 library tests.
- `cargo test -p agg-gui --test file_line_count -- --nocapture` passed.
- `cargo test -p demo-ui modal_ -- --nocapture` passed after moving modal drawing to the app-level viewport.
- `cargo test -p demo-ui code_example -- --nocapture` passed after wiring Code Example name/age shared state.
- `cargo test -p agg-gui test_text_field_tracks_external_text_cell -- --nocapture` passed after adding external text binding for filter/clear parity.
- `cargo check -p demo-ui` and `cargo test -p demo-ui -- --nocapture` passed after the second-pass gallery, Font Book, Code Example, and modal fixes.
