use super::*;

mod gesture;
mod keyboard;
mod pointer;
mod touch;
mod tree_paths;
use tree_paths::{collect_focusable, widget_at_path, widget_at_path_ref};

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
    /// Gesture-captured widget path. Set on the frame a multi-touch gesture
    /// begins if the widget under the initial centroid consumes
    /// `Event::MultiTouch`; the whole gesture then routes there (capture
    /// semantics) even as the centroid drifts. Cleared when the aggregate
    /// returns to `None`. See [`super::app::gesture`].
    gesture_captured: Option<Vec<usize>>,
    /// Whether last frame's aggregate was `Some` — lets the gesture router
    /// detect the `None`→`Some` start edge (when it hit-tests) versus an
    /// ongoing gesture (when it delivers to the captured path only).
    gesture_in_progress: bool,
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
    /// Primary-finger mouse emulation (tap = left click, drag =
    /// middle-drag pan).  Fed by the same `on_touch_*` entry points;
    /// its commands are replayed through `on_mouse_*` so shells only
    /// ever forward raw touches.  See [`crate::touch_emulation`].
    touch_mouse_emu: crate::touch_emulation::TouchMouseEmu,
    /// Last `async_state_epoch` `App::paint` observed.  At the top of
    /// each paint, if the current epoch differs we explicitly mark
    /// every widget dirty via `mark_subtree_dirty`, so a freshly-
    /// loaded image (or any other async result that landed outside
    /// the event-dispatch dirty-propagation path) lands in newly-
    /// rasterised retained backbuffers, not the previous frame's
    /// stale FBO contents.
    last_async_state_epoch: u64,
}

