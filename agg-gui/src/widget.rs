//! Widget trait, tree traversal, and the top-level [`App`] struct.
//!
//! # Coordinate system
//!
//! Widget bounds are expressed in **parent-local** first-quadrant (Y-up)
//! coordinates. A widget at `bounds.x = 10, bounds.y = 20` is drawn 10 units
//! right and 20 units up from its parent's bottom-left corner.
//!
//! OS/browser mouse events arrive in Y-down screen coordinates. The single
//! conversion `y_up = viewport_height - y_down` happens inside
//! [`App::on_mouse_move`] / [`App::on_mouse_down`] / [`App::on_mouse_up`].
//! All widget code sees Y-up coordinates only.
//!
//! # Tree traversal
//!
//! Paint: root → leaves (children painted on top of parents).
//! Hit test: root → leaves (deepest child under cursor wins).
//! Event dispatch: leaf → root (events bubble up; any widget can consume).

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, Modifiers, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor};

// ---------------------------------------------------------------------------
// Widget trait
// ---------------------------------------------------------------------------

/// Every visible element in the UI is a widget.
///
/// Implementors handle their own painting and event handling. The framework
/// takes care of tree traversal, coordinate translation, and focus management.
pub trait Widget {
    /// Bounding rectangle in **parent-local** Y-up coordinates.
    fn bounds(&self) -> Rect;

    /// Set the bounding rectangle. Called by the parent during layout.
    fn set_bounds(&mut self, bounds: Rect);

    /// Immutable access to child widgets.
    fn children(&self) -> &[Box<dyn Widget>];

    /// Mutable access to child widgets (required for event dispatch + layout).
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>>;

    /// Compute desired size given available space, and update internal layout.
    ///
    /// The parent passes the space it can offer; the widget returns the size it
    /// actually wants to occupy. The parent uses the returned size to set this
    /// widget's bounds before calling `layout` on the next sibling.
    fn layout(&mut self, available: Size) -> Size;

    /// Paint this widget's own content into `ctx`.
    ///
    /// The framework has already translated `ctx` so that `(0, 0)` is this
    /// widget's bottom-left corner. **Do not paint children here** — the
    /// framework recurses into them automatically after `paint` returns.
    ///
    /// `ctx` is a `&mut dyn DrawCtx`; the concrete type is either a software
    /// `GfxCtx` (back-buffer path) or a `GlGfxCtx` (hardware GL path).
    fn paint(&mut self, ctx: &mut dyn DrawCtx);

    /// Return `true` if `local_pos` (in this widget's local coordinates) falls
    /// inside this widget's interactive area. Default: axis-aligned rect test.
    fn hit_test(&self, local_pos: Point) -> bool {
        let b = self.bounds();
        local_pos.x >= 0.0 && local_pos.x <= b.width
            && local_pos.y >= 0.0 && local_pos.y <= b.height
    }

    /// When `true`, `hit_test_subtree` stops recursing into this widget's
    /// children and returns this widget as the hit target.  Used for floating
    /// overlays (e.g. a scrollbar painted above its content) that must claim
    /// the pointer before children that happen to share the same pixels.
    /// Default: `false`.
    fn claims_pointer_exclusively(&self, _local_pos: Point) -> bool { false }

    /// Handle an event. The event's positions are already in **local** Y-up
    /// coordinates. Return [`EventResult::Consumed`] to stop bubbling.
    fn on_event(&mut self, event: &Event) -> EventResult;

    /// Whether this widget can receive keyboard focus. Default: false.
    fn is_focusable(&self) -> bool {
        false
    }

