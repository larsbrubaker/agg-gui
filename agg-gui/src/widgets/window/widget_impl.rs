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

    /// A collapsed window paints only its title bar — nothing inside the
    /// content area is visible, so no child can legitimately request a
    /// repaint.  Closing (`is_visible` false) also short-circuits, matching
    /// the default trait impl.  Without these overrides a cursor blink or
    /// hover tween inside a collapsed/closed window would keep the host
    /// loop awake despite being invisible.
    fn needs_paint(&self) -> bool {
        if !self.is_visible() || self.collapsed {
            return false;
        }
        self.children().iter().any(|c| c.needs_paint())
    }

    fn next_paint_deadline(&self) -> Option<web_time::Instant> {
        if !self.is_visible() || self.collapsed {
            return None;
        }
        let mut best: Option<web_time::Instant> = None;
        for c in self.children() {
            if let Some(t) = c.next_paint_deadline() {
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

    fn compositing_layer(&mut self) -> Option<CompositingLayer> {
        if !self.is_visible() {
            let alpha = self.visibility_anim.value();
            if self.requested_visible() || alpha <= 0.001 {
                return None;
            }
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
        Some(CompositingLayer::new(
            outset_left,
            outset_bottom,
            outset_right,
            outset_top,
            alpha,
        ))
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

    fn layout(&mut self, available: Size) -> Size {
        // Rising-edge visibility detection → request parent raise.  The
        // sidebar toggles `visible_cell`; we observe the transition here
        // and set `raise_request`, which the parent `Stack` drains on its
        // next layout (one-frame delay, invisible to the user).
        let now_visible = self.requested_visible();
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
                child.layout(Size::new(self.bounds.width, content_h));
                child.set_bounds(Rect::new(0.0, 0.0, self.bounds.width, content_h));
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
            // not the stretched-to-canvas ones.  Maximise is an interaction
            // state, not a saved size: we want cold reloads to come up at
            // the user's last chosen "real" size, then let them re-maximise
            // if they want.  Matches native window-manager behaviour.
            let save_bounds = if self.maximized {
                self.pre_maximize_bounds
            } else {
                self.bounds
            };
            cell.set(save_bounds);
        }

        Size::new(self.bounds.width, self.bounds.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.is_visible() {
            return;
        }

        let v = ctx.visuals();
        let w = self.bounds.width;
        // bounds.height == TITLE_H when collapsed (adjusted on toggle).
        let h = self.bounds.height;

        // Drop shadow — stacked rounded rects approximating a Gaussian blur.
        // Outer layers inflate outward and fade with a (1−t)² falloff; drawn
        // outside-in so the denser core overlays the softer halo.
        let base = v.window_shadow;
        for i in (0..SHADOW_STEPS).rev() {
            let t = i as f64 / SHADOW_STEPS as f64;
            let infl = t * SHADOW_BLUR;
            let falloff = (1.0 - t).powi(2) as f32;
            let alpha = base.a * falloff / SHADOW_STEPS as f32 * 6.0;
            ctx.set_fill_color(Color::rgba(base.r, base.g, base.b, alpha));
            ctx.begin_path();
            ctx.rounded_rect(
                SHADOW_DX - infl,
                -SHADOW_DY - infl,
                w + 2.0 * infl,
                h + 2.0 * infl,
                CORNER_R + infl,
            );
            ctx.fill();
        }

        ctx.set_layer_rounded_clip(0.0, 0.0, w, h, CORNER_R);

        // Window body.
        ctx.set_fill_color(v.window_fill);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, CORNER_R);
        ctx.fill();

        // Sync the title-bar sub-widget's display state for this frame
        // and paint it.  Positioning was done in `layout`; we just need
        // to hand it the per-frame interaction snapshot and dispatch
        // through `paint_subtree` so the ancestor-chain stack gets the
        // WindowTitleBar entry (background_color = window_title_fill).
        {
            let mut st = self.title_state.borrow_mut();
            st.bar_color = if self.drag_mode == DragMode::Move {
                v.window_title_fill_drag
            } else {
                v.window_title_fill
            };
            st.title_color = v.window_title_text;
            st.collapsed = self.collapsed;
            st.maximized = self.maximized;
            st.close_hovered = self.close_hovered;
            st.maximize_hovered = self.maximize_hovered;
        }
        let tb_bounds = self.title_bar.bounds();
        ctx.save();
        ctx.translate(tb_bounds.x, tb_bounds.y);
        paint_subtree(&mut self.title_bar, ctx);
        ctx.restore();

        // Outer border — on top of the title bar so the rounded corners
        // cleanly frame both body and title region.
        ctx.set_fill_color(v.window_fill); // restore default fill — stroke follows
        ctx.set_stroke_color(v.window_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.5, 0.5, (w - 1.0).max(0.0), (h - 1.0).max(0.0), CORNER_R);
        ctx.stroke();
    }

    // paint_overlay: draws the resize handle dots + edge highlights on top of content.
    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.is_visible() || self.collapsed {
            return;
        }
        // Skip all resize-related chrome when the window can't be resized,
        // so an auto-sized or `.resizable(false)` window doesn't look
        // deceptively interactive.
        if !self.resizable || self.auto_size {
            return;
        }
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // ── SE corner drag grip (3 diagonal lines, egui-style) ───────────────
        // Only shown when both axes are resizable; for uni-axis resizable
        // windows the SE grip would suggest a capability that isn't there.
        if self.resizable_h && self.resizable_v {
            let is_se_active = matches!(self.drag_mode, DragMode::Resize(ResizeDir::SE));
            let is_se_hover = self.hover_dir == Some(ResizeDir::SE);
            let grip_color = if is_se_active {
                v.window_resize_active
            } else if is_se_hover {
                v.window_resize_hover
            } else {
                v.window_stroke
            };
            ctx.set_stroke_color(grip_color);
            ctx.set_line_width(1.5);
            let m = 3.0_f64; // margin from corner edge
            for i in 1..=3_i32 {
                let off = i as f64 * 4.0 + m;
                ctx.begin_path();
                ctx.move_to(w - off, m);
                ctx.line_to(w - m, off);
                ctx.stroke();
            }
        }

        // ── Resize edge / corner highlight ────────────────────────────────────
        // Determine the highlighted direction and whether it is actively dragging.
        let (highlight, is_active) = match self.drag_mode {
            DragMode::Resize(d) => (Some(d), true),
            DragMode::Move => (None, false), // no edge highlight while moving
            DragMode::None => (self.hover_dir, false),
        };
        let dir = match highlight {
            Some(d) => d,
            None => return,
        };

        let color = if is_active {
            v.window_resize_active
        } else {
            v.window_resize_hover
        };
        ctx.set_stroke_color(color);
        ctx.set_line_width(2.0);

        // Which edges to highlight (derived from direction).
        let (top, bottom, left, right) = match dir {
            ResizeDir::N => (true, false, false, false),
            ResizeDir::S => (false, true, false, false),
            ResizeDir::E => (false, false, false, true),
            ResizeDir::W => (false, false, true, false),
            ResizeDir::NE => (true, false, false, true),
            ResizeDir::NW => (true, false, true, false),
            ResizeDir::SE => (false, true, false, true),
            ResizeDir::SW => (false, true, true, false),
        };

        // Segments run between the rounded-corner tangent points.
        let cr = CORNER_R;
        if top {
            ctx.begin_path();
            ctx.move_to(cr, h);
            ctx.line_to(w - cr, h);
            ctx.stroke();
        }
        if bottom {
            ctx.begin_path();
            ctx.move_to(cr, 0.0);
            ctx.line_to(w - cr, 0.0);
            ctx.stroke();
        }
        if left {
            ctx.begin_path();
            ctx.move_to(0.0, cr);
            ctx.line_to(0.0, h - cr);
            ctx.stroke();
        }
        if right {
            ctx.begin_path();
            ctx.move_to(w, cr);
            ctx.line_to(w, h - cr);
            ctx.stroke();
        }
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
                        self.clamp_to_canvas();
                        self.hover_dir = None;
                        set_cursor_icon(CursorIcon::Grabbing);
                        crate::animation::request_tick();
                        return EventResult::Consumed;
                    }
                    DragMode::Resize(dir) => {
                        let world = Point::new(pos.x + self.bounds.x, pos.y + self.bounds.y);
                        self.apply_resize(world);
                        set_cursor_icon(resize_cursor(dir));
                        crate::animation::request_tick();
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
                    crate::animation::request_tick();
                }
                EventResult::Ignored
            }

            Event::MouseDown {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                // Click-to-raise — any left click that reaches this Window
                // (hit-test routed it here in reverse paint order, so we
                // ARE the topmost widget under the cursor in the stack
                // sense) requests a raise.  Classic window-manager
                // behaviour: clicking anywhere on a window pops it to the
                // top of the z-order.  Consumed by `Stack::layout` on the
                // next frame via `take_raise_request`; one-frame visual
                // delay is invisible in practice.
                self.raise_request.set(true);
                // Z-order changes are visible; repaint.
                crate::animation::request_tick();
                if let Some(cb) = self.on_raised.as_mut() {
                    cb(&self.title);
                }

                // Close button — highest priority.
                if self.in_close_button(*pos) {
                    self.visible = false;
                    self.visibility_anim.set_target(0.0);
                    if let Some(ref cell) = self.visible_cell {
                        cell.set(false);
                    }
                    if let Some(cb) = self.on_close.as_mut() {
                        cb();
                    }
                    crate::animation::request_tick();
                    return EventResult::Consumed;
                }

                // Maximize / Restore button.
                if self.in_maximize_button(*pos) {
                    self.toggle_maximize();
                    crate::animation::request_tick();
                    return EventResult::Consumed;
                }

                // Collapse / expand chevron.
                if self.in_chevron_button(*pos) {
                    self.toggle_collapse();
                    // Null out the double-click timer so clicking the
                    // chevron then quickly clicking the bar doesn't
                    // trigger a maximize toggle.
                    self.last_title_click = None;
                    crate::animation::request_tick();
                    return EventResult::Consumed;
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
                    let now = Instant::now();
                    let is_double = self
                        .last_title_click
                        .map(|t| now.duration_since(t).as_millis() < DBL_CLICK_MS)
                        .unwrap_or(false);

                    if is_double {
                        // Windows convention: double-click title bar toggles
                        // maximize / restore.  Collapse/expand lives on the
                        // chevron button to the left.
                        self.toggle_maximize();
                        self.last_title_click = None;
                        crate::animation::request_tick();
                    } else {
                        self.last_title_click = Some(now);
                        let world = Point::new(pos.x + self.bounds.x, pos.y + self.bounds.y);
                        self.drag_mode = DragMode::Move;
                        self.drag_start_world = world;
                        self.drag_start_bounds = self.bounds;
                    }
                    return EventResult::Consumed;
                }

                // Click on content area: consume so it doesn't fall through.
                if !self.collapsed {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }

            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                let was_dragging = self.drag_mode != DragMode::None;
                self.drag_mode = DragMode::None;
                if was_dragging {
                    crate::animation::request_tick();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }

            _ => EventResult::Ignored,
        }
    }
}