impl App {
    /// Create a new `App` with `root` as the root widget.
    pub fn new(root: Box<dyn Widget>) -> Self {
        Self {
            root,
            focus: None,
            hovered: None,
            captured: None,
            gesture_captured: None,
            gesture_in_progress: false,
            viewport_height: 1.0,
            viewport_size: Size::new(1.0, 1.0),
            global_key_handler: None,
            touch_state: crate::touch_state::TouchState::new(),
            touch_mouse_emu: crate::touch_emulation::TouchMouseEmu::new(),
            last_async_state_epoch: 0,
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

    /// Whether the focused widget accepts typed text (a `TextField`, `TextArea`,
    /// `RichTextEdit`, or a `DragValue` in its edit mode).
    ///
    /// Web shells use this to decide when to focus the hidden DOM `<textarea>`
    /// that backs IME composition *and* clipboard events: without it focused,
    /// the browser never delivers `copy` / `cut` / `paste` events to a
    /// canvas-only app, so the in-canvas editors get no clipboard bridge.
    ///
    /// Delegates straight to [`Widget::accepts_text_input`] on the focused
    /// widget rather than matching type-name strings, so any new editor that
    /// overrides the trait is enrolled automatically (a hardcoded name list
    /// would silently miss it and reintroduce exactly the wasm clipboard bug).
    pub fn focused_is_text_input(&self) -> bool {
        self.focus
            .as_deref()
            .is_some_and(|path| widget_at_path_ref(self.root.as_ref(), path).accepts_text_input())
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
        // Effective scale combines hardware DPR with the UX zoom
        // factor — mobile platforms set ux_scale ≈ 1.7 so widgets at
        // their natural logical size read comfortably at arm's length.
        let scale = crate::ux_scale::effective_scale().max(1e-6);
        let logical = Size::new(viewport.width / scale, viewport.height / scale);
        self.viewport_height = logical.height;
        self.viewport_size = logical;
        set_current_viewport(logical);
        // Fresh safe-area for this frame. The on-screen keyboard is the
        // one library-owned edge obstruction, so it reserves its strip
        // here; app chrome (rails, trays) reserves via
        // `widgets::ReserveInset` during the tree layout below.
        crate::overlay_insets::begin_frame();
        if crate::widgets::on_screen_keyboard::is_visible() {
            crate::overlay_insets::reserve(crate::layout_props::Insets {
                bottom: crate::widgets::on_screen_keyboard::target_panel_height(logical.width),
                ..crate::layout_props::Insets::default()
            });
        }
        self.root
            .set_bounds(Rect::new(0.0, 0.0, logical.width, logical.height));
        self.root.layout(logical);
        self.apply_pending_focus();
        // Re-evaluate the keyboard-avoidance lift against FRESH bounds
        // (see `keyboard_scroll::relift_after_layout` for the why).
        crate::widget::keyboard_scroll::relift_after_layout(
            self.focus.as_deref(),
            logical.width,
            self.root.as_mut(),
        );
    }

    /// Service a pending programmatic focus request
    /// ([`crate::focus::request_focus`]). Runs at the end of [`layout`] so the
    /// tree (and thus the set of focusable widgets) reflects any visibility
    /// change made in the same handler that requested focus. Moves focus to
    /// the focusable widget whose [`Widget::focus_id`] matches; no-op when
    /// there's no request or no match.
    fn apply_pending_focus(&mut self) {
        let Some(id) = crate::focus::take_focus_request() else {
            return;
        };
        let mut all: Vec<Vec<usize>> = Vec::new();
        collect_focusable(self.root.as_ref(), &mut Vec::new(), &mut all);
        let target = all
            .into_iter()
            .find(|p| widget_at_path_ref(self.root.as_ref(), p).focus_id() == Some(id));
        if let Some(path) = target {
            self.set_focus(Some(path));
        }
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
        // Async-state dirty walk: an image load (or other async source)
        // that finished outside event dispatch bumped
        // `async_state_epoch`.  Walk the whole tree and mark every
        // widget dirty so retained backbuffers re-rasterise on this
        // frame — without this, the freshly-decoded pixels would land
        // inside a Window FBO whose cache check sees no other change
        // and composites the previous frame's stale bitmap.  The
        // explicit walk replaces a brittle "compare an extra epoch
        // inside every cache" mechanism with a single deterministic
        // hook at the start of paint.
        let async_epoch = crate::animation::async_state_epoch();
        if async_epoch != self.last_async_state_epoch {
            tree::mark_subtree_dirty(self.root.as_mut());
            self.last_async_state_epoch = async_epoch;
        }
        let viewport = self.viewport_size;
        crate::widgets::combo_box::begin_combo_popup_frame(viewport);
        crate::widgets::tooltip::begin_tooltip_frame(viewport);
        // Central tooltip pass: find the deepest hovered widget carrying a
        // `with_tooltip` string and drive the app-wide tip state machine. Any
        // resulting tip is submitted to the tooltip queue and painted below in
        // the global-overlay drain, above all clips.
        self.drive_tooltip_controller();
        // Recompute the multi-touch aggregate once per paint and publish
        // to the thread-local — widgets read it during `on_event` or
        // `paint` without an explicit `&App` reference.
        self.touch_state.update_gesture();
        crate::touch_state::set_current(self.touch_state.current());
        // Route the aggregate as a captured `Event::MultiTouch` before the
        // paint traversal runs: a consuming widget marks its cached window
        // subtree dirty (via the normal Consumed → request_draw → epoch-bump
        // → mark_dirty chain), so the fold it applied here re-rasters this
        // same frame.  See `super::app::gesture`.
        self.dispatch_gesture();
        // Tick the keyboard-driven lift once per paint.  Translates
        // the widget tree (and its global overlays) upward by `lift`
        // pixels so a focused field doesn't disappear behind the
        // soft-keyboard panel; the panel itself paints unlifted so
        // it always sits at the bottom of the viewport.
        let lift = super::keyboard_scroll::tick_lift();
        // Use the combined device-DPR × UX-zoom scale so widgets at
        // their natural logical size render at the right physical pixel
        // count *and* at a comfortable on-screen footprint.
        let scale = crate::ux_scale::effective_scale();
        if (scale - 1.0).abs() > 1e-6 {
            ctx.save();
            ctx.scale(scale, scale);
            super::keyboard_scroll::paint_lifted_tree(self.root.as_mut(), ctx, viewport, lift);
            crate::widgets::on_screen_keyboard::paint_software_keyboard(ctx, viewport);
            ctx.restore();
        } else {
            super::keyboard_scroll::paint_lifted_tree(self.root.as_mut(), ctx, viewport, lift);
            crate::widgets::on_screen_keyboard::paint_software_keyboard(ctx, viewport);
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
        self.root.needs_draw()
            || crate::animation::wants_draw()
            || crate::widgets::on_screen_keyboard::needs_draw()
            || super::keyboard_scroll::is_lift_animating()
    }

    /// Pump pending synthetic keys back through [`Self::on_key_down`]
    /// AND apply any pending dismiss request — the close key on the
    /// keyboard panel clears focus, which then drops the
    /// keyboard-aware screen lift via `notify_focus_change`.
    fn drain_keyboard_synthetic_keys(&mut self) {
        let pending = crate::widgets::on_screen_keyboard::drain_synthetic_keys();
        for (key, mods) in pending {
            self.on_key_down(key, mods);
        }
        if crate::widgets::on_screen_keyboard::take_dismiss_request() {
            self.set_focus(None);
        }
    }

    /// Test-only mirror of the end-of-event-loop drain.
    #[cfg(test)]
    pub fn drain_keyboard_events_for_test(&mut self) {
        self.drain_keyboard_synthetic_keys();
    }

    /// Earliest scheduled draw deadline across the visible widget tree.
    /// Hosts translate `Some(t)` into `ControlFlow::WaitUntil(t)` so that
    /// e.g. a text field's cursor blink wakes the loop exactly at the flip
    /// boundary.  Invisible subtrees contribute nothing.
    pub fn next_draw_deadline(&self) -> Option<web_time::Instant> {
        // Two schedule channels feed the host's WaitUntil: per-widget
        // deadlines (cursor blink) from the tree walk, and the global
        // `animation::request_draw_after` thread-local. Both are read
        // non-destructively so a host can re-arm `WaitUntil` idempotently
        // every idle iteration — an intervening non-repainting event can no
        // longer strand the scheduled wake. A due global deadline surfaces
        // separately through `animation::wants_draw`. Serve the earliest.
        let widget = self.root.next_draw_deadline();
        let scheduled = crate::animation::peek_next_draw_deadline();
        match (widget, scheduled) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (deadline, None) | (None, deadline) => deadline,
        }
    }

    // --- Platform event ingestion ---
    //
    // Hosts pass raw physical-pixel coordinates (e.g. `e.clientX * devicePixelRatio`
    // in wasm, or `WindowEvent::CursorMoved.position` on native).  These methods
    // divide by the current device scale factor and flip Y so widget code sees
    // logical Y-up coordinates matching the layout pass.

    /// Mouse wheel scrolled. `screen_y` is Y-down. Convention matches
    /// `winit` / `WheelEvent`: positive `delta_y` = wheel rotated
    /// forward = user wants to see content ABOVE the current view.
    /// Scroll containers DECREASE their offset when `delta_y` is
    /// positive. Positive `delta_x` = see content to the LEFT.
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
        let pos = super::keyboard_scroll::lift_to_world(self.flip_y(screen_x, screen_y));
        set_current_mouse_world(pos);
        let hit = active_modal_path(self.root.as_ref())
            .map(|path| self.extend_modal_path(&path, pos))
            .or_else(|| self.compute_hit(pos));
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

    /// `true` while a widget is actively capturing the pointer — i.e. the
    /// user is mid-drag (a window edge, slider thumb, scrollbar, etc.).
    /// Used by the demo harness to throttle expensive per-frame snapshots
    /// (the inspector tree walk) during interactions; the snapshot can
    /// safely defer until the user releases without changing the visible
    /// outcome (the underlying widget tree topology doesn't change during
    /// a drag, only the widgets' bounds).
    pub fn has_captured_pointer(&self) -> bool {
        self.captured.is_some()
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

    /// Native drag-and-drop landed `paths` on the window at the given
    /// screen position. Dispatches an [`Event::FileDropped`] to the
    /// widget under the cursor (same hit-test path as `on_mouse_down`),
    /// so a widget can opt in by handling the event in `on_event`.
    ///
    /// Native shells typically receive one path per `DroppedFile` event
    /// from winit; they may forward each separately, or batch a single
    /// drag gesture into one call. The widget receives `paths` as-is.
    pub fn on_file_dropped(
        &mut self,
        screen_x: f64,
        screen_y: f64,
        paths: Vec<std::path::PathBuf>,
    ) {
        if paths.is_empty() {
            return;
        }
        let pos = super::keyboard_scroll::lift_to_world(self.flip_y(screen_x, screen_y));
        let event = Event::FileDropped { pos, paths };
        let hit = self.compute_hit(pos);
        let consumed = match hit {
            Some(path) => dispatch_event(&mut self.root, &path, &event, pos),
            // No hit target: dispatch to the root anyway so app-level
            // handlers (e.g. "open the dropped .atmr project") can run
            // even when the user drops on chrome rather than canvas.
            None => dispatch_event(&mut self.root, &[], &event, pos),
        }
        .is_consumed();
        if !consumed {
            // The widget under the drop point ignored the files. Offer
            // the event to the rest of the tree before giving up — the
            // reported position is often wrong through no fault of the
            // user (winit's Windows backend discards the OLE drop point
            // and emits no CursorMoved during the drag, so shells fall
            // back to the last pre-drag cursor position). A drop must
            // find the app's file handler even when it "lands" on
            // chrome or a sibling pane.
            super::tree::dispatch_event_broadcast(&mut self.root, &event, pos);
        }
        crate::animation::request_draw();
    }

    // --- Touch ingestion ---
    //
    // Raw touches go into the multi-touch gesture recogniser; widgets
    // read `current_multi_touch()` each frame.  The primary finger is
    // ALSO replayed through the `on_mouse_*` entry points by the
    // core-owned `TouchMouseEmu` (see `crate::touch_emulation`), so
    // widgets that only understand mouse input keep working and
    // platform shells forward raw touches only.  Coordinates are the
    // same physical-pixel Y-down units the mouse entry points accept.
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
    /// logical Y-up SCREEN space (unlifted).  Global overlays such as
    /// the on-screen keyboard panel test against this; widget-tree
    /// dispatch then calls
    /// [`keyboard_scroll::lift_to_world`](super::keyboard_scroll::lift_to_world)
    /// to drop into the lifted frame.
    fn flip_y(&self, x: f64, y_down: f64) -> Point {
        // Same effective scale used for layout / paint so event coords
        // arrive in the same logical space the widget tree was laid out in.
        let scale = crate::ux_scale::effective_scale().max(1e-6);
        let lx = x / scale;
        let ly_down = y_down / scale;
        Point::new(lx, self.viewport_height - ly_down)
    }

    fn compute_hit(&self, pos: Point) -> Option<Vec<usize>> {
        global_overlay_hit_path(self.root.as_ref(), pos)
            .or_else(|| hit_test_subtree(self.root.as_ref(), pos))
    }

    /// Feed the central tooltip controller from the current hover path: the
    /// deepest hovered widget whose [`Widget::tooltip_text`] is `Some` supplies
    /// the tip (its index-path identity + text), anchored at the pointer.
    fn drive_tooltip_controller(&self) {
        let target = self
            .hovered
            .as_deref()
            .and_then(|path| deepest_tipped(self.root.as_ref(), path));
        crate::widgets::tooltip::controller::drive(target, current_mouse_world());
    }

    /// Test hook: run the central tooltip pass without a full paint (no
    /// `DrawCtx` needed). Mirrors what [`paint`](Self::paint) does after
    /// `begin_tooltip_frame`, so a test can drive hover + clock + this and then
    /// assert controller state.
    #[cfg(test)]
    pub fn update_tooltips_for_test(&self) {
        crate::widgets::tooltip::begin_tooltip_frame(self.viewport_size);
        self.drive_tooltip_controller();
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
        if let Some(new) = new_path.clone() {
            dispatch_event(&mut self.root, &new, &Event::FocusGained, Point::ORIGIN);
        }
        super::keyboard_scroll::notify_focus_change(
            new_path.as_deref(),
            self.viewport_size.width,
            self.root.as_mut(),
        );
    }

    /// Lift the focused widget above the on-screen keyboard panel so
    /// typing never disappears behind it.  No-op when already visible.
    pub fn ensure_focused_visible_above_keyboard(&mut self) {
        super::keyboard_scroll::ensure_focused_visible_above_keyboard(
            self.focus.as_deref(),
            self.viewport_size.width,
            self.root.as_mut(),
        );
    }

    /// Focus the first focusable widget in paint order.
    ///
    /// Apps whose root is a game/canvas widget need this at startup:
    /// [`on_key_up`](Self::on_key_up) only routes to the focused widget
    /// (there is no unconsumed-key fallback for releases), so held-key
    /// state tracking breaks until the user clicks or tabs. Calling this
    /// right after building the app gives that widget key delivery from
    /// the first frame. No-op when nothing is focusable.
    pub fn focus_first(&mut self) {
        if self.focus.is_none() {
            self.advance_focus(true);
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

/// Walk the hover `path` from `root` and return the **deepest** widget along it
/// whose [`Widget::tooltip_text`] is `Some`, as `(identity_path, text)`. The
/// identity is the path prefix down to that widget, which the controller uses
/// to detect target changes (moving to a different tipped control ⇒ reshow).
/// A shallower tipped ancestor is used only when no deeper descendant has a tip.
fn deepest_tipped(root: &dyn Widget, path: &[usize]) -> Option<(Vec<usize>, String)> {
    let mut widget: &dyn Widget = root;
    let mut best: Option<(usize, String)> = None;
    if let Some(t) = widget.tooltip_text() {
        best = Some((0, t.to_string()));
    }
    for (i, &idx) in path.iter().enumerate() {
        let Some(child) = widget.children().get(idx) else {
            break;
        };
        widget = child.as_ref();
        if let Some(t) = widget.tooltip_text() {
            best = Some((i + 1, t.to_string()));
        }
    }
    best.map(|(depth, text)| (path[..depth].to_vec(), text))
}
