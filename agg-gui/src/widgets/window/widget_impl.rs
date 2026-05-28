use super::*;

impl Widget for Window {
    fn type_name(&self) -> &'static str {
        "Window"
    }
    /// External identity for z-order persistence, inspector lookup, etc.
    fn id(&self) -> Option<&str> {
        Some(&self.title)
    }

    fn is_visible(&self) -> bool {
        self.requested_visible() || self.fade_out_active.get()
    }

    /// Collapsed or closed windows should not keep the host loop awake.
    fn needs_draw(&self) -> bool {
        if !self.is_visible() || self.collapsed {
            return false;
        }
        self.children().iter().any(|c| c.needs_draw())
    }

    fn next_draw_deadline(&self) -> Option<web_time::Instant> {
        if !self.is_visible() || self.collapsed {
            return None;
        }
        let mut best: Option<web_time::Instant> = None;
        for c in self.children() {
            if let Some(t) = c.next_draw_deadline() {
                best = Some(match best {
                    Some(b) if b <= t => b,
                    _ => t,
                });
            }
        }
        best
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn margin(&self) -> Insets {
        self.base.margin
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn h_anchor(&self) -> HAnchor {
        self.base.h_anchor
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }
    fn min_size(&self) -> Size {
        self.base.min_size
    }
    fn max_size(&self) -> Size {
        self.base.max_size
    }

    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            (
                "backbuffer_kind",
                if self.use_gl_backbuffer {
                    "GlFbo".to_string()
                } else {
                    "None".to_string()
                },
            ),
            ("backbuffer_dirty", self.backbuffer.dirty.to_string()),
            (
                "backbuffer_repaints",
                self.backbuffer.repaint_count.to_string(),
            ),
            (
                "backbuffer_composites",
                self.backbuffer.composite_count.to_string(),
            ),
            (
                "backbuffer_size",
                format!("{}x{}", self.backbuffer.width, self.backbuffer.height),
            ),
        ]
    }

    /// Pop this window to the top of the parent `Stack` when the
    /// false→true visibility edge fires (see `layout`).
    fn take_raise_request(&mut self) -> bool {
        let pending = self.raise_request.get();
        self.raise_request.set(false);
        pending
    }

    fn set_bounds(&mut self, b: Rect) {
        if let Some(ref cell) = self.reset_to {
            if let Some(new_b) = cell.get() {
                self.bounds = new_b;
                self.pre_collapse_h = new_b.height;
                self.collapsed = false;
                cell.set(None);
                return;
            }
        }
        if self.bounds.width == 0.0 || self.bounds.height == 0.0 {
            self.bounds = b;
            self.pre_collapse_h = b.height;
        }
    }

    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn backbuffer_spec(&mut self) -> BackbufferSpec {
        if !self.use_gl_backbuffer {
            return BackbufferSpec::none();
        }
        if !self.is_visible() {
            let alpha = self.visibility_anim.value();
            if self.requested_visible() || alpha <= 0.001 {
                return BackbufferSpec::none();
            }
        }

        // Live-content windows self-invalidate every frame, except when
        // collapsed or hidden — no wasted work behind a folded title bar.
        if self.live_content && !self.collapsed && self.requested_visible() {
            self.backbuffer.invalidate();
        }

        let requested_visible = self.requested_visible();
        self.visibility_anim
            .set_target(if requested_visible { 1.0 } else { 0.0 });
        let alpha = self.visibility_anim.tick();
        if !requested_visible && alpha > 0.001 {
            self.fade_out_active.set(true);
        }
        if !requested_visible && alpha <= 0.001 {
            self.fade_out_active.set(false);
        }

        let (outset_left, outset_bottom, outset_right, outset_top) = Self::layer_outsets();
        BackbufferSpec {
            kind: BackbufferKind::GlFbo,
            cached: true,
            alpha,
            outsets: Insets {
                left: outset_left,
                right: outset_right,
                top: outset_top,
                bottom: outset_bottom,
            },
            rounded_clip: Some(CORNER_R),
        }
    }

    fn backbuffer_state_mut(&mut self) -> Option<&mut BackbufferState> {
        Some(&mut self.backbuffer)
    }

    /// Clip child painting to the content area (below the title bar).
    /// When collapsed bounds.height == TITLE_H so the content rect has zero height,
    /// preventing any child from drawing outside the visible title-bar strip.
    fn clip_children_rect(&self) -> Option<(f64, f64, f64, f64)> {
        if !self.is_visible() {
            return None;
        }
        let w = self.bounds.width;
        let content_h = (self.bounds.height - TITLE_H).max(0.0);
        // Clip to content area: y=0 (bottom) up to content_h, full width.
        Some((0.0, 0.0, w, content_h))
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        if !self.requested_visible() {
            return false;
        }
        if self.drag_mode != DragMode::None {
            return true;
        }
        let b = self.bounds();
        local_pos.x >= 0.0
            && local_pos.x <= b.width
            && local_pos.y >= 0.0
            && local_pos.y <= b.height
    }

    fn claims_pointer_exclusively(&self, local_pos: Point) -> bool {
        self.requested_visible()
            && (self.drag_mode != DragMode::None || self.resize_dir(local_pos).is_some())
    }

    fn layout(&mut self, available: Size) -> Size {
        // Drain the title-bar chevron's click flag — the chevron is a
        // real child widget that flips this `Rc<Cell<bool>>` when the
        // framework dispatches its MouseDown.  Acting on the flag here
        // (rather than in our own `on_event`) lets the child consume
        // the event normally instead of forcing the parent to manual
        // hit-test the chevron's coordinates.
        if self.title_bar.take_chevron_click() {
            self.toggle_collapse();
            self.last_title_click = None;
            crate::animation::request_draw();
        }
        // Rising-edge visibility detection requests a parent raise.
        let now_visible = self.requested_visible();
        // First-layout fit (visibility-cell-managed windows only):
        // a window restored as already-visible via `visible_cell` misses
        // the rising-edge branch below (last_visible was seeded to match
        // the cell), so without this its persisted bounds can land
        // outside the live viewport — the user sees the sidebar pill
        // highlighted but no window.  Gating on `visible_cell.is_some()`
        // keeps the auto-save invariant for plain `with_bounds(...)`
        // windows whose layout must never mutate persisted state.
        if now_visible && self.needs_initial_fit.get() && self.visible_cell.is_some() {
            self.fit_fully_to_canvas(available);
        }
        self.needs_initial_fit.set(false);
        if now_visible && !self.last_visible.get() {
            self.raise_request.set(true);
            if let Some(cb) = self.on_raised.as_mut() {
                cb(&self.title);
            }
            // Un-maximize on reopen.  Clicking a sidebar checkbox is "open
            // this window for use" — the user expects the window to come
            // up at its normal size, not still stretched to fill the canvas
            // from the last session's maximise.  Restore `pre_maximize_bounds`
            // which `toggle_maximize` saved when the user maximised.
            if self.maximized {
                self.bounds = self.pre_maximize_bounds;
                self.maximized = false;
            }
            self.fit_fully_to_canvas(available);
        }
        if now_visible {
            self.fade_out_active.set(false);
            self.visibility_anim.set_target(1.0);
        } else {
            self.visibility_anim.set_target(0.0);
            if self.visibility_anim.tick() <= 0.001 {
                self.fade_out_active.set(false);
            }
        }
        self.last_visible.set(now_visible);

        if !self.is_visible() {
            return Size::new(self.bounds.width, self.bounds.height);
        }

        if self.maximized && available.width > 0.0 && available.height > 0.0 {
            self.bounds = snap(Rect::new(0.0, 0.0, available.width, available.height));
            self.pre_collapse_h = self.bounds.height;
        }

        // Auto-size: measure the child's preferred size, then adopt it as the
        // new window size (pinning the top edge — Y-up → adjust `bounds.y` so
        // the title bar stays put when the height changes).  Skip while
        // collapsed: the user toggled a fixed TITLE_H height.
        //
        // We cap the measurement request by `child.max_size()` when finite
        // (otherwise by the canvas size): flex containers return their given
        // `available.width` rather than an intrinsic natural width, so without
        // a cap we'd produce an infinite/canvas-wide window.  Callers wanting
        // a content-fitted window set `with_max_size(Size::new(w, f64::MAX))`
        // on their root widget.
        if self.auto_size && !self.collapsed && !self.maximized {
            if let Some(child) = self.children.first_mut() {
                let max_sz = child.max_size();
                // `Size::MAX` uses `f64::MAX / 2.0` as its sentinel so
                // widgets can add-without-overflow (see `geometry.rs`).
                // That value is *technically* finite, so a plain
                // `.is_finite()` check wrongly treats it as a real cap
                // and cascades an ~`f64::MAX/2` width down to wrapped
                // Labels, whose bounds then blow up LCD-backbuffer
                // allocators to hundreds of GB.  Guard with a sane
                // threshold: anything ≥ `CAP_SENTINEL` means "no cap,
                // fall back to viewport-provided bounds".
                const CAP_SENTINEL: f64 = 1.0e18;
                // WIDTH is PINNED to the current bounds.width (seeded
                // by `with_bounds` and preserved across frames).
                // Why: wrapping Labels inside the content claim their
                // full available width — if we pass the viewport
                // width here, the window grows to the canvas on the
                // first frame and never shrinks back.  egui's
                // equivalent is `default_width`, which also pins.
                let cap_w = self.bounds.width.max(MIN_W);
                let cap_h = if max_sz.height.is_finite() && max_sz.height < CAP_SENTINEL {
                    max_sz.height
                } else {
                    available.height.max(MIN_H)
                };
                let pref = child.layout(Size::new(cap_w, cap_h));
                // Auto-size follows content in BOTH directions — so
                // the window can also shrink back down when the
                // inner Resize (or any other sizing widget) narrows.
                // Lower bound: `MIN_W`.  Upper bound: the parent-
                // provided `available.width` (main_area / canvas).
                // Matches egui where auto_sized tracks content size
                // symmetrically.
                let new_w = pref.width.max(MIN_W).min(available.width.max(MIN_W));
                let new_h = (pref.height + TITLE_H).min(cap_h + TITLE_H).max(MIN_H);
                let top = self.bounds.y + self.bounds.height;
                self.bounds.width = new_w;
                self.bounds.height = new_h;
                self.bounds.y = top - new_h;
                self.pre_collapse_h = new_h;
            }
        }

        // ── Tight-fit pre-pass ───────────────────────────────────
        //
        // When `with_tight_content_fit(true)` is set (and we're not
        // already in the auto_size block above, which handles both
        // axes), ask the content tree what minimum height it needs
        // at our current width and SNAP `bounds.height` to that.
        //
        // Uses `Widget::measure_min_height` rather than `layout` so
        // the result is independent of flex distribution — a
        // flex-fill widget like `TextArea` reports its true wrapped-
        // content height through `measure_min_height` even though
        // its `layout` returns the full slot.  This is what makes
        // egui's "no scroll, no clip, no whitespace" contract work
        // for windows whose content includes a flex-fill child.
        if self.tight_content_fit && !self.auto_size && !self.collapsed && !self.maximized {
            if let Some(child) = self.children.first() {
                let needed = child.measure_min_height(self.bounds.width);
                let new_h = (needed + TITLE_H).max(MIN_H);
                let top = self.bounds.y + self.bounds.height;
                self.bounds.height = new_h;
                self.bounds.y = top - new_h;
                self.last_content_natural_h.set(needed);
            }
        }

        // When collapsed, bounds.height == TITLE_H (set during toggle).
        let content_h = (self.bounds.height - TITLE_H).max(0.0);

        if let Some(child) = self.children.first_mut() {
            if !self.collapsed {
                let desired = child.layout(Size::new(self.bounds.width, content_h));
                let child_h = if child.v_anchor().is_stretch() {
                    content_h
                } else {
                    desired.height.clamp(
                        child.min_size().height,
                        child.max_size().height.min(content_h),
                    )
                };
                let child_y = if child.v_anchor().contains(VAnchor::BOTTOM) {
                    0.0
                } else if child.v_anchor().contains(VAnchor::CENTER) {
                    ((content_h - child_h) * 0.5).max(0.0)
                } else {
                    (content_h - child_h).max(0.0)
                };
                if (child_h - content_h).abs() > f64::EPSILON {
                    child.layout(Size::new(self.bounds.width, child_h));
                }
                child.set_bounds(Rect::new(0.0, child_y, self.bounds.width, child_h));
            }
            // When collapsed the child keeps its last bounds but is not visible
            // because hit_test returns false for the content area.
        }

        // Cache the child's required height via `measure_min_height`
        // so `apply_resize` and the tight-fit floor see a current
        // value EVEN when the content's `layout` returns the slot
        // size (the flex-fill case).  `Widget::measure_min_height`
        // walks the content tree and returns the actual content
        // requirement at the supplied width.
        if (self.tight_content_fit || self.floor_content_height) && !self.collapsed {
            if let Some(child) = self.children.first() {
                self.last_content_natural_h
                    .set(child.measure_min_height(self.bounds.width));
            }
        }

        // Position the title-bar strip at the top of the window and
        // give it a layout pass so the title label knows its size.
        let tb_y = self.bounds.height - TITLE_H;
        self.title_bar
            .set_bounds(Rect::new(0.0, tb_y, self.bounds.width, TITLE_H));
        self.title_bar.layout(Size::new(self.bounds.width, TITLE_H));

        // Record the canvas size — used by drag / resize / collapse clamp
        // paths that fire on USER ACTION.  We deliberately do NOT clamp
        // passively at layout time: platforms fire a Resized event with a
        // transient smaller size during fullscreen/maximize EXIT (Windows
        // notably), and if we clamped on shrink the auto-save would persist
        // those transient clamped bounds — the "all windows pushed down to
        // the same Y on next startup" bug.  Clamping only on user actions
        // (dragging a window, resize-handle, collapse toggle) keeps saved
        // state pinned to what the user actually chose.
        //
        // If a later OS shrink genuinely leaves a window's title bar out of
        // reach, the user can drag it back, use "Organize windows" to
        // retile, or a dedicated "reset positions" command.
        self.canvas_size = available;
        if let Some(ref cell) = self.position_cell {
            // When maximised, persist the UNDERLYING pre-maximise bounds,
            // not the stretched-to-canvas ones.  The maximized flag itself is
            // persisted separately so reloads restore the interaction state
            // without losing the user's last normal-size bounds.
            let save_bounds = if self.maximized {
                self.pre_maximize_bounds
            } else {
                self.bounds
            };
            cell.set(save_bounds);
        }
        if let Some(ref cell) = self.maximized_cell {
            cell.set(self.maximized);
        }

        // Snap-layout registration — every laid-out window declares
        // itself as a snap target so peers dragging nearby can pull
        // toward its edges.  Hidden / maximised windows opt out via
        // `Snappable::is_snap_target` and are removed from the
        // thread-local registry so their stale bounds don't yank
        // anyone around.
        {
            use crate::snap::Snappable;
            if self.is_snap_target() {
                crate::snap::register_target(self.snap_id, self.bounds);
            } else {
                crate::snap::unregister_target(self.snap_id);
            }
        }

        Size::new(self.bounds.width, self.bounds.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        super::paint::paint_window(self, ctx);
    }

    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        super::paint::paint_overlay(self, ctx);
    }

    fn finish_paint(&mut self, ctx: &mut dyn DrawCtx) {
        super::paint::finish_paint(self, ctx);
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if !self.requested_visible() {
            return EventResult::Ignored;
        }

        match event {
            Event::MouseMove { pos } => {
                let was_close = self.close_hovered;
                let was_max = self.maximize_hovered;
                let was_dir = self.hover_dir;
                self.close_hovered = self.in_close_button(*pos);
                self.maximize_hovered = self.in_maximize_button(*pos);

                match self.drag_mode {
                    DragMode::Move => {
                        let world = Point::new(pos.x + self.bounds.x, pos.y + self.bounds.y);
                        let dx = world.x - self.drag_start_world.x;
                        let dy = world.y - self.drag_start_world.y;
                        self.bounds.x = (self.drag_start_bounds.x + dx).round();
                        self.bounds.y = (self.drag_start_bounds.y + dy).round();
                        // Snap pass — runs only when the global flag
                        // is on.  Reads the thread-local target list
                        // populated by every other window's `layout`
                        // and writes the resulting visual guides for
                        // `SnapOverlay` to render.
                        self.apply_move_snap();
                        self.clamp_to_canvas();
                        self.hover_dir = None;
                        set_cursor_icon(CursorIcon::Grabbing);
                        crate::animation::request_draw_without_invalidation();
                        return EventResult::Ignored;
                    }
                    DragMode::Resize(dir) => {
                        let world = Point::new(pos.x + self.bounds.x, pos.y + self.bounds.y);
                        self.apply_resize(world);
                        self.apply_resize_snap(dir);
                        set_cursor_icon(resize_cursor(dir));
                        crate::animation::request_draw();
                        return EventResult::Consumed;
                    }
                    DragMode::None => {
                        // Track which edge/corner the cursor is hovering over so
                        // paint_overlay can draw the appropriate highlight.
                        self.hover_dir = self.resize_dir(*pos);
                        if let Some(dir) = self.hover_dir {
                            set_cursor_icon(resize_cursor(dir));
                        }
                    }
                }
                if was_close != self.close_hovered
                    || was_max != self.maximize_hovered
                    || was_dir != self.hover_dir
                {
                    crate::animation::request_draw();
                }
                EventResult::Ignored
            }

            Event::MouseDown { button, pos, .. }
                if matches!(*button, MouseButton::Left | MouseButton::Middle) =>
            {
                let is_left_click = *button == MouseButton::Left;
                // Press-to-raise: any direct press on this window brings it forward.
                self.raise_request.set(true);
                // Z-order changes are visible; repaint.
                crate::animation::request_draw();
                if let Some(cb) = self.on_raised.as_mut() {
                    cb(&self.title);
                }

                // Close button — highest priority.
                if is_left_click && self.in_close_button(*pos) {
                    self.visible = false;
                    self.visibility_anim.set_target(0.0);
                    if let Some(ref cell) = self.visible_cell {
                        cell.set(false);
                    }
                    if let Some(cb) = self.on_close.as_mut() {
                        cb();
                    }
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }

                // Maximize / Restore button.
                if is_left_click && self.in_maximize_button(*pos) {
                    self.toggle_maximize();
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }

                // Route the click into the title-bar sub-tree FIRST so
                // any child widget there (currently the chevron) gets a
                // chance to consume it.  `WindowTitleBar` lives outside
                // `Window.children` because the body content owns that
                // slot, so the framework's normal hit-test pass never
                // descends into it — we run the framework's hit-test
                // + dispatch helpers manually on the sub-tree instead.
                if is_left_click && self.in_title_bar(*pos) {
                    let tb_bounds = self.title_bar.bounds();
                    let tb_local = Point::new(pos.x - tb_bounds.x, pos.y - tb_bounds.y);
                    if let Some(path) = crate::widget::hit_test_subtree(&self.title_bar, tb_local) {
                        // Path could be empty (clicked the bar itself
                        // but not a child) — skip in that case so the
                        // title-drag handling further down still runs.
                        if !path.is_empty() {
                            // Preserve modifiers from the original event.
                            let mods = match event {
                                Event::MouseDown { modifiers, .. } => *modifiers,
                                _ => Default::default(),
                            };
                            let translated = Event::MouseDown {
                                pos: tb_local,
                                button: *button,
                                modifiers: mods,
                            };
                            let result = crate::widget::dispatch_event_dyn(
                                &mut self.title_bar,
                                &path,
                                &translated,
                                tb_local,
                            );
                            if result == EventResult::Consumed {
                                // Chevron flag is drained in `layout`,
                                // but we also want this frame to redraw
                                // before that.
                                crate::animation::request_draw();
                                return EventResult::Consumed;
                            }
                        }
                    }
                }

                // Resize edge — check before title bar to handle corner overlap.
                if let Some(dir) = self.resize_dir(*pos) {
                    // Only start resize if not in the close button area and not a pure title bar drag.
                    // The N edge overlaps the title bar — prefer resize over drag from the top N px.
                    let world = Point::new(pos.x + self.bounds.x, pos.y + self.bounds.y);
                    self.drag_mode = DragMode::Resize(dir);
                    self.drag_start_world = world;
                    self.drag_start_bounds = self.bounds;
                    return EventResult::Consumed;
                }

                // Title bar drag + double-click maximize.
                if self.in_title_bar(*pos) {
                    // Double-click detection.
                    let is_double = if is_left_click {
                        let now = Instant::now();
                        self.last_title_click
                            .map(|t| now.duration_since(t).as_millis() < DBL_CLICK_MS)
                            .unwrap_or(false)
                    } else {
                        false
                    };

                    if is_double {
                        // Windows convention: double-click title bar toggles
                        // maximize / restore.  Collapse/expand lives on the
                        // chevron button to the left.
                        self.toggle_maximize();
                        self.last_title_click = None;
                        crate::animation::request_draw();
                    } else {
                        if is_left_click {
                            self.last_title_click = Some(Instant::now());
                        }
                        let world = Point::new(pos.x + self.bounds.x, pos.y + self.bounds.y);
                        self.drag_mode = DragMode::Move;
                        self.drag_start_world = world;
                        self.drag_start_bounds = self.bounds;
                    }
                    return EventResult::Consumed;
                }

                // Click on content area: consume so it doesn't fall through.
                if is_left_click && !self.collapsed {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }

            Event::MouseUp {
                button: MouseButton::Left | MouseButton::Middle,
                ..
            } => {
                let was_dragging = self.drag_mode != DragMode::None;
                self.drag_mode = DragMode::None;
                if was_dragging {
                    // Drag ended — wipe the snap guides so the
                    // overlay clears.  Cheap no-op when snapping was
                    // off (guide buffer was already empty).
                    crate::snap::clear_guides();
                    crate::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }

            _ => EventResult::Ignored,
        }
    }
}
