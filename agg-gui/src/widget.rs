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

    // Iterate over indices to avoid holding a reference while recursing.
    let n = widget.children().len();
    for i in 0..n {
        let child_bounds = widget.children()[i].bounds();
        ctx.save();
        ctx.translate(child_bounds.x, child_bounds.y);
        // We need exclusive access to the child. Use index-based access.
        let child = &mut widget.children_mut()[i];
        paint_subtree(child.as_mut(), ctx);
        ctx.restore();
    }

    if buffered {
        ctx.pop_layer();
    }
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
    if !widget.hit_test(local_pos) {
        return None;
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
        Event::MouseWheel { delta_y, .. } => Event::MouseWheel { pos: new_pos, delta_y: *delta_y },
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

    let b = widget.bounds();
    let abs = Rect::new(
        screen_origin.x + b.x,
        screen_origin.y + b.y,
        b.width,
        b.height,
    );
    out.push(InspectorNode {
        type_name:  widget.type_name(),
        screen_bounds: abs,
        depth,
        properties: widget.properties(),
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
    /// Viewport height in pixels — used for Y-down → Y-up conversion.
    viewport_height: f64,
}

impl App {
    /// Create a new `App` with `root` as the root widget.
    pub fn new(root: Box<dyn Widget>) -> Self {
        Self {
            root,
            focus: None,
            hovered: None,
            viewport_height: 1.0,
        }
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
        if let Some(path) = hit {
            dispatch_event(&mut self.root, &path, &event, pos);
        }
    }

    /// Mouse button released. `screen_y` is Y-down.
    pub fn on_mouse_up(&mut self, screen_x: f64, screen_y: f64, button: MouseButton, mods: Modifiers) {
        let pos = self.flip_y(screen_x, screen_y);
        let hit = self.compute_hit(pos);
        let event = Event::MouseUp { pos, button, modifiers: mods };
        if let Some(path) = hit {
            dispatch_event(&mut self.root, &path, &event, pos);
        }
    }

    /// Key pressed. Delivered to the focused widget and bubbles up.
    pub fn on_key_down(&mut self, key: Key, mods: Modifiers) {
        if key == Key::Tab {
            self.advance_focus(!mods.shift);
            return;
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
    pub fn on_mouse_wheel(&mut self, screen_x: f64, screen_y: f64, delta_y: f64) {
        let pos = self.flip_y(screen_x, screen_y);
        let hit = self.compute_hit(pos);
        let event = Event::MouseWheel { pos, delta_y };
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

    /// Call when the cursor leaves the window to clear hover state.
    pub fn on_mouse_leave(&mut self) {
        self.dispatch_mouse_move(Point::new(-1.0, -1.0));
    }

    // --- Private helpers ---

    #[inline]
    fn flip_y(&self, x: f64, y_down: f64) -> Point {
        Point::new(x, self.viewport_height - y_down)
    }

    fn compute_hit(&self, pos: Point) -> Option<Vec<usize>> {
        hit_test_subtree(self.root.as_ref(), pos)
    }

    fn dispatch_mouse_move(&mut self, pos: Point) {
        let new_hit = self.compute_hit(pos);

        // If the hovered widget changed, clear the old one.
        if new_hit != self.hovered {
            if let Some(old_path) = self.hovered.take() {
                // Send an out-of-bounds move to the old widget to clear hover.
                let clear = Event::MouseMove { pos: Point::new(-1.0, -1.0) };
                dispatch_event(&mut self.root, &old_path, &clear, Point::new(-1.0, -1.0));
            }
            self.hovered = new_hit.clone();
        }

        let event = Event::MouseMove { pos };
        if let Some(path) = new_hit {
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