    /// A static name for this widget type, used by the inspector. Default: "Widget".
    fn type_name(&self) -> &'static str {
        "Widget"
    }

    /// Return `false` to suppress painting this widget **and all its children**.
    /// The widget's own `paint()` will not be called.  Default: `true`.
    fn is_visible(&self) -> bool {
        true
    }

    /// Return type-specific properties for the inspector properties pane.
    ///
    /// Each entry is `(name, display_value)`.  The default returns an empty
    /// list; widgets override this to expose their state to the inspector.
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![]
    }

    /// Whether this widget renders into its own offscreen buffer before
    /// compositing into the parent.
    ///
    /// When `true`, `paint_subtree` wraps the widget (and all its descendants)
    /// in `ctx.push_layer` / `ctx.pop_layer`.  The widget and its children draw
    /// into a fresh transparent framebuffer; when complete, the buffer is
    /// SrcOver-composited back into the parent render target.  This enables
    /// per-widget alpha compositing, caching, and isolation.
    ///
    /// Default: `false` (pass-through rendering).
    fn has_backbuffer(&self) -> bool {
        false
    }

    /// Whether the inspector should recurse into this widget's children.
    ///
    /// Returns `false` for widgets that are part of the inspector infrastructure
    /// (e.g. the inspector's own `TreeView`) to prevent the inspector from
    /// showing itself recursively, which would grow the node list every frame.
    ///
    /// The widget itself is still included in the inspector snapshot — only
    /// its subtree is suppressed.
    fn contributes_children_to_inspector(&self) -> bool {
        true
    }

    /// Return `false` to hide this widget (and its subtree) from the inspector
    /// node snapshot entirely.  Intended for zero-size utility widgets such
    /// as layout-time watchers / tickers / invisible composers — they bloat
    /// the inspector tree without providing user-relevant information and,
    /// at scale, can make the inspector's per-frame tree rebuild expensive.
    fn show_in_inspector(&self) -> bool { true }

    /// Paint decorations that must appear **on top of all children**.
    ///
    /// Called by [`paint_subtree`] after all children have been painted.
    /// The default implementation is a no-op; override in widgets that need
    /// to draw overlays (e.g. resize handles, drag previews) that must not
    /// be occluded by child content.
    fn paint_overlay(&mut self, _ctx: &mut dyn DrawCtx) {}

    /// Return a clip rectangle (in local coordinates) that constrains all child
    /// painting.  `paint_subtree` applies this clip before recursing into
    /// children, then restores the previous clip state afterward.  The clip does
    /// **not** affect `paint_overlay`, which runs after the clip is removed.
    ///
    /// The default clips children to this widget's own bounds, preventing
    /// overflow.  Override to return a narrower rect (e.g. Window clips to the
    /// content area below the title bar, or an empty rect when collapsed).
    fn clip_children_rect(&self) -> Option<(f64, f64, f64, f64)> {
        let b = self.bounds();
        Some((0.0, 0.0, b.width, b.height))
    }

    // -------------------------------------------------------------------------
    // Layout properties (universal — every widget carries these)
    // -------------------------------------------------------------------------

    /// Outer margin around this widget in logical units.
    ///
    /// The parent layout reads this to compute spacing and position.
    /// Default: [`Insets::ZERO`].
    fn margin(&self) -> Insets { Insets::ZERO }

    /// Horizontal anchor: how this widget sizes/positions itself horizontally
    /// within the slot the parent assigns.
    /// Default: [`HAnchor::FIT`] (take natural content width).
    fn h_anchor(&self) -> HAnchor { HAnchor::FIT }

    /// Vertical anchor: how this widget sizes/positions itself vertically
    /// within the slot the parent assigns.
    /// Default: [`VAnchor::FIT`] (take natural content height).
    fn v_anchor(&self) -> VAnchor { VAnchor::FIT }

    /// Minimum size constraint (logical units).
    ///
    /// The parent will never assign a slot smaller than this.
    /// Default: [`Size::ZERO`] (no minimum).
    fn min_size(&self) -> Size { Size::ZERO }

    /// Maximum size constraint (logical units).
    ///
    /// The parent will never assign a slot larger than this.
    /// Default: [`Size::MAX`] (no maximum).
    fn max_size(&self) -> Size { Size::MAX }

    /// Whether [`paint_subtree`] should snap this widget's incoming
    /// translation to the physical pixel grid.
    ///
    /// Defaults to the process-wide
    /// [`pixel_bounds::default_enforce_integer_bounds`](crate::pixel_bounds::default_enforce_integer_bounds)
    /// flag so the common case — crisp UI text + strokes — works without
    /// ceremony.  Widgets with a [`WidgetBase`] should delegate to
    /// `self.base().enforce_integer_bounds` so per-instance overrides take
    /// effect; widgets that genuinely want sub-pixel positioning (smooth
    /// scroll markers, zoomed canvases) override to return `false`.
    ///
    /// Mirrors MatterCAD's `GuiWidget.EnforceIntegerBounds` accessor.
    fn enforce_integer_bounds(&self) -> bool {
        crate::pixel_bounds::default_enforce_integer_bounds()
    }
}

