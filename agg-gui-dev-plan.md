# Custom WGL GUI Framework — Architecture & Development Plan

**AGG-GUI** | Rust + WGL/OpenGL | Owner-Drawn GUI | WASM/WebGL Demo  
**Coordinate System:** First-quadrant (origin bottom-left), mathematically consistent  
**Rendering Model:** Retained-mode, full redraw every frame (game-loop style)  
**Hard Dependencies:** AGG (Rust port), Clipper2 (Rust port), Manifold (Rust port, future)

---

## 1. Architectural Overview

### 1.1 Layer Stack

```
┌─────────────────────────────────────┐
│        Application / Widgets        │  ← Widget tree, all composition
├─────────────────────────────────────┤
│      Layout Widgets (Flex, Grid,    │  ← Layout is widget composition,
│      Stack, Flow, etc.)             │    not a separate engine
├─────────────────────────────────────┤
│      Widget Base (bounds, parent,   │  ← Every visible thing is a widget
│      children, paint, hit-test)     │    with bounds and a parent
├─────────────────────────────────────┤
│      Graphics Context (Cairo-like)  │  ← Stateful 2D drawing API
├─────────────────────────────────────┤
│     AGG + Clipper2                  │  ← Rasterization, boolean geometry
├─────────────────────────────────────┤
│     Framebuffer (full redraw)       │  ← Single RGBA buffer, redrawn every frame
├─────────────────────────────────────┤
│     WGL/OpenGL (native)             │  ← Platform presentation
│     WebGL (WASM demo)               │
└─────────────────────────────────────┘
```

Each layer has a clean trait boundary. No layer reaches more than one level down.

### 1.2 Rendering Model — Full Redraw, Retained Mode

The system redraws the entire UI every frame, like a game renderer. There is no tiling, no dirty-region tracking, no partial redraw.

- A single RGBA framebuffer is allocated at window size
- Every frame: clear the buffer, walk the widget tree, every widget paints into the buffer via `GfxCtx`, upload the buffer to a GL texture, draw a full-screen quad
- This is simple, predictable, and eliminates an entire class of "stale region" bugs
- Performance comes from keeping per-widget paint logic fast, not from avoiding paint calls
- Individual widgets **may** hold internal back buffers (e.g., `TextWidget` caches its rasterized text) but these buffers are still blitted into the main framebuffer every frame — the back buffer avoids re-rasterization, not re-blitting

```
Every frame:
  1. Clear framebuffer
  2. Walk widget tree root → leaves
  3. Each widget paints into framebuffer via GfxCtx
     - TextWidget: blit from its back buffer (re-rasterize only if content changed)
     - Button: paint background directly, child TextWidget blits
     - etc.
  4. Upload framebuffer to GL texture
  5. Draw full-screen quad
```

### 1.3 Coordinate System — First Quadrant Throughout

The entire system operates in first-quadrant coordinates (origin bottom-left, Y-up). This is a **non-negotiable architectural invariant**, not a late-stage transform.

- The graphics context, layout, hit testing, event coordinates, and widget tree all operate in Y-up space.
- AGG is configured for **bottom-up (Y-up) memory order** directly. No Y-flip at the rasterizer boundary.
- GL presentation is naturally Y-up. Framebuffer upload is direct, no inversion.
- Mouse/input events from the OS arrive in screen coordinates (Y-down). They are converted to first-quadrant coordinates **once**, at the event ingestion boundary, before entering the widget tree.
- For WASM/WebGL: browser mouse events are also Y-down; the same single inversion applies at the event boundary.

```
          Application space (Y-up)
                  │
    ┌─────────────┼─────────────┐
    │  Graphics Context (Y-up)  │
    │         │                 │
    │    AGG rasterizer         │
    │    (bottom-up memory,     │
    │     native Y-up)          │
    │         │                 │
    │    Framebuffer upload     │
    │    (direct, no flip)      │
    └───────────┼───────────────┘
                ▼
         GL presentation (Y-up)
```

**Why this matters:** Rotations, arc directions, and angular coordinates behave as mathematicians expect. A positive angle rotates counterclockwise. The unit circle is oriented correctly. Atan2 returns expected values. No mental translation is needed between the framework's behavior and mathematical reference material.

### 1.4 Scaling and Zoom

The system must support arbitrary scale transforms for UI scaling (HiDPI) and interactive zoom (e.g., geometry node graph zooming/panning like Blender).

- The `GfxCtx` transform stack handles scaling naturally — zoom is a scale + translate on the graphics context
- Widget hit testing applies the inverse transform to map mouse coordinates back into widget-local space
- `TextWidget` back buffers are invalidated when scale changes (text must be re-rasterized at the new resolution to remain crisp — blitting a scaled cached bitmap produces blur)
- UI-level DPI scaling is applied as a root transform, separate from application-level zoom

