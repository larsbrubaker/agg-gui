# Demo Page Reimplementation Plan

## Reference: egui Demo (egui.rs/#demo)

Reimplement our demo page to match the egui demo in layout, functionality, and depth. All work is original — implemented in our coordinate system and render model. No egui code is adopted. We study their behaviors and reproduce equivalent functionality in our system.

**Our source:** https://github.com/larsbrubaker/agg-gui/actions
Each demo window should link back to its corresponding source file in our repo, not egui's.

---

## Key Technical Policies

**Buffered Text Widget**: All text rendering throughout the demo (labels, buttons, text fields, code editor, tooltips, sidebar items — everything) must use our buffered text widget. The one exception is the existing Text tab, which currently uses a different text path; that tab should be converted into a floating demo window and migrated to the buffered text widget as part of this work.

**Default Font**: Arial is our default system font. The Settings panel must include a font selector so the user can change the active font at runtime. The font change should propagate to all rendered text immediately.

---

## Overall Layout

The egui demo uses a three-region layout that we will replicate:

- **Left sidebar** (~200px): Fixed panel with the heading "agg-gui Demo", an "Organize windows" button, and a scrollable checklist of all demo/test windows grouped by category. Each item is a checkbox that opens/closes its corresponding window.
- **Central canvas**: The main area where floating windows appear. Windows are draggable, resizable, closable, and constrained to the available canvas area.
- **Right sidebar**: A collapsible panel for settings, inspection, and memory debugging tools.

Font sizes to match: ~15px body, ~18px headings in sidebar, ~13px for widget labels inside demo windows. Default font is Arial; user-selectable via Settings.

---

## Sidebar Categories & Items

### Demos

Each item opens a floating window. Checkboxes in the sidebar control open/close state.

| # | Demo Name | Description | New? |
|---|-----------|-------------|------|
| 1 | 🔤 Widget Gallery | Showcase of every widget type: labels, buttons, checkboxes, radio buttons, sliders, drag values, text fields, color pickers, combo boxes, date pickers, toggle switches | NEW |
| 2 | 🖮 Code Editor | Syntax-highlighted multi-line text editor with language selector and theme customization | NEW |
| 3 | 📊 Code Example | Inline code display with syntax highlighting | NEW |
| 4 | 🎵 Dancing Strings | Animated procedural line art reacting to time | NEW |
| 5 | ✋ Drag and Drop | Multi-column drag-and-drop reordering of items | NEW |
| 6 | 🔤 Font Book | Browse all available glyphs/characters by Unicode category | NEW |
| 7 | 🖼 Frame Demo | Demonstrates frame/border styling options | NEW |
| 8 | 📦 Interactive Container | Nested interactive container behaviors | NEW |
| 9 | 🔲 Modals | Modal dialog overlays with backdrop | NEW |
| 10 | 🔳 Misc Demo Window | Collapsible sections demoing: text layout, colors, interaction, animation, plot, UI composition | NEW |
| 11 | 📱 Multi Touch | Multi-touch gesture recognition (pinch, rotate, translate) | NEW |
| 12 | ✏️ Paint Bezier | Interactive bezier curve editor with control points | NEW |
| 13 | 🎨 Painting | Freehand drawing canvas with stroke recording | NEW |
| 14 | 📐 Panels | Nested panel layout demo (left, right, top, bottom, central) | NEW |
| 15 | 🔒 Password | Password field with visibility toggle | NEW |
| 16 | 🖱️ Popups | Context menus and popup behaviors | NEW |
| 17 | 🌄 Scene | 2D scene with pan and zoom | NEW |
| 18 | 📸 Screenshot | Screen capture functionality demo | NEW |
| 19 | 📜 Scrolling | Vertical, horizontal, and bidirectional scroll areas with stick-to-bottom behavior | NEW |
| 20 | 🎚️ Sliders | All slider variants: integer, float, logarithmic, vertical, custom range, clamping | NEW |
| 21 | 📏 Strip Demo | Strip layout (fixed + remainder sizing for rows/columns) | NEW |
| 22 | 📋 Table Demo | Sortable, resizable, scrollable data tables with heterogeneous columns | NEW |
| 23 | ✏️ Text Edit | Single-line and multi-line text editing with selection, clipboard, and undo/redo | NEW |
| 24 | 🔤 Text Layout | Text wrapping, alignment, rich text, and mixed fonts | NEW |
| 25 | 🔀 Toggle Switch | Custom widget: animated iOS-style toggle | NEW |
| 26 | 💬 Tooltips | Hover tooltips with rich content, nested tooltips, tooltip positioning | NEW |
| 27 | ↩️ Undo/Redo | Undo/redo system demonstration | NEW |
| 28 | ⚙️ Window Options | Window configuration: resizable, collapsible, scroll, anchoring, auto-sizing | NEW |
| 29 | 🧊 3D Cube | Our existing 3D rotating cube demo (currently in demo) | CONVERT |
| 30 | 🔤 Text (Buffered) | Our existing Text tab converted to a floating demo window, migrated to buffered text widget | CONVERT |

Every demo egui ships, we ship. No omissions. If egui adds new demos in the future, we track and add them.

### Tests

| # | Test Name | Description | New? |
|---|-----------|-------------|------|
| 1 | Layout Test | Automated layout correctness verification | NEW |
| 2 | Highlighting | Text highlighting and selection rendering test | NEW |

### Built-in Windows (Right Sidebar / Top Menu)

| Window | Description |
|--------|-------------|
| Settings | Global UI settings: style, spacing, **font selector** (default: Arial), animation speed, debug options |
| Inspection | Widget inspection: hover any widget to see its ID, rect, and response |
| Memory | Runtime memory usage display and area reset |
| About | Version info and credits |