// ---------------------------------------------------------------------------
// Tree traversal helpers (free functions operating on &mut dyn Widget)
// ---------------------------------------------------------------------------

/// Paint `widget` and all its descendants. The caller must ensure `ctx` is
/// already translated so that (0,0) maps to `widget`'s bottom-left corner.
///
/// If the widget returns `true` from [`Widget::has_backbuffer`], the entire
/// subtree (widget + all descendants) is rendered into a fresh offscreen layer
/// via [`DrawCtx::push_layer`] / [`DrawCtx::pop_layer`].  The layer is then
/// SrcOver-composited back into the parent render target.
pub fn paint_subtree(widget: &mut dyn Widget, ctx: &mut dyn DrawCtx) {
    if !widget.is_visible() { return; }

    // Buffered widgets: redirect self + descendants into an offscreen layer.
    let buffered = widget.has_backbuffer();
    if buffered {
        let b = widget.bounds();
        ctx.push_layer(b.width, b.height);
    }

    widget.paint(ctx);

    // Always clip children to this widget's clip rect (default: widget bounds).
    // save/restore means the clip is removed before paint_overlay runs.
    let b = widget.bounds();
    let (cx, cy, cw, ch) = widget.clip_children_rect()
        .unwrap_or((0.0, 0.0, b.width, b.height));
    ctx.save();
    ctx.clip_rect(cx, cy, cw, ch);

    // Iterate over indices to avoid holding a reference while recursing.
    let n = widget.children().len();
    for i in 0..n {
        let child_bounds  = widget.children()[i].bounds();
        let snap_to_pixel = widget.children()[i].enforce_integer_bounds();
        ctx.save();
        // Integer-snap the incoming translation when the child opts in — the
        // MatterCAD pattern.  A parent flex layout often produces fractional
        // child Y (`font_size × 1.5` line heights accumulate down a column);
        // snapping here kills the fractional component at exactly one site,
        // guaranteeing that every rect/stroke/blit drawn inside `paint`
        // lands on the physical pixel grid.
        if snap_to_pixel {
            ctx.translate(child_bounds.x.round(), child_bounds.y.round());
        } else {
            ctx.translate(child_bounds.x, child_bounds.y);
        }
        // We need exclusive access to the child. Use index-based access.
        let child = &mut widget.children_mut()[i];
        paint_subtree(child.as_mut(), ctx);
        ctx.restore();
    }

    ctx.restore(); // lifts the children clip before paint_overlay

    if buffered {
        ctx.pop_layer();
    }

    // Paint decorations that must appear on top of all children (e.g. resize handles).
    widget.paint_overlay(ctx);
}