### 1.5 Key Design Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Graphics API model | Cairo-style stateful context | Clean state stack, Rust-friendly lifecycle, proven API surface |
| Widget architecture | Pure composition — everything is a widget with bounds and a parent | Deterministic, debuggable, proven in MatterControl |
| Layout model | Layout widgets (FlexRow, FlexColumn, Stack, etc.) — no external layout engine | Layout is composition, not a separate system. No Taffy dependency |
| Rendering model | Full redraw every frame, retained widget tree | Simple, no dirty-region bugs, game-loop predictability |
| Rasterization | AGG (Rust port) — **hard dependency** | Already available, high-quality AA, supports bottom-up memory |
| Boolean geometry | Clipper2 (Rust port) — **hard dependency** | Already available, clip regions, boolean ops |
| 3D boolean geometry | Manifold (Rust port) — **hard dependency, future** | Integrate when available |
| Text rendering | AGG font pipeline + rustybuzz shaping, TextWidget back buffer | Full subpixel accuracy, kerning preserved, no glyph atlas |
| Coordinate system | First-quadrant Y-up everywhere | Mathematical consistency, GL-native |
| Demo platform | GitHub Pages WASM/WebGL single-page app | Live progress visibility, single WebGL surface |

---

## 2. Core Abstractions

### 2.0 Hard Dependencies — Do Not Reimplement

AGG, Clipper2, and (when available) Manifold are **hard dependencies**. Their types, structures, and APIs are used directly — we do not wrap them in abstraction layers that duplicate their functionality. Specifically:

- **AGG** owns all rasterization: path rendering, scanline generation, anti-aliasing, font/glyph rasterization, gamma correction, and pixel format handling. If AGG has a type for it (e.g., `path_storage`, `conv_transform`, `conv_stroke`, scanline types, pixel format types), we use AGG's type. We do not build parallel path or stroke types.
- **Clipper2** owns all 2D boolean geometry: union, intersection, difference, XOR of polygonal regions. Clip region management in the graphics context delegates to Clipper2 directly.
- **Manifold** (future) will own 3D boolean geometry when integrated. The architecture should not preclude its addition.

The graphics context and widget layers are **consumers** of these libraries, not wrappers around them. Thin adapter code at the boundary is acceptable (e.g., converting our `Color` type to AGG's pixel format), but we never rebuild a capability that exists in a dependency.

### 2.1 Graphics Context (`GfxCtx`)

Modeled after Cairo's `cairo_t`. This is the primary drawing API that all widget painting goes through.

```rust
// Conceptual API — not final signatures
pub struct GfxCtx { /* ... */ }

impl GfxCtx {
    // State stack
    fn save(&mut self);
    fn restore(&mut self);

    // Transform (operates in Y-up space)
    fn translate(&mut self, tx: f64, ty: f64);
    fn rotate(&mut self, radians: f64);   // CCW positive
    fn scale(&mut self, sx: f64, sy: f64);
    fn set_transform(&mut self, matrix: Affine2D);

    // Path construction
    fn begin_path(&mut self);
    fn move_to(&mut self, x: f64, y: f64);
    fn line_to(&mut self, x: f64, y: f64);
    fn cubic_to(&mut self, cx1: f64, cy1: f64, cx2: f64, cy2: f64, x: f64, y: f64);
    fn arc(&mut self, cx: f64, cy: f64, r: f64, start: f64, end: f64); // radians, CCW
    fn close_path(&mut self);

    // Drawing
    fn fill(&mut self);
    fn stroke(&mut self);
    fn clip(&mut self);           // Intersect clip with current path (uses Clipper2)

    // Style state
    fn set_fill_color(&mut self, color: Color);
    fn set_stroke_color(&mut self, color: Color);
    fn set_line_width(&mut self, w: f64);
    fn set_line_join(&mut self, join: LineJoin);
    fn set_line_cap(&mut self, cap: LineCap);
    fn set_blend_mode(&mut self, mode: BlendMode);  // Porter-Duff

    // Text (delegates to AGG font pipeline)
    fn set_font(&mut self, font: &Font);
    fn set_font_size(&mut self, size: f64);
    fn fill_text(&mut self, text: &str, x: f64, y: f64);
    fn measure_text(&self, text: &str) -> TextMetrics;

    // Back buffer blitting
    fn draw_back_buffer(&mut self, buffer: &BackBuffer, offset: Point);

    // Framebuffer access
    fn framebuffer(&self) -> &Framebuffer;
}
```

**State stack entries** hold: current transform, clip region, fill/stroke style, blend mode, line attributes. `save()` / `restore()` push and pop the entire graphics state, exactly as Cairo does.

### 2.2 Framebuffer

```rust
pub struct Framebuffer {
    pixels: Vec<u8>,            // RGBA8, bottom-up row order (Y-up)
    width: u32,
    height: u32,
    gl_texture_id: u32,         // Single texture, uploaded every frame
}
```

One buffer. Full redraw. Full upload. No tiling, no dirty tracking.

### 2.3 Widget Base

Every visible thing in the system is a widget. A widget has bounds, a parent, and children. Layout is determined by widget composition — layout widgets (FlexRow, FlexColumn, etc.) are just widgets that position their children according to a strategy.

```rust
pub trait Widget {
    // Identity and hierarchy
    fn bounds(&self) -> Rect;           // In parent-local coordinates (Y-up)
    fn children(&self) -> &[Box<dyn Widget>];
    fn parent(&self) -> Option<&dyn Widget>;

    // Layout: parent sets available space, widget reports desired size
    fn layout(&mut self, available: Size) -> Size;

    // Paint: widget draws itself, then children draw themselves
    fn paint(&mut self, ctx: &mut GfxCtx);

    // Events
    fn hit_test(&self, point: Point) -> bool;
    fn on_event(&mut self, event: &Event) -> EventResult;

    // Back buffer (optional, off by default)
    fn back_buffer_enabled(&self) -> bool { false }
}
```

**Paint order:** The framework walks the widget tree. For each widget, it applies the widget's transform (translate to the widget's position within its parent), calls `paint()`, then recurses into children. The widget only needs to paint *its own* content — child painting is handled by the framework.

