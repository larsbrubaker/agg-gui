# Demo Page Reimplementation Plan

## Reference: egui Demo (local reference copy)

Reimplement our demo page to match the **exact** layout, demos, menus, dark/light modes, rows, examples, and tests of the egui demo. The authoritative reference is our local copy at `agg-gui/agg-gui/reference-egui-main`. All work is original — implemented with our own widgets, windowing system, coordinate system, and render pipeline. No egui code is adopted. We study their source and behaviors at the file level and reproduce equivalent functionality in our system.

**Key reference files** (inside `reference-egui-main`):
- `crates/egui_demo_lib/src/demo/` — all demo implementations
- `crates/egui_demo_lib/src/demo/demo_app_windows.rs` — sidebar, window registration, `DemoGroups::default()`
- `crates/egui_demo_lib/src/demo/mod.rs` — `Demo` / `View` traits, module declarations
- `crates/egui_demo_app/src/wrap_app.rs` — app shell, top bar, `Anchor` enum, app tab switching

**Our source:** https://github.com/larsbrubaker/agg-gui
Each demo window should link back to its corresponding source file in our repo, not egui's.

---

## Key Technical Policies

**Buffered Text Widget — The Only Text Path**: All text rendering uses back-buffered text widgets. There is no other text caching layer, no separate glyph cache, no ad-hoc draw-text call. The back-buffered text widget *is* the caching mechanism — it rasterizes its content into a back buffer and blits that buffer during paint. Every piece of visible text in the entire application (labels, buttons, text fields, code editor, tooltips, sidebar items, window titles, menu entries, table cells, Markdown content — everything) is a back-buffered text widget instance in the widget tree. The one exception is the existing Text tab, which currently uses a different text path; that tab must be converted into a floating demo window and migrated to the back-buffered text widget as part of this work.

**Widget Composition — No Monolithic Widgets**: All widgets are built through composition of child widgets. A button is a container that holds a back-buffered text widget (and optionally an icon widget). A checkbox is a layout of an indicator widget and a back-buffered text widget. A slider is a track widget, a thumb widget, and a back-buffered text widget for the value label. There are no widgets that internally call a raw "draw text" function — if a widget needs to display text, it composes a back-buffered text widget as a child. This applies at every level: demo windows are compositions of layout widgets containing interactive widgets containing text widgets. The widget tree is the only structure; there is no parallel rendering layer.

**Default Font**: Arial is our default system font. The Backend panel must include a font selector so the user can change the active font at runtime. The font change should propagate to all rendered text immediately (every back-buffered text widget re-rasterizes on font change).

**Markdown Widget**: A reusable, library-level widget (`widgets/markdown.rs`) that uses [pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark) to parse Markdown and render it into native agg-gui controls. This widget is part of the core `agg-gui` crate, usable anywhere a widget can be placed — windows, dialogs, panels, scroll views, etc. All text rendered through our buffered text widget. The `pulldown-cmark` dependency is added to `agg-gui/Cargo.toml`.