/// Walk the subtree rooted at `widget` and return the path (list of child
/// indices) to the deepest widget that passes `hit_test` at `local_pos`.
///
/// `local_pos` is expressed in `widget`'s coordinate space (not including
/// `widget.bounds().x/y` — the caller has already accounted for that).
///
/// Returns `Some(vec![])` if `widget` itself is hit but no child is.
/// Returns `None` if nothing is hit.
pub fn hit_test_subtree(widget: &dyn Widget, local_pos: Point) -> Option<Vec<usize>> {
    if !widget.is_visible() || !widget.hit_test(local_pos) {
        return None;
    }
    // Let overlays (e.g. a floating scrollbar) claim the pointer before any
    // child that happens to cover the same pixels.
    if widget.claims_pointer_exclusively(local_pos) {
        return Some(vec![]);
    }
    // Check children in reverse order (last drawn = topmost = highest priority).
    for (i, child) in widget.children().iter().enumerate().rev() {
        let child_local = Point::new(
            local_pos.x - child.bounds().x,
            local_pos.y - child.bounds().y,
        );
        if let Some(mut sub_path) = hit_test_subtree(child.as_ref(), child_local) {
            sub_path.insert(0, i);
            return Some(sub_path);
        }
    }
    Some(vec![]) // hit this widget, no child claimed it
}

/// Dispatch `event` through a path (list of child indices from the root).
/// The event bubbles leaf → root; returns `Consumed` if any widget consumed it.
///
/// `pos_in_root` is the event position in the root widget's coordinate space.
/// The function translates it down through each level of the path.
pub fn dispatch_event(
    root: &mut Box<dyn Widget>,
    path: &[usize],
    event: &Event,
    pos_in_root: Point,
) -> EventResult {
    if path.is_empty() {
        return root.on_event(event);
    }
    let idx = path[0];
    // Path can become stale between when it was captured (hit-test or
    // previous-frame hovered/focus) and when it is dispatched — e.g. a
    // CollapsingHeader collapsed since then and dropped its child.  Rather
    // than panic, just stop descending and deliver the event at this level.
    if idx >= root.children().len() {
        return root.on_event(event);
    }
    let child_bounds = root.children()[idx].bounds();
    let child_pos = Point::new(pos_in_root.x - child_bounds.x, pos_in_root.y - child_bounds.y);
    let translated_event = translate_event(event, child_pos);

    let child_result = dispatch_event(
        &mut root.children_mut()[idx],
        &path[1..],
        &translated_event,
        child_pos,
    );
    if child_result == EventResult::Consumed {
        return EventResult::Consumed;
    }
    // Bubble: deliver to this widget too (with original pos_in_root coords).
    root.on_event(event)
}

/// Produce a version of `event` with mouse positions replaced by `new_pos`.
/// Non-mouse events (key, focus) are returned unchanged.
fn translate_event(event: &Event, new_pos: Point) -> Event {
    match event {
        Event::MouseMove { .. } => Event::MouseMove { pos: new_pos },
        Event::MouseDown { button, modifiers, .. } => Event::MouseDown {
            pos: new_pos, button: *button, modifiers: *modifiers,
        },
        Event::MouseUp { button, modifiers, .. } => Event::MouseUp {
            pos: new_pos, button: *button, modifiers: *modifiers,
        },
        Event::MouseWheel { delta_y, delta_x, .. } => Event::MouseWheel {
            pos: new_pos, delta_y: *delta_y, delta_x: *delta_x,
        },
        other => other.clone(),
    }
}

// ---------------------------------------------------------------------------
// Inspector support
// ---------------------------------------------------------------------------