**Layout protocol:** A parent widget calls `layout(available_size)` on each child. The child returns its desired size. The parent then sets each child's position within its own coordinate space. This is pure composition — no external layout engine.

### 2.4 Layout Widgets

Layout is not a separate system — it is implemented by container widgets that know how to position children.

```rust
// Examples of layout widgets — these are regular widgets

pub struct FlexRow { /* flex-direction: row, distributes children horizontally */ }
pub struct FlexColumn { /* flex-direction: column, distributes children vertically */ }
pub struct Stack { /* overlays children at the same position */ }
pub struct Padding { /* single child with insets */ }
pub struct SizedBox { /* forces a specific size on its child */ }
pub struct ScrollView { /* virtual viewport, scrollable content */ }
pub struct Splitter { /* draggable divider between two children */ }
```

Each layout widget implements `layout()` by querying its children's desired sizes and then assigning positions. For example, `FlexColumn` sums children's heights, distributes remaining space according to flex factors, and assigns Y positions bottom-to-top (Y-up: first child at the bottom).

**No Taffy, no CSS, no external layout engine.** If we want flex-like behavior, we implement it in `FlexRow`/`FlexColumn`. If we want grid, we implement it in `Grid`. Each is a self-contained widget.

### 2.5 TextWidget

```rust
pub struct TextWidget {
    text: String,
    font: Font,
    font_size: f64,
    color: Color,
    use_back_buffer: bool,            // Default: true
    back_buffer: Option<BackBuffer>,
}

impl Widget for TextWidget {
    fn back_buffer_enabled(&self) -> bool { self.use_back_buffer }

    fn layout(&mut self, available: Size) -> Size {
        // Measure text via rustybuzz + AGG, return bounding size
    }

    fn paint(&mut self, ctx: &mut GfxCtx) {
        if self.use_back_buffer {
            let buf = self.back_buffer.get_or_insert_with(|| /* allocate */);
            if !buf.valid {
                // Shape text via rustybuzz
                // Rasterize full shaped run into back buffer via AGG
                buf.valid = true;
            }
            ctx.draw_back_buffer(buf, Point::ORIGIN);
        } else {
            // No back buffer — rasterize directly via AGG
            ctx.fill_text(&self.text, 0.0, 0.0);
        }
    }
}
```

**Back-buffer model — available to all widgets, default on for TextWidget only:**
- Every widget has the **option** to use a back buffer. This is a capability of the base widget, not something special to text.
- `TextWidget` has back-buffering **on by default** because text rasterization is expensive relative to blitting. But even `TextWidget` can have it turned off — in that case, it still renders correctly by rasterizing glyphs directly into the framebuffer on every paint call via AGG. The back buffer avoids re-rasterization, not re-drawing.
- All other widgets have back-buffering **off by default**. Any widget can opt in if needed.
- Back-buffered widgets still blit their buffer into the main framebuffer every frame — the full-redraw model is unchanged.