---

## Window System Behaviors to Implement

These are core behaviors we need to study in egui and implement equivalently in our system:

### Window Management
- **Drag to move**: Title bar drag repositions window. Must work with our coordinate system.
- **Resize from edges/corners**: 8-directional resize handles.
- **Close button**: X button in title bar. Syncs with sidebar checkbox state.
- **Collapse/minimize**: Click title bar to toggle between collapsed (title only) and expanded.
- **Auto-sizing**: Windows auto-fit content on first open, then respect manual resize.
- **Snapping**: Window edge snapping to canvas boundaries and to other window edges.
- **Constrain to canvas**: Windows cannot be dragged outside the available area.
- **Z-ordering**: Click to bring window to front. Proper paint order.
- **"Organize windows"**: Reset all window positions to a tiled default layout.
- **Scroll within windows**: Windows with overflow content get vertical scrollbars.

### Text Field Behaviors
- **Clipboard**: Ctrl+C/V/X with system clipboard integration.
- **Mouse selection**: Click-and-drag to select, double-click for word, triple-click for line.
- **Keyboard navigation**: Arrow keys, Home/End, Ctrl+arrows for word jump, Shift for selection extension.
- **Undo/Redo**: Ctrl+Z / Ctrl+Shift+Z within text fields.
- **Multi-line**: Enter to newline, Tab handling, vertical scrolling.
- **Syntax highlighting**: Tokenize and colorize code in the code editor.

### Interaction Model
- **Hover states**: Visual feedback on all interactive elements.
- **Focus management**: Tab to cycle focus, visible focus rings.
- **Drag values**: Click-and-drag on numeric fields to scrub values.
- **Tooltips**: Delayed tooltip appearance with rich content support.
- **Context menus**: Right-click menus with submenus.
- **Modal dialogs**: Overlay with backdrop, focus trapping.
- **Multi-touch**: Pinch-to-zoom and rotate gestures on touch devices.

---

## Implementation Phases

### Phase 1 — Layout Shell
- Implement the three-panel layout (left sidebar, canvas, right sidebar).
- Build the sidebar checklist system with checkbox → window open/close binding.
- Build the floating window system: drag, resize, close, collapse, z-order.
- Implement window snapping and canvas constraint.
- Add the "Organize windows" reset button.

### Phase 2 — Core Widget Library
- Widget Gallery: label, button, checkbox, radio, slider, drag value, combo box, color picker, toggle switch, progress bar, spinner, separator, hyperlink, image.
- Text input: single-line and multi-line with full clipboard, selection, and undo/redo.
- Password field with visibility toggle.
- Scrollable regions (vertical, horizontal, both).

### Phase 3 — Demo Windows (Batch 1)
- Widget Gallery window (uses all Phase 2 widgets).
- Sliders demo (all slider variants).
- Text Edit demo.
- Password demo.
- Toggle Switch demo.
- Code Editor with syntax highlighting.
- Tooltips demo.

### Phase 4 — Demo Windows (Batch 2)
- Drag and Drop (multi-column).
- Painting / freehand canvas.
- Paint Bezier (control-point editor).
- Dancing Strings (animated procedural art).
- Font Book (Unicode glyph browser).
- Scrolling demo.
- Table Demo (sortable, resizable columns).
- Strip Demo.

### Phase 5 — Demo Windows (Batch 3)
- Panels demo (nested layout).
- Window Options demo.
- Frame Demo.
- Interactive Container.
- Modals demo.
- Popups / Context Menus.
- Scene (2D pan/zoom).
- Screenshot demo.
- Code Example.
- Text Layout demo.
- Undo/Redo demo.
- Multi Touch demo.

### Phase 6 — Integration & Polish
- Convert the existing Text tab into a floating demo window using the buffered text widget.
- Integrate our existing 3D Cube as a demo window entry.
- Implement right sidebar (Settings with font selector, Inspection, Memory, About).
- Implement the font selector in Settings: default Arial, runtime-switchable, immediate propagation.
- Audit every text rendering path to confirm buffered text widget usage — no exceptions.
- Layout Test and Highlighting test windows.
- Match egui font sizes, spacing, and color scheme.
- Test all window interactions: snapping, resize, z-order, collapse.
- Performance pass: ensure smooth 60fps with many windows open.
- Touch/mobile responsiveness pass.
- Add source links in each demo window pointing to the corresponding file in https://github.com/larsbrubaker/agg-gui/actions.

---

## Technical Notes

- **No egui code is used.** We study their public demo and source for behavioral reference only, then implement equivalent functionality in our own coordinate system and render pipeline.
- **Our source**: All demo source links point to https://github.com/larsbrubaker/agg-gui/actions — each demo window should include a link to its own source file in this repo.
- **Buffered text widget everywhere.** Every piece of rendered text in the demo — sidebar labels, window titles, button text, text fields, code editors, tooltips, tables — uses our buffered text widget. No exceptions. The existing Text tab is converted to a floating window and migrated to the buffered text widget.
- **Default font is Arial.** The Settings panel provides a font selector dropdown to change the active font at runtime. Font changes propagate immediately to all text.
- **Each demo is self-contained.** A demo struct owns its state and renders into whatever window/panel it's given.
- **Sidebar state** is a flat map of `demo_name → bool` controlling which windows are open.
- **Window positions/sizes** are persisted in our existing state system so they survive across sessions.
- **Complete parity.** Every demo present in egui's demo must have a corresponding implementation in our system. This is not a subset — it is the full set, plus our own additions (3D Cube, Text).
- **egui behavioral reference**: `github.com/emilk/egui`, specifically `crates/egui_demo_lib/src/demo/` for all demo implementations and `crates/egui_demo_app/` for the app shell. Study for behavior, implement our own.