/// Flat snapshot of one widget for the inspector panel.
#[derive(Clone)]
pub struct InspectorNode {
    pub type_name: &'static str,
    /// Absolute screen bounds (Y-up), accumulated as the tree is walked.
    pub screen_bounds: Rect,
    pub depth: usize,
    /// Type-specific display properties from [`Widget::properties`].
    pub properties: Vec<(&'static str, String)>,
}

/// Walk the subtree rooted at `widget` and collect an `InspectorNode` per
/// widget in DFS paint order (root first).
///
/// `screen_origin` is the accumulated parent offset in screen Y-up coords.
pub fn collect_inspector_nodes(
    widget: &dyn Widget,
    depth: usize,
    screen_origin: Point,
    out: &mut Vec<InspectorNode>,
) {
    // Invisible widgets (and their entire subtrees) are excluded from the
    // inspector — they are not part of the live rendered scene.
    if !widget.is_visible() { return; }
    // Utility widgets opt out of the inspector entirely.
    if !widget.show_in_inspector() { return; }

    let b = widget.bounds();
    let abs = Rect::new(
        screen_origin.x + b.x,
        screen_origin.y + b.y,
        b.width,
        b.height,
    );
    // Build the properties vec — include the universal `backbuffer` flag
    // first (so every widget shows it in a consistent location), then the
    // widget-specific properties.
    let mut props = vec![
        ("backbuffer", if widget.has_backbuffer() { "true".to_string() }
                       else                        { "false".to_string() }),
    ];
    props.extend(widget.properties());
    out.push(InspectorNode {
        type_name:  widget.type_name(),
        screen_bounds: abs,
        depth,
        properties: props,
    });

    // Widgets that are part of the inspector infrastructure opt out of child
    // recursion to prevent the inspector from growing its own node list every
    // frame (exponential growth).  Their sub-trees are still visible in the
    // inspector on the next frame through the normal layout snapshot.
    if !widget.contributes_children_to_inspector() { return; }

    let child_origin = Point::new(abs.x, abs.y);
    for child in widget.children() {
        collect_inspector_nodes(child.as_ref(), depth + 1, child_origin, out);
    }
}

/// Collect all focusable widgets in paint order (DFS root → leaves).
/// Returns their paths as `Vec<Vec<usize>>`.
fn collect_focusable(widget: &dyn Widget, current_path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    if widget.is_focusable() {
        out.push(current_path.clone());
    }
    for (i, child) in widget.children().iter().enumerate() {
        current_path.push(i);
        collect_focusable(child.as_ref(), current_path, out);
        current_path.pop();
    }
}

/// Get a mutable reference to the widget at the given path.
fn widget_at_path<'a>(root: &'a mut Box<dyn Widget>, path: &[usize]) -> &'a mut dyn Widget {
    if path.is_empty() {
        return root.as_mut();
    }
    let idx = path[0];
    widget_at_path(&mut root.children_mut()[idx], &path[1..])
}

// ---------------------------------------------------------------------------
// App — top-level owner of the widget tree
// ---------------------------------------------------------------------------

/// Owns the widget tree, handles focus, and converts OS events to Y-up coords.
///
/// Create with [`App::new`], call [`App::layout`] every frame before
/// [`App::paint`], and feed OS events through the `on_*` methods.
pub struct App {
    root: Box<dyn Widget>,
    /// Current focus path (indices from root into children vec).
    /// `None` means no widget has focus.
    focus: Option<Vec<usize>>,
    /// Path to the widget last seen under the cursor (for hover clearing).
    hovered: Option<Vec<usize>>,
    /// Mouse-captured widget path. Set when a widget consumes `MouseDown`;
    /// cleared on `MouseUp`. While set, `MouseMove` events go to the captured
    /// widget regardless of cursor position — enabling slider drag-outside-bounds.
    captured: Option<Vec<usize>>,
    /// Viewport height in pixels — used for Y-down → Y-up conversion.
    viewport_height: f64,
    /// Optional global key handler called *before* dispatching to the focused widget.
    /// Returns `true` if the key was handled globally (suppresses focused dispatch).
    global_key_handler: Option<Box<dyn FnMut(Key, Modifiers) -> bool>>,
}

impl App {
    /// Create a new `App` with `root` as the root widget.
    pub fn new(root: Box<dyn Widget>) -> Self {
        Self {
            root,
            focus: None,
            hovered: None,
            captured: None,
            viewport_height: 1.0,
            global_key_handler: None,
        }
    }

    /// Register a global key handler invoked before the focused widget receives
    /// the key.  Return `true` to consume the event (suppress focused dispatch).
    ///
    /// # Example
    /// ```ignore
    /// app.set_global_key_handler(|key, mods| {
    ///     if mods.ctrl && mods.shift && key == Key::O {
    ///         organize_windows();
    ///         return true;
    ///     }
    ///     false
    /// });
    /// ```
    pub fn set_global_key_handler(&mut self, handler: impl FnMut(Key, Modifiers) -> bool + 'static) {
        self.global_key_handler = Some(Box::new(handler));
    }