**Text rendering details:**
- We do NOT cache individual glyph bitmaps. Caching isolated glyph bitmaps destroys inter-glyph kerning and subpixel positioning. This is what egui does and it produces visibly poor text.
- Text shaping is handled by rustybuzz (HarfBuzz-compatible). Glyph rasterization is handled entirely by AGG.
- `TextWidget` back buffers are invalidated when text, font, size, or layout bounds change. They are also invalidated when scale changes (zoom / DPI) to maintain crispness.

### 2.6 Event Pipeline

```
OS event (screen coords, Y-down)        Browser event (CSS coords, Y-down)
    │                                         │
    └──────────────┬──────────────────────────┘
                   ▼
Event ingestion: convert to first-quadrant coords
    │   y = window_height - screen_y
    ▼
Event dispatch: walk widget tree
    │   hit_test(point) from root down
    │   transform point into each widget's local space
    ▼
Widget event handler: application code sees Y-up local coords only
```

Events bubble up from leaf to root. Any widget can consume an event to stop propagation. Events carry first-quadrant coordinates by the time any widget code sees them.

---

## 3. Platform Abstraction

### 3.1 Native (Windows WGL)

- Win32 window creation, WGL context setup
- Message pump → event ingestion
- GL texture upload + full-screen quad

### 3.2 WASM/WebGL (Demo)

- `wasm-bindgen` + `web-sys` for canvas and WebGL context
- `requestAnimationFrame` game loop
- Browser keyboard/mouse events → event ingestion (same coordinate conversion)
- Same widget tree, same `GfxCtx`, same AGG rasterization — only the GL presentation layer differs

```rust
// Platform trait — implemented for WGL and WebGL
pub trait Platform {
    fn create_window(&mut self, width: u32, height: u32);
    fn gl_context(&self) -> &GlContext;
    fn poll_events(&mut self) -> Vec<RawEvent>;  // Platform-native events
    fn swap_buffers(&mut self);
}
```

The framework does not know which platform it's running on after initialization. All platform differences are behind this trait.

---

## 4. Standard Widget Library

### Tier 1 — Primitives
- `Container` / `Box`: background color, border, padding, rounded corners
- `TextWidget`: styled text display with back-buffered rasterization
- `Icon`: vector icon rendering (AGG paths)
- `Image`: bitmap display

### Tier 2 — Interactive
- `Button`: press/release states, hover, disabled, focus ring. Contains a `TextWidget` child.
- `TextField`: text input, cursor, selection, clipboard. Contains a `TextWidget` for display.
- `Checkbox`, `RadioButton`: toggle state, group management
- `Slider`: continuous value selection, thumb drag

### Tier 3 — Layout Containers
- `FlexRow`, `FlexColumn`: flex-style distribution
- `Stack`: overlay children
- `ScrollView`: virtual viewport, scroll bars, inertial scrolling
- `ListView`: virtualized list (only lays out / paints visible items)
- `Splitter`: draggable divider between two children
- `TabView`: tabbed container with tab bar (used in the WASM demo for feature sections)
- `Padding`, `SizedBox`, `Spacer`: spacing utilities

### Tier 4 — Complex
- `TreeView`: hierarchical tree with expand/collapse, optional drag-and-drop (see §4.1)
- `Dialog` / `Modal`: overlay with focus trapping
- `Menu` / `ContextMenu`: popup menus with keyboard navigation
- `Tooltip`: hover-triggered overlay
- `DropdownSelect`: combined button + popup list

### 4.1 TreeView — Detailed Design

TreeView is a critical widget for the geometry node graph application. It needs to support large hierarchies, drag-and-drop for both external items and internal reordering, and smooth interaction.

```rust
pub struct TreeView {
    root: TreeNode,
    selected: Vec<NodeId>,
    expanded: HashSet<NodeId>,
    drag_state: Option<DragState>,
}

pub struct TreeNode {
    id: NodeId,
    label: TextWidget,           // Text rendering via composition
    icon: Option<Icon>,
    children: Vec<TreeNode>,
    can_accept_drop: bool,       // Can items be dropped onto this node?
    draggable: bool,             // Can this node be dragged?
}

pub enum DragOperation {
    ReorderWithin,               // Rearranging nodes within the tree
    InsertExternal(DragPayload), // Dropping an external item into the tree
}

pub struct DropTarget {
    node_id: NodeId,
    position: DropPosition,      // Before, After, or AsChild
}

pub enum DropPosition {
    Before,                      // Insert before this node (same level)
    After,                       // Insert after this node (same level)
    AsChild,                     // Insert as a child of this node
}
```