**Icons**: All icons throughout the UI use [Google Material Icons](https://fonts.google.com/icons) and [Bootstrap Icons](https://icons.getbootstrap.com/) rendered as font glyphs. Where egui uses emoji characters for sidebar items, window titles, menu entries, buttons, and the theme switch, we substitute the appropriate Google Material or Bootstrap icon glyph. Both icon fonts are bundled with the application and loaded alongside Arial. Icon selection should prefer Google Material Icons for standard UI concepts (settings, close, menu, light/dark mode, zoom, etc.) and Bootstrap Icons where Material does not cover the need. Icons must respond to the active theme (dark/light) like all other text.

---

## Overall Layout

The egui demo uses a four-region layout that we will replicate exactly. Reference: `desktop_ui()` in `demo_app_windows.rs` and `WrapApp::ui()` in `wrap_app.rs`.

- **Top bar**: Contains a dark/light/system theme preference switch, a "Backend" toggle button, and selectable app tabs (e.g. "Demos", "3D Cube", "Rendering test"). Inside the Demos view, a File menu appears with Organize Windows, Reset Memory, and zoom controls (native only).
- **Right panel** (~160px, non-resizable): Logo/heading ("agg-gui Demo"), About checkbox, then a scrollable checklist of all demo and test windows grouped by category ("Demos" and "Tests" sections). "Organize windows" button at the bottom. Reference: `Panel::right("egui_demo_panel")`.
- **Left panel** (collapsible "Backend"): Backend/debug panel with run mode, zoom controls, debug settings, frame history, font selector (default: Arial, runtime-switchable with immediate propagation), and memory/inspection tools.
- **Central canvas**: The main area where floating demo windows appear. Windows are draggable, resizable, closable, and constrained to the available canvas area.

Font sizes to match: ~15px body, ~18px headings in sidebar, ~13px for widget labels inside demo windows. Default font is Arial; user-selectable via Backend panel.

---

## Dark / Light Mode

The top bar includes a three-way theme preference switch matching egui's `global_theme_preference_switch`:

- **Dark** / **Light** / **Follow system** toggle
- All widgets, windows, backgrounds, text, and borders must respond to theme changes immediately
- Match egui's exact color scheme for both dark and light modes (study `Visuals::dark()` and `Visuals::light()` in the reference source)
- Theme preference is persisted across sessions

---

## Top-Level App Tabs

The app shell has two selectable tabs in the top bar:

| Tab | Description |
|-----|-------------|
| Demos | The main demo gallery with right sidebar checklist and floating windows (primary focus) |
| Rendering test | Rendering validation surface for color/gradient/shape correctness |

Default selection is "Demos" on startup.

**Note:** Unlike egui, we do not have a separate "3D Cube" top-level tab. Our GL cube is implemented as a floating demo window inside the Demos canvas (egui lacks this capability since it has no GL widget). The 3D Cube entry in the sidebar checklist opens it as a normal window.

---

## Sidebar Categories & Items

### Demos

Each item opens a floating window. Checkboxes in the right sidebar control open/close state. Order matches egui's `DemoGroups::default()` registration order exactly.

| # | Demo Name | Description | Status |
|---|-----------|-------------|--------|
| 1 | Paint Bezier | Interactive bezier curve editor with control points | NEW |
| 2 | Code Editor | Syntax-highlighted multi-line text editor with language selector and theme customization | NEW |
| 3 | Code Example | Inline code display with syntax highlighting | NEW |
| 4 | Dancing Strings | Animated procedural line art reacting to time | NEW |
| 5 | Drag and Drop | Multi-column drag-and-drop reordering of items | NEW |
| 6 | Extra Viewport | Additional viewport / window creation demo | NEW |
| 7 | Font Book | Browse all available glyphs/characters by Unicode category | NEW |
| 8 | Frame Demo | Demonstrates frame/border styling options | NEW |
| 9 | Highlighting | Text highlighting and selection rendering | NEW |
| 10 | Interactive Container | Nested interactive container behaviors | NEW |
| 11 | Misc Demo Window | Collapsible sections demoing: text layout, colors, interaction, animation, password, UI composition | NEW |
| 12 | Modals | Modal dialog overlays with backdrop | NEW |
| 13 | Multi Touch | Multi-touch gesture recognition (pinch, rotate, translate) | NEW |
| 14 | Painting | Freehand drawing canvas with stroke recording | NEW |
| 15 | Panels | Nested panel layout demo (left, right, top, bottom, central) | NEW |
| 16 | Popups | Context menus and popup behaviors | NEW |
| 17 | Scene | 2D scene with pan and zoom | NEW |
| 18 | Screenshot | Screen capture functionality demo | NEW |
| 19 | Scrolling | Vertical, horizontal, and bidirectional scroll areas with stick-to-bottom behavior | NEW |
| 20 | Sliders | All slider variants: integer, float, logarithmic, vertical, custom range, clamping | NEW |
| 21 | Strip Demo | Strip layout (fixed + remainder sizing for rows/columns) | NEW |
| 22 | Table Demo | Sortable, resizable, scrollable data tables with heterogeneous columns | NEW |
| 23 | Text Edit | Single-line and multi-line text editing with selection, clipboard, and undo/redo | NEW |
| 24 | Text Layout | Text wrapping, alignment, rich text, and mixed fonts | NEW |
| 25 | Tooltips | Hover tooltips with rich content, nested tooltips, tooltip positioning | NEW |
| 26 | Undo/Redo | Undo/redo system demonstration | NEW |
| 27 | Widget Gallery | Showcase of every widget type: labels, buttons, checkboxes, radio buttons, sliders, drag values, text fields, color pickers, combo boxes, toggle switches | NEW |
| 28 | Window Options | Window configuration: resizable, collapsible, scroll, anchoring, auto-sizing | NEW |

**Note:** Password and Toggle Switch are **helper modules** used by Misc Demo Window and Widget Gallery respectively — they are not standalone sidebar entries (matching egui's structure).

Every demo egui ships, we ship. No omissions. If egui adds new demos in the future, we track and add them.

### Tests

Each test opens a floating window. Checkboxes in the right sidebar control open/close state. Matches egui's `DemoGroups::default()` test registration exactly.

| # | Test Name | Description | Status |
|---|-----------|-------------|--------|
| 1 | Clipboard Test | Clipboard read/write correctness verification | NEW |
| 2 | Cursor Test | Cursor shape and positioning test | NEW |
| 3 | Grid Test | Grid layout correctness verification | NEW |
| 4 | Id Test | Widget ID uniqueness and stability test | NEW |
| 5 | Input Event History | Input event recording and replay display | NEW |
| 6 | Input Test | Keyboard/mouse input handling test | NEW |
| 7 | Layout Test | Automated layout correctness verification | NEW |
| 8 | Manual Layout Test | Manual/absolute positioning layout test | NEW |
| 9 | Vector Rendering Test | Vector/path rendering correctness (adapted from egui's SVG test) | NEW |
| 10 | Tessellation Test | Tessellation correctness for shape rendering | NEW |
| 11 | Window Resize Test | Window resize behavior and constraint test | NEW |

### About Window

The About entry is a toggle checkbox at the top of the right sidebar checklist (above the Demos section), matching egui's layout. It opens as a floating window.

- Uses the **Markdown widget** to render our `README.md` from the GitHub repo (https://github.com/larsbrubaker/agg-gui)
- This ensures the About window always reflects the current project description, features, and usage
- For offline/fallback, a copy of `README.md` is bundled at build time (via `include_str!` or similar) so the About window works without network access
- Default open on first launch (matching egui's behavior)

### Backend Panel (Left, Collapsible)

Toggled via the "Backend" button in the top bar. Contains:

| Section | Description |
|---------|-------------|
| Run Mode | Continuous vs reactive repaint mode |
| Zoom | UI zoom controls (native only) |
| Font Selector | Default Arial, runtime-switchable, immediate propagation to all text |
| Debug | Debug painting, widget inspection overlay |
| Frame History | Frame timing and performance graph |
| Memory | Runtime memory usage display and area reset |

### File Menu (Demos View)

Appears in the top menu bar when the "Demos" app tab is selected:

| Item | Shortcut | Description |
|------|----------|-------------|
| Zoom controls | — | Zoom in/out/reset (native only) |
| Organize Windows | Ctrl+Shift+O | Reset all window positions to tiled default layout |
| Reset Memory | Ctrl+Shift+R | Clear all persisted UI state |

---

## Markdown Widget

A core library widget for rendering Markdown content into native agg-gui controls.

**Location:** `agg-gui/src/widgets/markdown.rs` — part of the `agg-gui` crate, not demo-only.

**Dependency:** `pulldown-cmark` added to `agg-gui/Cargo.toml`.

**Supported Markdown elements:**
- Headings (h1–h6) with appropriate font sizes and weight
- Paragraphs with word wrapping
- **Bold**, *italic*, and ***bold-italic*** inline styles
- `Inline code` with monospace font and background highlight
- Code blocks with syntax highlighting (reuse code editor highlighting infrastructure)
- Links (rendered as clickable hyperlinks)
- Ordered and unordered lists with proper indentation
- Blockquotes with left border styling
- Horizontal rules as separators
- Tables with headers and row styling
- Images (rendered if our image widget supports it, placeholder otherwise)

**Usage:** Accepts a `&str` of Markdown content and renders it as a widget subtree. Usable anywhere a widget can be placed — windows, dialogs, panels, scroll views. All text rendered through our buffered text widget.

---

## Window System Behaviors to Implement

These are core behaviors we need to study in the reference egui source and implement equivalently in our system:

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

### Phase 1 — Layout Shell & Top Bar
- Implement the top bar with dark/light/system theme switch and selectable app tabs ("Demos", "3D Cube", "Rendering test").
- Implement the right panel (~160px) with logo, About checkbox, scrollable demo/test checklist grouped by "Demos" and "Tests" sections, and "Organize windows" button.
- Build the floating window system: drag, resize, close, collapse, z-order.
- Implement window snapping and canvas constraint.
- Dark/light mode color scheme for all chrome (top bar, panels, window frames).

### Phase 2 — Markdown Widget, About Window & Backend Panel
- Implement the Markdown widget (`widgets/markdown.rs`) using `pulldown-cmark`.
- Implement the About window rendering our `README.md` via the Markdown widget.
- Implement the collapsible Backend panel (left side): run mode, zoom, font selector (default Arial, runtime-switchable), debug options, frame history, memory.
- Implement the File menu inside the Demos view: Organize Windows, Reset Memory, zoom controls.

### Phase 3 — Core Widget Library
- Widget Gallery: label, button, checkbox, radio, slider, drag value, combo box, color picker, toggle switch, progress bar, spinner, separator, hyperlink, image.
- Text input: single-line and multi-line with full clipboard, selection, and undo/redo.
- Password field with visibility toggle (helper module for Misc Demo Window).
- Scrollable regions (vertical, horizontal, both).

### Phase 4 — Demo Windows (Batch 1)
In egui registration order:
- Paint Bezier (control-point editor).
- Code Editor with syntax highlighting.
- Code Example (inline code display).
- Dancing Strings (animated procedural art).
- Drag and Drop (multi-column).
- Extra Viewport (additional viewport creation).
- Font Book (Unicode glyph browser).
- Frame Demo (frame/border styling).
- Highlighting (text highlighting).
- Interactive Container (nested containers).

### Phase 5 — Demo Windows (Batch 2)
- Misc Demo Window (composite sections: text layout, colors, interaction, animation, password, UI composition).
- Modals (modal dialogs with backdrop).
- Multi Touch (gesture recognition).
- Painting (freehand canvas).
- Panels (nested panel layout).
- Popups / Context Menus.
- Scene (2D pan/zoom).
- Screenshot.

### Phase 6 — Demo Windows (Batch 3)
- Scrolling (vertical, horizontal, bidirectional, stick-to-bottom).
- Sliders (all variants).
- Strip Demo (strip layout).
- Table Demo (sortable, resizable columns).
- Text Edit.
- Text Layout (wrapping, alignment, rich text).
- Tooltips.
- Undo/Redo.
- Widget Gallery (showcase of all widget types).
- Window Options (window configuration).

### Phase 7 — Test Windows & Integration
- All 11 test windows: Clipboard, Cursor, Grid, Id, Input Event History, Input, Layout, Manual Layout, Vector Rendering, Tessellation, Window Resize.
- Integrate our existing 3D Cube as a top-level app tab.
- Implement the Rendering test app tab.
- Convert the existing Text tab into a floating demo window using the buffered text widget.
- Audit every text rendering path to confirm buffered text widget usage — no exceptions.
- Match egui font sizes, spacing, and color scheme for both dark and light modes.
- Test all window interactions: snapping, resize, z-order, collapse.
- Performance pass: ensure smooth 60fps with many windows open.
- Touch/mobile responsiveness pass.
- Add source links in each demo window pointing to the corresponding file in https://github.com/larsbrubaker/agg-gui.

---

## Technical Notes

- **No egui code is used.** We study the reference source at `agg-gui/agg-gui/reference-egui-main` for exact layout constants, spacing, colors, and behavioral reference, then implement equivalent functionality in our own coordinate system, widget library, and render pipeline.
- **Our source**: All demo source links point to https://github.com/larsbrubaker/agg-gui — each demo window should include a link to its own source file in this repo.
- **Back-buffered text widgets are the only text path.** Every piece of rendered text — sidebar labels, window titles, button text, text fields, code editors, tooltips, tables, Markdown content — is a back-buffered text widget instance in the widget tree. There is no separate text caching layer, glyph cache, or raw draw-text call. Text caching happens inside the back-buffered text widget's back buffer, and nowhere else. The existing Text tab is converted to a floating window and migrated to the back-buffered text widget.
- **Widget composition, not monolithic widgets.** Every widget that displays text is a structured layout of child widgets that includes back-buffered text widget children. Buttons, checkboxes, sliders, table cells, menu items — all are compositions. No widget internally renders text outside of a child back-buffered text widget.
- **Markdown widget** (`widgets/markdown.rs`) is a core library widget using `pulldown-cmark`. It lives in the `agg-gui` crate (not demo-only) and is available to any consumer of the library. The About window uses it to render `README.md`.
- **Default font is Arial.** The Backend panel provides a font selector dropdown to change the active font at runtime. Font changes propagate immediately to all text.
- **Dark/light mode** is a core requirement. The theme preference switch (dark/light/system) appears in the top bar. All widgets and chrome respond immediately to theme changes. Both color schemes match egui's `Visuals::dark()` and `Visuals::light()`.
- **Each demo is self-contained.** A demo struct owns its state and renders into whatever window/panel it's given.
- **Sidebar state** is a `BTreeSet<String>` of open window names, matching egui's pattern in `DemoWindows`.
- **Window positions/sizes** are persisted in our existing state system so they survive across sessions.
- **Complete parity.** Every demo and test present in egui's demo must have a corresponding implementation in our system. This is the full set (28 demos + 11 tests), plus our own additions (3D Cube as app tab).
- **Icons** are Google Material Icons and Bootstrap Icons, rendered as font glyphs. Both icon fonts are bundled with the application. They replace egui's emoji-based icons for sidebar items, window titles, menu entries, buttons, and theme controls. Prefer Material Icons for standard UI concepts; use Bootstrap Icons where Material lacks coverage.
- **Reference source**: `agg-gui/agg-gui/reference-egui-main`, specifically `crates/egui_demo_lib/src/demo/` for all demo implementations, `crates/egui_demo_lib/src/demo/demo_app_windows.rs` for sidebar/window registration, and `crates/egui_demo_app/src/wrap_app.rs` for the app shell. Study for behavior and exact layout, implement our own.