    /// Lay out the widget tree to fill `viewport`. Call once per frame before
    /// [`paint`][Self::paint].
    pub fn layout(&mut self, viewport: Size) {
        self.viewport_height = viewport.height;
        self.root.set_bounds(Rect::new(0.0, 0.0, viewport.width, viewport.height));
        self.root.layout(viewport);
    }

    /// Paint the entire widget tree into `ctx`. Call after [`layout`][Self::layout].
    pub fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        paint_subtree(self.root.as_mut(), ctx);
    }

    // --- Platform event ingestion (Y-down → Y-up conversion happens here) ---

    /// Mouse cursor moved. `screen_y` is Y-down (OS / browser convention).
    pub fn on_mouse_move(&mut self, screen_x: f64, screen_y: f64) {
        // Reset cursor so the hovered widget can set it; Default if nothing sets it.
        crate::cursor::reset_cursor_icon();
        let pos = self.flip_y(screen_x, screen_y);
        self.dispatch_mouse_move(pos);
    }

    /// Mouse button pressed. `screen_y` is Y-down.
    pub fn on_mouse_down(&mut self, screen_x: f64, screen_y: f64, button: MouseButton, mods: Modifiers) {
        let pos = self.flip_y(screen_x, screen_y);
        let hit = self.compute_hit(pos);

        // Click-to-focus: if the hit widget is focusable, give it focus.
        if let Some(ref path) = hit {
            let w = widget_at_path(&mut self.root, path);
            if w.is_focusable() {
                self.set_focus(Some(path.clone()));
            } else {
                self.set_focus(None);
            }
        } else {
            self.set_focus(None);
        }

        let event = Event::MouseDown { pos, button, modifiers: mods };
        if let Some(mut path) = hit {
            let result = dispatch_event(&mut self.root, &path, &event, pos);
            if result == EventResult::Consumed {
                self.maybe_bring_to_front(&mut path);
                self.captured = Some(path);
            }
        }
    }

    /// Mouse button released. `screen_y` is Y-down.
    pub fn on_mouse_up(&mut self, screen_x: f64, screen_y: f64, button: MouseButton, mods: Modifiers) {
        let pos = self.flip_y(screen_x, screen_y);
        let event = Event::MouseUp { pos, button, modifiers: mods };
        // Deliver release to captured widget first (if any), then clear capture.
        if let Some(path) = self.captured.take() {
            dispatch_event(&mut self.root, &path, &event, pos);
        } else {
            let hit = self.compute_hit(pos);
            if let Some(path) = hit {
                dispatch_event(&mut self.root, &path, &event, pos);
            }
        }
    }

    /// Key pressed. Delivered to the focused widget and bubbles up.
    ///
    /// If a global key handler was registered via [`set_global_key_handler`] and
    /// it returns `true`, the key is consumed and the focused widget does not
    /// receive it.
    pub fn on_key_down(&mut self, key: Key, mods: Modifiers) {
        if key == Key::Tab {
            self.advance_focus(!mods.shift);
            return;
        }
        // Call global handler first; bail out if it consumes the key.
        if let Some(ref mut handler) = self.global_key_handler {
            if handler(key.clone(), mods) {
                return;
            }
        }
        let event = Event::KeyDown { key, modifiers: mods };
        if let Some(path) = self.focus.clone() {
            dispatch_event(&mut self.root, &path, &event, Point::ORIGIN);
        }
    }

    /// Key released. Delivered to the focused widget.
    pub fn on_key_up(&mut self, key: Key, mods: Modifiers) {
        let event = Event::KeyUp { key, modifiers: mods };
        if let Some(path) = self.focus.clone() {
            dispatch_event(&mut self.root, &path, &event, Point::ORIGIN);
        }
    }

    /// Mouse wheel scrolled. `screen_y` is Y-down. `delta_y` positive = scroll up.
    /// `delta_x` positive = content moves right.
    pub fn on_mouse_wheel(&mut self, screen_x: f64, screen_y: f64, delta_y: f64) {
        self.on_mouse_wheel_xy(screen_x, screen_y, 0.0, delta_y);
    }

    /// Mouse wheel with an explicit horizontal component (trackpad pan,
    /// shift+wheel via the platform harness).
    pub fn on_mouse_wheel_xy(
        &mut self,
        screen_x: f64, screen_y: f64,
        delta_x: f64, delta_y: f64,
    ) {
        let pos = self.flip_y(screen_x, screen_y);
        let hit = self.compute_hit(pos);
        let event = Event::MouseWheel { pos, delta_y, delta_x };
        if let Some(path) = hit {
            dispatch_event(&mut self.root, &path, &event, pos);
        }
    }

    /// Snapshot the entire widget tree for the inspector.
    pub fn collect_inspector_nodes(&self) -> Vec<InspectorNode> {
        let mut out = Vec::new();
        collect_inspector_nodes(self.root.as_ref(), 0, Point::ORIGIN, &mut out);
        out
    }

    /// Serialize the widget tree — types, bounds, depth, properties — as JSON.
    ///
    /// Produces a flat array of nodes in paint-order DFS.  Suitable for writing
    /// to a file and diffing between runs to verify layout stability.  Used by
    /// the demo harness's debug hotkey.
    pub fn dump_tree_json(&self) -> String {
        let nodes = self.collect_inspector_nodes();
        let mut s = String::from("[\n");
        for (i, n) in nodes.iter().enumerate() {
            let props_json = n.properties.iter()
                .map(|(k, v)| format!("{:?}: {:?}", k, v))
                .collect::<Vec<_>>()
                .join(", ");
            s.push_str(&format!(
                "  {{\"type\":{:?},\"depth\":{},\"x\":{:.2},\"y\":{:.2},\"w\":{:.2},\"h\":{:.2},\"props\":{{{}}}}}",
                n.type_name, n.depth,
                n.screen_bounds.x, n.screen_bounds.y,
                n.screen_bounds.width, n.screen_bounds.height,
                props_json,
            ));
            if i + 1 < nodes.len() { s.push(','); }
            s.push('\n');
        }
        s.push(']');
        s
    }

    /// Returns `true` if any widget currently holds keyboard focus.
    /// Used by the render loop to schedule cursor-blink repaints.
    pub fn has_focus(&self) -> bool { self.focus.is_some() }

    /// Call when the cursor leaves the window to clear hover state.
    pub fn on_mouse_leave(&mut self) {
        crate::cursor::reset_cursor_icon();
        self.dispatch_mouse_move(Point::new(-1.0, -1.0));
    }

    // --- Private helpers ---

    /// If the click path passes through a `Window` widget, move that window to
    /// the end of its parent's children list so it paints on top of siblings.
    /// All stored paths (focus, hovered, captured, plus the clicked path itself)
    /// are updated to reflect the new index.
    fn maybe_bring_to_front(&mut self, clicked_path: &mut Vec<usize>) {
        // Walk the clicked path and record the deepest Window encountered.
        // At each step we descend into children[idx]; after descending, if the
        // new node is a Window we record (parent_path, win_idx).  We keep
        // scanning so a nested Window (unlikely but possible) wins.
        let mut node: &dyn Widget = self.root.as_ref();
        let mut window_info: Option<(Vec<usize>, usize)> = None; // (parent_path, win_idx)
        for (depth, &idx) in clicked_path.iter().enumerate() {
            let children = node.children();
            if idx >= children.len() { break; }
            node = &*children[idx];
            if node.type_name() == "Window" {
                // parent_path = clicked_path[..depth], win_idx = idx
                window_info = Some((clicked_path[..depth].to_vec(), idx));
            }
        }

        let (parent_path, win_idx) = match window_info { Some(x) => x, None => return };

        // Check there's actually a sibling to leapfrog.
        let n = {
            let parent = widget_at_path(&mut self.root, &parent_path);
            parent.children().len()
        };
        if win_idx >= n - 1 { return; } // already at front

        // Move the window to the end of its parent's children (mutable pass).
        {
            let parent = widget_at_path(&mut self.root, &parent_path);
            let child = parent.children_mut().remove(win_idx);
            parent.children_mut().push(child);
        }
        let new_idx = n - 1;
        let depth = parent_path.len(); // depth at which the window index sits

        // Update any stored path whose element at `depth` was affected by the move.
        fn shift_path(p: &mut Vec<usize>, depth: usize, old: usize, new: usize) {
            if p.len() > depth {
                let i = p[depth];
                if i == old {
                    p[depth] = new;
                } else if i > old && i <= new {
                    // Siblings that were after the removed window shift left by 1.
                    p[depth] -= 1;
                }
            }
        }
        shift_path(clicked_path, depth, win_idx, new_idx);
        if let Some(ref mut p) = self.focus    { shift_path(p, depth, win_idx, new_idx); }
        if let Some(ref mut p) = self.hovered  { shift_path(p, depth, win_idx, new_idx); }
        if let Some(ref mut p) = self.captured { shift_path(p, depth, win_idx, new_idx); }
    }

    #[inline]
    fn flip_y(&self, x: f64, y_down: f64) -> Point {
        Point::new(x, self.viewport_height - y_down)
    }

    fn compute_hit(&self, pos: Point) -> Option<Vec<usize>> {
        hit_test_subtree(self.root.as_ref(), pos)
    }

    fn dispatch_mouse_move(&mut self, pos: Point) {
        let new_hit = self.compute_hit(pos);

        // If the hovered widget changed, clear the old one — but skip the clear
        // event when the old widget still has mouse capture (it should keep
        // receiving real positions, not a (-1,-1) sentinel that snaps state).
        if new_hit != self.hovered {
            if let Some(old_path) = self.hovered.take() {
                let is_captured = self.captured.as_ref() == Some(&old_path);
                if !is_captured {
                    let clear = Event::MouseMove { pos: Point::new(-1.0, -1.0) };
                    dispatch_event(&mut self.root, &old_path, &clear, Point::new(-1.0, -1.0));
                }
            }
            self.hovered = new_hit.clone();
        }

        let event = Event::MouseMove { pos };
        if let Some(ref cap_path) = self.captured.clone() {
            // Captured widget always receives the real position, regardless of
            // whether the cursor is over it — this is what keeps a slider
            // tracking the cursor when dragged outside its bounds.
            dispatch_event(&mut self.root, cap_path, &event, pos);
        } else if let Some(path) = new_hit {
            dispatch_event(&mut self.root, &path, &event, pos);
        }
    }

    /// Set focus to `new_path`, sending `FocusLost` / `FocusGained` as needed.
    fn set_focus(&mut self, new_path: Option<Vec<usize>>) {
        if self.focus == new_path {
            return;
        }
        if let Some(old) = self.focus.take() {
            dispatch_event(&mut self.root, &old, &Event::FocusLost, Point::ORIGIN);
        }
        self.focus = new_path.clone();
        if let Some(new) = new_path {
            dispatch_event(&mut self.root, &new, &Event::FocusGained, Point::ORIGIN);
        }
    }

    /// Move focus to the next (or previous) focusable widget in paint order.
    fn advance_focus(&mut self, forward: bool) {
        let mut all: Vec<Vec<usize>> = Vec::new();
        collect_focusable(self.root.as_ref(), &mut vec![], &mut all);
        if all.is_empty() {
            return;
        }
        let current_idx = self.focus.as_ref()
            .and_then(|f| all.iter().position(|p| p == f));
        let next_idx = match current_idx {
            None => if forward { 0 } else { all.len() - 1 },
            Some(i) => {
                if forward {
                    (i + 1) % all.len()
                } else {
                    if i == 0 { all.len() - 1 } else { i - 1 }
                }
            }
        };
        let next_path = all[next_idx].clone();
        self.set_focus(Some(next_path));
    }
}