**Visual feedback during drag:**
- A drop indicator line shows where the item will land (between nodes, or indented to show "as child")
- The hovered node highlights if it can accept children
- The dragged item follows the cursor as a semi-transparent ghost

**TreeView as composition:**
- Each `TreeNode` is itself a widget (row container with indent, expand arrow, icon, `TextWidget`)
- `TreeView` is a `ScrollView` containing a virtualized flat list of visible (expanded) rows
- Expand/collapse changes which rows are visible, triggering re-layout
- Indentation is a simple x-offset based on depth level

---

## 5. WASM Demo — GitHub Pages

### 5.1 Purpose

A single-page GitHub Pages deployment that serves as:
- A live progress tracker — we deploy as we build, so the demo grows with the library
- A visual showcase of every widget and feature
- A proof that the framework runs identically on WASM/WebGL as on native WGL

### 5.2 Structure

The demo is a **single WebGL canvas** filling the browser viewport. All UI — tabs, navigation, content — is rendered by our widget system. There is no HTML UI outside the canvas.

```
┌─────────────────────────────────────────────────┐
│  Tab Bar (our TabView widget)                   │
│  ┌──────┬──────┬──────┬──────┬──────┐           │
│  │Basics│Layout│Text  │Tree  │Graph │           │
│  └──────┴──────┴──────┴──────┴──────┘           │
├─────────────────────────────────────────────────┤
│                                                 │
│  Tab content area — each tab demonstrates       │
│  a set of widgets / features                    │
│                                                 │
│  "Basics": buttons, checkboxes, sliders,        │
│            text fields, color, borders           │
│                                                 │
│  "Layout": flex rows/columns, nesting,          │
│            scroll views, splitters               │
│                                                 │
│  "Text": font rendering, sizes, styles,         │
│          multi-line, measurement accuracy         │
│                                                 │
│  "Tree": TreeView with drag/drop, large         │
│          hierarchy, expand/collapse              │
│                                                 │
│  "Graph": Node graph demo (Blender-style),      │
│           zoom/pan, connections, minimap          │
│                                                 │
└─────────────────────────────────────────────────┘
```

### 5.3 Demo Deployment Milestones

The demo is deployed incrementally — each phase adds a visible section:

| Phase completed | What appears in the demo |
|---|---|
| Phase 1 | Canvas renders, background color, a few paths — "it works" proof |
| Phase 2 | Basics tab: shapes, strokes, fills, transforms, clipping |
| Phase 3 | Text tab: rendered strings at various sizes, fonts, styles |
| Phase 4 | Interaction: buttons respond to clicks, text fields accept input |
| Phase 5 | Layout tab: flex rows/columns, scroll views |
| Phase 6 | Tree tab: TreeView with expand/collapse, drag/drop |
| Phase 7 | Graph tab: node graph with zoom/pan |
| Phase 8 | Polish: theming, smooth scrolling, complete widget showcase |

### 5.4 WASM Build Pipeline

```
cargo build --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir demo/pkg
# GitHub Actions deploys demo/ to GitHub Pages on push to main
```

CI builds the WASM demo on every push. The live demo always reflects the current state of main.

---

## 6. Development Phases

Each phase produces a **testable, demonstrable artifact** and a visible update to the WASM demo.

### Phase 1: Framebuffer, AGG Bridge, and GL Presentation

**Goal:** Render anti-aliased paths to screen via AGG → framebuffer → GL texture → window. Prove the coordinate system. Get the WASM demo canvas live.

**Deliverables:**
- `Framebuffer` struct (single RGBA buffer, bottom-up row order)
- AGG configured for bottom-up rendering
- `GlPresenter`: uploads framebuffer to a GL texture, draws full-screen quad
- Platform trait with WGL and WebGL implementations
- Game loop: clear → draw → upload → present
- WASM demo deployed to GitHub Pages showing rendered paths

**Verification tests:**
1. **Unit circle test:** Render a circle at (0.5, 0.5) normalized. Verify correct quadrant placement.
2. **Rotation test:** Render a right-pointing arrow, rotate +π/2. Verify it points upward.
3. **Bottom-up memory test:** Render a dot at Y=10 in a 100×100 buffer. Verify it's at row 10 from the bottom in memory.
4. **WASM parity test:** Same test scenes render identically in native WGL and WASM/WebGL.

**Estimated scope:** ~2–3 weeks

---

### Phase 2: Graphics Context API

**Goal:** Full `GfxCtx` with state stack, transforms, clipping, compositing, and stroke engine.

**Deliverables:**
- `GfxCtx` struct with full API as described in §2.1
- State stack (`save` / `restore`)
- Affine transform pipeline
- Clip region management via Clipper2
- Porter-Duff blend modes
- Stroke engine: line width, joins, caps

