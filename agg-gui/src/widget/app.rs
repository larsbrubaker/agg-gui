use super::*;

/// Collect all focusable widgets in paint order (DFS root → leaves).
/// Returns their paths as `Vec<Vec<usize>>`.
fn collect_focusable(
    widget: &dyn Widget,
    current_path: &mut Vec<usize>,
    out: &mut Vec<Vec<usize>>,
) {
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

fn widget_at_path_ref<'a>(root: &'a dyn Widget, path: &[usize]) -> &'a dyn Widget {
    if path.is_empty() {
        return root;
    }
    let idx = path[0];
    widget_at_path_ref(root.children()[idx].as_ref(), &path[1..])
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
    /// Viewport size in logical pixels from the most recent layout pass.
    viewport_size: Size,
    /// Optional legacy key handler called after widget-tree dispatch.
    /// Returns `true` if the key was handled.
    global_key_handler: Option<Box<dyn FnMut(Key, Modifiers) -> bool>>,
    /// Multi-touch gesture recogniser.  Platform shells feed raw touches
    /// through [`App::on_touch_start/move/end/cancel`]; widgets read the
    /// per-frame aggregate via [`crate::current_multi_touch`].
    touch_state: crate::touch_state::TouchState,
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
            viewport_size: Size::new(1.0, 1.0),
            global_key_handler: None,
            touch_state: crate::touch_state::TouchState::new(),
        }
    }

    /// Access the root widget — used by tests and inspectors that need to
    /// introspect the laid-out tree without re-routing events through the
    /// full dispatch machinery.  Pair with [`find_widget_by_id`] to locate
    /// a specific widget by its `Widget::id()` (e.g. a Window's title).
    pub fn root(&self) -> &dyn Widget {
        self.root.as_ref()
    }

    /// Mutable counterpart to [`root`].  Required when a test wants to
    /// drive a specific sub-widget directly (e.g. reading ScrollView
    /// scroll offset) after the App has routed an event.
    pub fn root_mut(&mut self) -> &mut dyn Widget {
        self.root.as_mut()
    }

    /// Return the type name of the currently focused widget, if any.
    pub fn focused_widget_type_name(&self) -> Option<&'static str> {
        self.focus
            .as_deref()
            .map(|path| widget_at_path_ref(self.root.as_ref(), path).type_name())
    }

    /// Register a legacy global key handler invoked only after the widget tree
    /// has ignored the key. Prefer widget-owned key handling for new behavior.
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
    pub fn set_global_key_handler(
        &mut self,
        handler: impl FnMut(Key, Modifiers) -> bool + 'static,
    ) {
        self.global_key_handler = Some(Box::new(handler));
    }

    /// Lay out the widget tree to fill `viewport`.  `viewport` is in **physical
    /// pixels** (e.g. `window.inner_size()` on native, `canvas.width/height` on
    /// wasm); this method divides by the current device scale factor so the
    /// widget tree lays out in logical (device-independent) units.  Call once
    /// per frame before [`paint`][Self::paint].
    pub fn layout(&mut self, viewport: Size) {
        let scale = crate::device_scale::device_scale().max(1e-6);
        let logical = Size::new(viewport.width / scale, viewport.height / scale);
        self.viewport_height = logical.height;
        self.viewport_size = logical;
        set_current_viewport(logical);
        self.root
            .set_bounds(Rect::new(0.0, 0.0, logical.width, logical.height));
        self.root.layout(logical);
    }

    /// Paint the entire widget tree into `ctx`. Call after [`layout`][Self::layout].
    ///
    /// Applies a `ctx.scale(dps, dps)` transform up-front so the whole tree —
    /// widget dimensions, font sizes, margins — is rendered at physical pixel
    /// density on HiDPI screens without any widget having to know about DPI.
    ///
    /// Also clears the immediate draw flag so widgets can re-request it during
    /// this paint if they need another frame; hosts read [`wants_draw`]
    /// after `paint` returns to decide whether to schedule continuous draws.
    pub fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        crate::animation::clear_draw_request();
        let viewport = self.viewport_size;
        crate::widgets::combo_box::begin_combo_popup_frame(viewport);
        crate::widgets::tooltip::begin_tooltip_frame();
        // Recompute the multi-touch aggregate once per paint and publish
        // to the thread-local — widgets read it during `on_event` or
        // `paint` without an explicit `&App` reference.
        self.touch_state.update_gesture();
        crate::touch_state::set_current(self.touch_state.current());
        let scale = crate::device_scale::device_scale();
        if (scale - 1.0).abs() > 1e-6 {
            ctx.save();
            ctx.scale(scale, scale);
            paint_subtree(self.root.as_mut(), ctx);
            crate::widgets::combo_box::paint_global_combo_popups(ctx);
            crate::widgets::tooltip::paint_global_tooltips(ctx, viewport);
            paint_global_overlays(self.root.as_mut(), ctx);
            // Modal/global overlays can contain ComboBox widgets. They submit
            // their popups while `paint_global_overlays` runs, so drain once
            // more to draw those popups above the modal body.
            crate::widgets::combo_box::paint_global_combo_popups(ctx);
            ctx.restore();
        } else {
            paint_subtree(self.root.as_mut(), ctx);
            crate::widgets::combo_box::paint_global_combo_popups(ctx);
            crate::widgets::tooltip::paint_global_tooltips(ctx, viewport);
            paint_global_overlays(self.root.as_mut(), ctx);
            crate::widgets::combo_box::paint_global_combo_popups(ctx);
        }
    }

    /// After a paint pass, returns `true` if any widget requested another frame
    /// (e.g. an in-progress hover animation).  Hosts should use this to set
    /// their event-loop control flow to continuous polling while it's `true`.
    ///
    /// Combines the visibility-gated tree-walk signal ([`Widget::needs_draw`])
    /// with the immediate draw request flag ([`crate::animation::wants_draw`]).
    /// Widgets call `request_draw` for ordinary visual invalidation; scheduled
    /// draw needs such as cursor blink should use `needs_draw` /
    /// `next_draw_deadline` so hidden subtrees do not keep the loop awake.
    pub fn wants_draw(&self) -> bool {
        self.root.needs_draw() || crate::animation::wants_draw()
    }

    /// Earliest scheduled draw deadline across the visible widget tree.
    /// Hosts translate `Some(t)` into `ControlFlow::WaitUntil(t)` so that
    /// e.g. a text field's cursor blink wakes the loop exactly at the flip
    /// boundary.  Invisible subtrees contribute nothing.
    pub fn next_draw_deadline(&self) -> Option<web_time::Instant> {
        self.root.next_draw_deadline()
    }

    // --- Platform event ingestion ---
    //
    // Hosts pass raw physical-pixel coordinates (e.g. `e.clientX * devicePixelRatio`
    // in wasm, or `WindowEvent::CursorMoved.position` on native).  These methods
    // divide by the current device scale factor and flip Y so widget code sees
    // logical Y-up coordinates matching the layout pass.

    /// Mouse cursor moved. `screen_y` is Y-down physical pixels.
    pub fn on_mouse_move(&mut self, screen_x: f64, screen_y: f64) {
        // Reset cursor so the hovered widget can set it; Default if nothing sets it.
        crate::cursor::reset_cursor_icon();
        let pos = self.flip_y(screen_x, screen_y);
        set_current_mouse_world(pos);
        if let Some(path) = active_modal_path(self.root.as_ref()) {
            let event = Event::MouseMove { pos };
            dispatch_event(&mut self.root, &path, &event, pos);
            self.hovered = Some(path);
            return;
        }
        self.dispatch_mouse_move(pos);
    }

    /// Mouse button pressed. `screen_y` is Y-down physical pixels.
    pub fn on_mouse_down(
        &mut self,
        screen_x: f64,
        screen_y: f64,
        button: MouseButton,
        mods: Modifiers,
    ) {
        let pos = self.flip_y(screen_x, screen_y);
        set_current_mouse_world(pos);
        let modal_path = active_modal_path(self.root.as_ref());
        let event = Event::MouseDown {
            pos,
            button,
            modifiers: mods,
        };
        if let Some(path) = modal_path {
            self.set_focus(None);
            if dispatch_event(&mut self.root, &path, &event, pos) == EventResult::Consumed {
                self.captured = Some(path);
            }
            return;
        }
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

        if let Some(mut path) = hit {
            let result = dispatch_event(&mut self.root, &path, &event, pos);
            if result == EventResult::Consumed {
                self.maybe_bring_to_front(&mut path);
                let capture_path = self.compute_hit(pos).unwrap_or(path);
                self.captured = Some(capture_path);
            }
        }
        // NO blanket request_draw.  Mouse-down on an inert area must not
        // cause a repaint.  Each widget that changes visual state in
        // response to a MouseDown (button press, window raise, focus
        // indicator on the focus-gained widget, etc.) is responsible for
        // calling `crate::animation::request_draw` itself.
    }

    /// Mouse button released. `screen_y` is Y-down.
    pub fn on_mouse_up(
        &mut self,
        screen_x: f64,
        screen_y: f64,
        button: MouseButton,
        mods: Modifiers,
    ) {
        let pos = self.flip_y(screen_x, screen_y);
        set_current_mouse_world(pos);
        let event = Event::MouseUp {
            pos,
            button,
            modifiers: mods,
        };
        if let Some(path) = active_modal_path(self.root.as_ref()) {
            self.captured = None;
            dispatch_event(&mut self.root, &path, &event, pos);
            return;
        }
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

    /// Key pressed. Delivered to the focused widget first, then to the visible
    /// widget tree as an unconsumed key if focus ignores it.
    pub fn on_key_down(&mut self, key: Key, mods: Modifiers) {
        if key == Key::Tab {
            self.advance_focus(!mods.shift);
            return;
        }
        let event = Event::KeyDown {
            key: key.clone(),
            modifiers: mods,
        };
        let result = if let Some(path) = active_modal_path(self.root.as_ref()) {
            dispatch_event(&mut self.root, &path, &event, Point::ORIGIN)
        } else if let Some(path) = self.focus.clone() {
            dispatch_event(&mut self.root, &path, &event, Point::ORIGIN)
        } else {
            EventResult::Ignored
        };
        if result != EventResult::Consumed {
            let result = dispatch_unconsumed_key(self.root.as_mut(), &key, mods);
            if result != EventResult::Consumed {
                if let Some(ref mut handler) = self.global_key_handler {
                    handler(key, mods);
                }
            }
        }
    }

    /// Key released. Delivered to the focused widget.
    pub fn on_key_up(&mut self, key: Key, mods: Modifiers) {
        let event = Event::KeyUp {
            key,
            modifiers: mods,
        };
        if let Some(path) = self.focus.clone() {
            dispatch_event(&mut self.root, &path, &event, Point::ORIGIN);
        }
    }

    /// Mouse wheel scrolled. `screen_y` is Y-down. `delta_y` positive = scroll up.
    /// `delta_x` positive = content moves right.
    pub fn on_mouse_wheel(&mut self, screen_x: f64, screen_y: f64, delta_y: f64) {
        self.on_mouse_wheel_xy_mods(screen_x, screen_y, 0.0, delta_y, Modifiers::default());
    }

    /// Mouse wheel with an explicit horizontal component (trackpad pan,
    /// shift+wheel via the platform harness).
    pub fn on_mouse_wheel_xy(&mut self, screen_x: f64, screen_y: f64, delta_x: f64, delta_y: f64) {
        self.on_mouse_wheel_xy_mods(screen_x, screen_y, delta_x, delta_y, Modifiers::default());
    }

    /// Mouse wheel with explicit horizontal component and modifier state.
    pub fn on_mouse_wheel_xy_mods(
        &mut self,
        screen_x: f64,
        screen_y: f64,
        delta_x: f64,
        delta_y: f64,
        modifiers: Modifiers,
    ) {
        let pos = self.flip_y(screen_x, screen_y);
        set_current_mouse_world(pos);
        let hit = active_modal_path(self.root.as_ref()).or_else(|| self.compute_hit(pos));
        let event = Event::MouseWheel {
            pos,
            delta_y,
            delta_x,
            modifiers,
        };
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
            let props_json = n
                .properties
                .iter()
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
            if i + 1 < nodes.len() {
                s.push(',');
            }
            s.push('\n');
        }
        s.push(']');
        s
    }

    /// Returns `true` if any widget currently holds keyboard focus.
    /// Used by the render loop to schedule cursor-blink repaints.
    pub fn has_focus(&self) -> bool {
        self.focus.is_some()
    }

    /// Call when the cursor leaves the window to clear hover state.
    pub fn on_mouse_leave(&mut self) {
        crate::cursor::reset_cursor_icon();
        self.dispatch_mouse_move(Point::new(-1.0, -1.0));
    }

    // --- Touch ingestion ---
    //
    // Raw touches go into the multi-touch gesture recogniser; widgets
    // read `current_multi_touch()` each frame.  Platform shells ALSO
    // route the first finger through the existing `on_mouse_*` entry
    // points so widgets that only understand mouse input keep working
    // without changes.  Coordinates are the same physical-pixel Y-down
    // units the mouse entry points accept.
    pub fn on_touch_start(
        &mut self,
        device: crate::touch_state::TouchDeviceId,
        id: crate::touch_state::TouchId,
        screen_x: f64,
        screen_y: f64,
        force: Option<f32>,
    ) {
        let pos = self.flip_y(screen_x, screen_y);
        self.touch_state.on_start(device, id, pos, force);
    }
    pub fn on_touch_move(
        &mut self,
        device: crate::touch_state::TouchDeviceId,
        id: crate::touch_state::TouchId,
        screen_x: f64,
        screen_y: f64,
        force: Option<f32>,
    ) {
        let pos = self.flip_y(screen_x, screen_y);
        self.touch_state.on_move(device, id, pos, force);
    }
    pub fn on_touch_end(
        &mut self,
        device: crate::touch_state::TouchDeviceId,
        id: crate::touch_state::TouchId,
    ) {
        self.touch_state.on_end_or_cancel(device, id);
    }
    pub fn on_touch_cancel(
        &mut self,
        device: crate::touch_state::TouchDeviceId,
        id: crate::touch_state::TouchId,
    ) {
        self.touch_state.on_end_or_cancel(device, id);
    }
    /// Current number of fingers down across all devices.  Used by
    /// widgets that want to know the gesture has *begun* before the
    /// first frame has had a chance to produce a delta (where
    /// `current_multi_touch()` may still be `None`).
    pub fn active_touch_count(&self) -> usize {
        self.touch_state.active_count()
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
            if idx >= children.len() {
                break;
            }
            node = &*children[idx];
            if node.type_name() == "Window" {
                // parent_path = clicked_path[..depth], win_idx = idx
                window_info = Some((clicked_path[..depth].to_vec(), idx));
            }
        }

        let (parent_path, win_idx) = match window_info {
            Some(x) => x,
            None => return,
        };

        // Check there's actually a sibling to leapfrog.
        let n = {
            let parent = widget_at_path(&mut self.root, &parent_path);
            parent.children().len()
        };
        if win_idx >= n - 1 {
            return;
        } // already at front

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
        if let Some(ref mut p) = self.focus {
            shift_path(p, depth, win_idx, new_idx);
        }
        if let Some(ref mut p) = self.hovered {
            shift_path(p, depth, win_idx, new_idx);
        }
        if let Some(ref mut p) = self.captured {
            shift_path(p, depth, win_idx, new_idx);
        }
    }

    #[inline]
    /// Convert a platform-supplied physical Y-down coordinate into the
    /// logical Y-up space the widget tree works in.  Divides by the current
    /// device scale factor (so mouse coords line up with the scaled paint
    /// transform) and flips Y against the cached logical viewport height.
    fn flip_y(&self, x: f64, y_down: f64) -> Point {
        let scale = crate::device_scale::device_scale().max(1e-6);
        let lx = x / scale;
        let ly_down = y_down / scale;
        Point::new(lx, self.viewport_height - ly_down)
    }

    fn compute_hit(&self, pos: Point) -> Option<Vec<usize>> {
        global_overlay_hit_path(self.root.as_ref(), pos)
            .or_else(|| hit_test_subtree(self.root.as_ref(), pos))
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
                    let clear = Event::MouseMove {
                        pos: Point::new(-1.0, -1.0),
                    };
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
        let current_idx = self
            .focus
            .as_ref()
            .and_then(|f| all.iter().position(|p| p == f));
        let next_idx = match current_idx {
            None => {
                if forward {
                    0
                } else {
                    all.len() - 1
                }
            }
            Some(i) => {
                if forward {
                    (i + 1) % all.len()
                } else {
                    if i == 0 {
                        all.len() - 1
                    } else {
                        i - 1
                    }
                }
            }
        };
        let next_path = all[next_idx].clone();
        self.set_focus(Some(next_path));
    }
}