**Verification tests:**
1. **State stack round-trip:** Save, modify all state, restore, verify exact reversion.
2. **Nested transform test:** Translate + rotate + draw line, verify pixel position matches hand-computed result.
3. **Clip intersection test:** Circular clip ∩ rectangular clip, fill surface, verify only intersection has pixels.
4. **Blend mode test:** Overlapping colored rects with each blend mode, compare against Porter-Duff reference.
5. **Stroke geometry test:** Known path with specific width/join/cap, verify corner geometry.
6. **Visual regression suite:** Battery of test scenes, hashed pixel output.

**Demo update:** Basics tab with interactive shapes, transforms, clipping demos.

**Estimated scope:** ~3–4 weeks

---

### Phase 3: Text Rendering

**Goal:** Shaped, kerned, subpixel-accurate text via AGG, with `TextWidget` back-buffer architecture.

**Deliverables:**
- Font loading via `fontdb`
- Text shaping via `rustybuzz`
- Glyph rasterization through AGG's font pipeline
- `TextWidget` with optional back buffer (default on)
- `GfxCtx` text API: `fill_text`, `measure_text`

**Verification tests:**
1. **Kerning test:** "AV" glyph distance matches font kern table.
2. **Subpixel test:** Same string at x=100.0 vs x=100.25 produces different pixel output.
3. **Back-buffer toggle test:** TextWidget renders identically with back buffer on and off.
4. **Back-buffer invalidation test:** Change text → buffer invalid → repaint → buffer valid.
5. **No-change frame test:** Unchanged TextWidget does not invoke AGG on second frame.
6. **Baseline test:** Text baseline aligns with specified Y (ascenders up, descenders down in Y-up space).
7. **Scale invalidation test:** Change scale/zoom, verify back buffer re-rasterizes at new resolution.
8. **Multi-line test:** Word-wrapped paragraph renders correctly in Y-up (first line at top = highest Y).
9. **Visual comparison test:** Side-by-side our rendering vs glyph-atlas approach. Document quality difference.

**Demo update:** Text tab showing rendered strings, sizes, fonts, multi-line.

**Estimated scope:** ~3–4 weeks

---

### Phase 4: Widget Base, Events, and Hit Testing

**Goal:** Establish the widget tree, event pipeline, and basic interactive widgets. First time the demo responds to input.

**Deliverables:**
- `Widget` trait as described in §2.3
- Widget tree traversal for paint and hit testing
- Event ingestion with coordinate inversion (Y-down → Y-up, one point, both platforms)
- Event bubbling (leaf → root, consume to stop)
- Focus model (tab navigation, focus ring)
- Cursor management
- Basic widgets: `Container`, `Button` (with `TextWidget` child), `TextField`

**Verification tests:**
1. **Coordinate inversion:** Click at screen (100, 50) in 800×600 window → event at (100, 550).
2. **Hit test accuracy:** Non-overlapping rects, clicks inside/outside/boundary, verify correct hits.
3. **Z-order hit test:** Overlapping widgets, verify topmost receives hit.
4. **Bubbling:** Click child, child sees event first, parent sees it if not consumed.
5. **Focus navigation:** Tab through focusable widgets in correct order.
6. **Button interaction:** Hover → press → release → click callback fires.
7. **TextField:** Type text, cursor moves, selection works, clipboard works.

**Demo update:** Basics tab now interactive — clickable buttons, typeable text fields.

**Estimated scope:** ~3–4 weeks

---

### Phase 5: Layout Widgets

**Goal:** Implement layout containers. All layout is widget composition — no external engine.

**Deliverables:**
- `FlexRow`: distributes children horizontally with flex factors
- `FlexColumn`: distributes children vertically with flex factors (Y-up: first child at bottom)
- `Stack`: overlays children
- `Padding`, `SizedBox`, `Spacer`
- `ScrollView`: virtual viewport with scroll bars
- `Splitter`: draggable divider
- `TabView`: tabbed container (used in the demo itself)

**Verification tests:**
1. **Flex row:** Three children flex-grow 1, each gets 1/3 width.
2. **Flex column Y-up:** First child is at the bottom (lowest Y), last child at top (highest Y).
3. **Nested layout:** FlexRow containing FlexColumn, all positions correct through two levels.
4. **ScrollView:** Content larger than viewport, scroll bar appears, scrolling moves content.
5. **Splitter:** Drag divider, children resize, minimum sizes respected.
6. **TabView:** Click tabs, content area switches between children.
7. **Resize:** Window resize triggers re-layout, all positions update.

**Demo update:** Layout tab with flex demos, scroll views, splitters. The demo itself now uses TabView for navigation.

**Estimated scope:** ~3–4 weeks

---

### Phase 6: TreeView and Drag-and-Drop

**Goal:** TreeView widget with full drag-and-drop for node reordering and external item insertion.

**Deliverables:**
- `TreeView` and `TreeNode` as described in §4.1
- Expand/collapse with indentation
- Single and multi-selection
- Keyboard navigation (arrow keys to move, enter to expand/collapse, space to select)
- Drag-and-drop: reorder nodes within the tree
- Drag-and-drop: accept external items into the tree
- Drop position indicators (before, after, as-child)
- Virtualized rendering (only visible expanded rows paint)

**Verification tests:**
1. **Expand/collapse:** Click expand arrow, children appear below. Click again, children disappear. Verify Y positions are correct (children at lower Y than parent in Y-up).
2. **Selection:** Click node, it selects. Ctrl-click adds to selection. Shift-click selects range.
3. **Keyboard nav:** Arrow keys move selection through visible nodes. Right expands, Left collapses.
4. **Drag reorder:** Drag node A before node B, tree order updates. Verify the data model reflects the visual change.
5. **Drag as-child:** Drag node over a folder node, drop indicator shows "as child", drop inserts as last child.
6. **External drop:** Drag an item from outside the tree, drop onto a node, verify callback fires with correct target.
7. **Large tree:** 10,000 nodes, only visible rows rendered, smooth scroll and expand/collapse.
8. **Deep nesting:** 20 levels deep, indentation renders correctly, drag-drop works at all depths.

**Demo update:** Tree tab with a large interactive tree, drag-and-drop demo.

**Estimated scope:** ~3–4 weeks

---

### Phase 7: Node Graph Widget (Blender-style)

**Goal:** A zoomable, pannable node graph widget for the geometry node application. This exercises the scaling infrastructure and demonstrates the framework's capability for complex custom widgets.

**Deliverables:**
- `NodeGraph` widget: zoomable/pannable canvas
- `GraphNode` widget: titled box with input/output ports
- Connection rendering: bezier curves between ports (drawn via `GfxCtx` paths)
- Port interaction: click-drag from output to input to create connections
- Box selection: drag to select multiple nodes
- Minimap: small overview showing viewport position within the full graph
- Zoom/pan: mouse wheel to zoom, middle-click drag to pan, zoom centers on cursor

**Verification tests:**
1. **Zoom test:** Zoom in, node graph scales up. Zoom out, it scales down. Text remains crisp at each zoom level (TextWidget back buffers invalidated and re-rasterized).
2. **Pan test:** Middle-drag pans the view. Node positions in graph space do not change.
3. **Connection test:** Drag from output port to input port, bezier curve renders, connection is established in the data model.
4. **Box selection:** Drag-select area, all nodes within the area are selected.
5. **Scale + hit test:** At 200% zoom, clicking a node still hits correctly (inverse transform applied).
6. **Minimap:** Minimap shows all nodes, viewport rectangle is visible and draggable.

**Demo update:** Graph tab with an interactive node graph demo.

**Estimated scope:** ~4–5 weeks

---

### Phase 8: Theming, Accessibility, and Polish

**Goal:** Production readiness. Consistent styling, HiDPI, performance verification.

**Deliverables:**
- Theme system: `Theme` struct with colors, fonts, spacing, radii. All widgets read from theme.
- HiDPI: root scale transform, TextWidget re-rasterization at native density.
- Performance profiling: frame time budget (layout + paint + upload), ensure 60fps for standard UIs.
- Keyboard shortcuts and accessibility labels (screen reader integration deferred to follow-up).

**Verification tests:**
1. **Theme swap:** Switch themes at runtime, all widgets update without tree rebuild.
2. **HiDPI:** 2× scale, text and paths are crisp.
3. **60fps:** Complex UI (tree + node graph + text) maintains 60fps.
4. **Full demo polish:** All tabs in the WASM demo are complete and interactive.

**Estimated scope:** ~3–4 weeks

---

## 7. Milestone Summary

| Phase | Deliverable | Key Proof | Est. Weeks |
|---|---|---|---|
| 1 | Framebuffer + AGG + GL + WASM canvas | Paths render in Q1 coords, demo is live | 2–3 |
| 2 | Graphics context API | Visual regression suite passes | 3–4 |
| 3 | Text rendering + TextWidget | Subpixel-accurate kerned text, back buffer works | 3–4 |
| 4 | Widget base + events + basic widgets | Interactive buttons and text fields in demo | 3–4 |
| 5 | Layout widgets + TabView | Flex layout, scroll, tabs — demo uses its own tabs | 3–4 |
| 6 | TreeView + drag-and-drop | Large tree with drag reorder and external drop | 3–4 |
| 7 | Node graph (Blender-style) | Zoomable pannable graph with connections | 4–5 |
| 8 | Theming + polish | 60fps, HiDPI, complete demo | 3–4 |
| | **Total** | | **~24–33 weeks** |

---

## 8. Risks and Mitigations

### Risk: Full-redraw performance at large window sizes / HiDPI
**Mitigation:** At 4K (3840×2160×4 bytes) the framebuffer is ~33MB, and the GL texture upload is the bottleneck. Modern GPUs handle this comfortably via `glTexSubImage2D` with PBO (pixel buffer object) for async upload. If profiling shows upload is a problem, we add PBO double-buffering — the framework doesn't need to change, only the presenter. Avoid premature optimization: measure first.

### Risk: AGG rasterization speed for complex scenes at 60fps
**Mitigation:** AGG is fast for typical GUI rendering (rectangles, rounded rects, text, simple paths). The full-redraw model means we rasterize everything every frame, but most GUI widgets are geometrically simple. TextWidget back buffers eliminate the most expensive per-frame cost. If a custom widget is too expensive (e.g., a dense node graph), it can opt into a back buffer. Profile before adding complexity.

### Risk: TextWidget back-buffer memory with scale changes
**Mitigation:** Back buffers are re-allocated at the new size when scale changes. A zoom animation would cause repeated allocation — for smooth zoom, consider rendering text at the nearest cached scale and scaling the bitmap during the animation, then re-rasterizing at the final scale when zoom settles. This is a targeted optimization for the node graph zoom use case.

### Risk: Layout widget complexity without an established algorithm (no Taffy)
**Mitigation:** Start with simple layout widgets (FlexRow, FlexColumn) that implement a straightforward single-pass size negotiation. Do not attempt a full CSS flex spec — implement the subset needed for our UIs. Add complexity (wrapping, baseline alignment, etc.) only when a real widget needs it. The MatterControl layout system worked with simpler semantics than CSS flex, and that's fine.

### Risk: TreeView drag-and-drop edge cases
**Mitigation:** Drag-and-drop has many subtle interactions (scroll-on-drag-near-edge, auto-expand-on-hover, drop-between vs drop-as-child disambiguation). Implement the basic mechanics first (Phase 6), then add polish behaviors incrementally. Define a clear acceptance test list before starting.

### Risk: WASM/WebGL parity with native WGL
**Mitigation:** The platform trait is thin — only window creation, GL context, event polling, and buffer swap. Everything above it (AGG, GfxCtx, widgets) is pure Rust that compiles to both targets. Test parity by running the visual regression suite in both environments. Differences will be in event handling edge cases (keyboard shortcuts, clipboard API), not in rendering.

---

## 9. Non-Goals (Explicitly Out of Scope)

- **GPU-accelerated path rendering.** AGG software rasterization with full-frame GL upload is the architecture.
- **Tiling or partial/dirty-region redraw.** Full redraw every frame. Simple and correct.
- **External layout engine (Taffy, CSS, etc.).** Layout is widget composition.
- **Immediate-mode GUI.** Retained-mode widget tree.
- **CSS parsing or HTML rendering.** We are not building a browser.
- **Cross-platform window management beyond WGL and WebGL.** macOS/Linux is a future effort.
- **Individual glyph bitmap caching / glyph atlas.** Text is rasterized as full shaped runs via AGG.
- **3D rendering integration.** The framework is 2D.

---

## 10. Testing Strategy

### 10.1 Test Categories

**Unit tests:** Pure functions (coordinate math, color blending, layout computation) tested exhaustively.

**Visual regression tests:** Test harness renders scenes to pixel buffers, compares against stored reference images. Any visual change requires explicit approval.

**Integration tests:** Create a widget tree, simulate events, verify resulting pixel output and state changes.

**Performance tests:** Frame time budgets for full-tree paint + upload. Run in CI, fail on regression.

**WASM parity tests:** Same visual regression suite runs in both native and WASM, outputs compared.

### 10.2 Coordinate System Test Suite

Dedicated tests that run at every layer to guard the first-quadrant invariant:

1. A point at (10, 10) in application space appears at the bottom-left region of the window.
2. A positive rotation visually rotates counterclockwise.
3. An arc from 0 to π/2 sweeps from +X toward +Y (rightward and upward).
4. Text baseline at Y=baseline_y has ascenders above and descenders below.
5. A mouse click at the bottom-left corner of the window produces coordinates near (0, 0).
6. In a FlexColumn, the first child has a lower Y value than the last child.

These tests run on every commit.
