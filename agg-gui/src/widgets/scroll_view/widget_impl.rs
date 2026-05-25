use super::*;

impl Widget for ScrollView {
    fn type_name(&self) -> &'static str {
        "ScrollView"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn needs_draw(&self) -> bool {
        if !self.is_visible() {
            return false;
        }
        self.scrollbar_animation_active()
            || self.painted_style_epoch.get() != current_scroll_style_epoch()
            || self.children().iter().any(|c| c.needs_draw())
    }

    /// Absorb part of a keyboard-driven "lift content upward by N
    /// pixels" request from `App::ensure_focused_visible_above_keyboard`.
    ///
    /// In Y-up screen space, increasing `v.offset` by `D` shifts this
    /// scroll view's child upward by `D` pixels (see the formula
    /// `child_y = vh - content + offset` in [`layout`]).  We clamp by
    /// the remaining slack so we never scroll past the bottom of the
    /// content, and a negative `amount` reverses (used to release
    /// the auto-scroll when focus leaves the text field).
    fn try_scroll_to_lift(&mut self, amount: f64) -> f64 {
        if !self.v.enabled || amount.abs() < 0.5 {
            return 0.0;
        }
        let (_, vh) = self.viewport();
        let max = self.v.max_scroll(vh);
        let before = self.v.offset;
        let target = (before + amount).clamp(0.0, max);
        let applied = target - before;
        if applied.abs() < 0.5 {
            return 0.0;
        }
        self.v.offset = target;
        self.publish_offsets();
        crate::animation::request_draw();
        applied
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

    fn hit_test(&self, local_pos: Point) -> bool {
        if self.v.dragging || self.h.dragging || self.middle_dragging {
            return true;
        }
        let b = self.bounds();
        local_pos.x >= 0.0
            && local_pos.x <= b.width
            && local_pos.y >= 0.0
            && local_pos.y <= b.height
    }

    fn claims_pointer_exclusively(&self, local_pos: Point) -> bool {
        if self.v.dragging || self.h.dragging || self.middle_dragging {
            return true;
        }
        let (vw, vh) = self.viewport();
        if self.v.enabled && self.v.content > vh && self.pos_in_v_hover(local_pos) {
            return true;
        }
        if self.h.enabled && self.h.content > vw && self.pos_in_h_hover(local_pos) {
            return true;
        }
        false
    }

    fn layout(&mut self, available: Size) -> Size {
        // Pull live state from external cells first.
        if let Some(c) = &self.offset_cell {
            self.v.offset = c.get();
        }
        if let Some(c) = &self.h_offset_cell {
            self.h.offset = c.get();
        }
        if let Some(c) = &self.visibility_cell {
            self.bar_visibility = c.get();
        } else if !self.visibility_explicit {
            self.bar_visibility = current_scroll_visibility();
        }
        if let Some(c) = &self.style_cell {
            self.style = c.get();
        } else if !self.style_explicit {
            // No explicit override → follow the global scroll-bar style so
            // the Appearance demo restyles every `ScrollView` in the app.
            self.style = current_scroll_style();
        }

        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);

        // For horizontal scrolling, content width is unconstrained (the child
        // may return a width larger than our viewport).  For vertical-only, we
        // pin child to the viewport width so wrapping widgets behave.
        let (vw_guess, _vh_guess) = self.viewport();
        let child_in_w = if self.h.enabled {
            f64::MAX / 2.0
        } else {
            vw_guess
        };
        let child_in_h = f64::MAX / 2.0;

        if let Some(child) = self.children.first_mut() {
            let natural = child.layout(Size::new(child_in_w, child_in_h));
            self.v.content = natural.height;
            self.h.content = if self.h.enabled {
                natural.width
            } else {
                vw_guess
            };
        }

        // Re-query viewport now that content dimensions are known (Solid bars
        // may reserve different space once we know overflow).
        let (vw, vh) = self.viewport();

        if self.stick_to_bottom && self.was_at_bottom {
            self.v.offset = self.v.max_scroll(vh);
        }
        self.clamp_offsets();
        self.was_at_bottom = (self.v.max_scroll(vh) - self.v.offset).abs() < 0.5;

        // Publish offsets / max / viewport.
        if let Some(c) = &self.offset_cell {
            c.set(self.v.offset);
        }
        if let Some(c) = &self.max_scroll_cell {
            c.set(self.v.max_scroll(vh));
        }
        if let Some(c) = &self.h_offset_cell {
            c.set(self.h.offset);
        }
        if let Some(c) = &self.h_max_scroll_cell {
            c.set(self.h.max_scroll(vw));
        }
        if let Some(c) = &self.viewport_cell {
            // Content-space viewport rect in Y-UP content coords:
            //   x = h_offset  (left edge of visible region)
            //   y = (v_content_height - vh - v_offset) if inverting, but we
            //       expose TOP-DOWN coords for easier row math: y = v_offset.
            // We output a rect where (x, y) is the TOP-LEFT of visible content
            // in a conventional top-down space, and (width, height) = viewport.
            c.set(Rect::new(self.h.offset, self.v.offset, vw, vh));
        }

        // Position child inside the widget.
        if let Some(child) = self.children.first_mut() {
            let child_y = vh - self.v.content + self.v.offset;
            let child_x = -self.h.offset;
            child.set_bounds(Rect::new(
                child_x.round(),
                child_y.round(),
                if self.h.enabled { self.h.content } else { vw },
                self.v.content,
            ));
        }

        available
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn clip_children_rect(&self) -> Option<(f64, f64, f64, f64)> {
        // Clip children to the VIEWPORT so the content never overpaints the
        // scrollbar gutter or the edge guards.
        let (vw, vh) = self.viewport();
        Some((0.0, self.bounds.height - vh, vw, vh))
    }

    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        self.painted_style_epoch.set(current_scroll_style_epoch());

        // ── Fade gradient under the scrollbars ──
        //
        // egui paints the fade after content but before the bars, so the
        // fade hints clipped content without dimming the scrollbar itself.
        if self.style.fade_strength > 0.001 && self.style.fade_size > 0.5 {
            self.paint_fade(ctx);
        }

        // ── Vertical bar ──
        let (_, vh) = self.viewport();
        let v_geom = self.v_scrollbar_geometry();
        if let Some(bar) = self
            .v
            .prepare_paint(vh, self.style, self.bar_visibility, v_geom)
        {
            paint_prepared_scrollbar(ctx, bar);
        }

        // ── Horizontal bar ──
        let (vw, _) = self.viewport();
        let h_geom = self.h_scrollbar_geometry();
        if let Some(bar) = self
            .h
            .prepare_paint(vw, self.style, self.bar_visibility, h_geom)
        {
            paint_prepared_scrollbar(ctx, bar);
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            // ── Mouse wheel ───────────────────────────────────────────────────
            Event::MouseWheel {
                delta_y, delta_x, ..
            } => {
                // Convention: positive delta_y = user wants to see
                // content ABOVE = DECREASE offset (offset 0 = top of
                // content). Same sign for horizontal.
                let mut consumed = false;
                if self.v.enabled {
                    self.v.offset = self.v.offset - delta_y * 40.0;
                    consumed = true;
                }
                if self.h.enabled {
                    self.h.offset = self.h.offset - delta_x * 40.0;
                    consumed = true;
                }
                self.clamp_offsets();
                let (_, vh) = self.viewport();
                self.was_at_bottom = (self.v.max_scroll(vh) - self.v.offset).abs() < 0.5;
                if let Some(c) = &self.offset_cell {
                    c.set(self.v.offset);
                }
                if let Some(c) = &self.h_offset_cell {
                    c.set(self.h.offset);
                }
                if consumed {
                    crate::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }

            // ── Mouse move ────────────────────────────────────────────────────
            Event::MouseMove { pos } => {
                if self.middle_dragging {
                    let world = crate::widget::current_mouse_world().unwrap_or(*pos);
                    let dx = world.x - self.middle_start_world.x;
                    let dy = world.y - self.middle_start_world.y;
                    if self.h.enabled {
                        self.h.offset = self.middle_start_h_offset - dx;
                    }
                    if self.v.enabled {
                        self.v.offset = self.middle_start_v_offset + dy;
                    }
                    self.clamp_offsets();
                    let (_, vh) = self.viewport();
                    self.was_at_bottom = (self.v.max_scroll(vh) - self.v.offset).abs() < 0.5;
                    self.publish_offsets();
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }

                let (vw, vh) = self.viewport();
                let v_scroll = self.v.enabled && self.v.content > vh;
                let h_scroll = self.h.enabled && self.h.content > vw;
                let v_hover_changed =
                    self.v
                        .update_hover(*pos, vh, self.style, self.v_scrollbar_geometry());
                let h_hover_changed =
                    self.h
                        .update_hover(*pos, vw, self.style, self.h_scrollbar_geometry());
                if (v_scroll && v_hover_changed) || (h_scroll && h_hover_changed) {
                    crate::animation::request_draw();
                }

                if self.v.dragging {
                    if self
                        .v
                        .drag_to(*pos, vh, self.style, self.v_scrollbar_geometry())
                    {
                        self.was_at_bottom = (self.v.max_scroll(vh) - self.v.offset).abs() < 0.5;
                        if let Some(c) = &self.offset_cell {
                            c.set(self.v.offset);
                        }
                        crate::animation::request_draw();
                    }
                    return EventResult::Consumed;
                }
                if self.h.dragging {
                    if self
                        .h
                        .drag_to(*pos, vw, self.style, self.h_scrollbar_geometry())
                    {
                        if let Some(c) = &self.h_offset_cell {
                            c.set(self.h.offset);
                        }
                        crate::animation::request_draw();
                    }
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }

            // ── Mouse down ────────────────────────────────────────────────────
            Event::MouseDown {
                pos,
                button: MouseButton::Middle,
                ..
            } => {
                let (vw, vh) = self.viewport();
                if (self.v.enabled && self.v.content > vh)
                    || (self.h.enabled && self.h.content > vw)
                {
                    self.middle_dragging = true;
                    self.middle_start_world = crate::widget::current_mouse_world().unwrap_or(*pos);
                    self.middle_start_v_offset = self.v.offset;
                    self.middle_start_h_offset = self.h.offset;
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }

            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                let (vw, vh) = self.viewport();
                let v_scroll = self.v.enabled && self.v.content > vh;
                let h_scroll = self.h.enabled && self.h.content > vw;

                if v_scroll && self.pos_in_v_hover(*pos) {
                    if self
                        .v
                        .begin_drag(*pos, vh, self.style, self.v_scrollbar_geometry())
                    {
                        // No tick: thumb grab has no visible effect until
                        // the cursor actually moves.
                    } else if self
                        .v
                        .page_at(*pos, vh, self.style, self.v_scrollbar_geometry())
                    {
                        if let Some(c) = &self.offset_cell {
                            c.set(self.v.offset);
                        }
                        // Offset changed — visible scroll.
                        crate::animation::request_draw();
                    }
                    return EventResult::Consumed;
                }
                if h_scroll && self.pos_in_h_hover(*pos) {
                    if self
                        .h
                        .begin_drag(*pos, vw, self.style, self.h_scrollbar_geometry())
                    {
                        // No tick — see v-axis thumb grab comment above.
                    } else if self
                        .h
                        .page_at(*pos, vw, self.style, self.h_scrollbar_geometry())
                    {
                        if let Some(c) = &self.h_offset_cell {
                            c.set(self.h.offset);
                        }
                        crate::animation::request_draw();
                    }
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }

            // ── Mouse up ──────────────────────────────────────────────────────
            Event::MouseUp { button, .. } => {
                let was = self.v.dragging
                    || self.h.dragging
                    || (*button == MouseButton::Middle && self.middle_dragging);
                self.v.dragging = false;
                self.h.dragging = false;
                if *button == MouseButton::Middle {
                    self.middle_dragging = false;
                }
                if was {
                    crate::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }

            _ => EventResult::Ignored,
        }
    }

    /// Surface the per-axis offsets and the maximum scroll distance as
    /// inspector / test properties.  Tests use these to verify that a
    /// shrunken viewport actually exposes scrollable overflow.
    fn properties(&self) -> Vec<(&'static str, String)> {
        let (vw, vh) = self.viewport();
        vec![
            ("v_enabled", self.v.enabled.to_string()),
            ("h_enabled", self.h.enabled.to_string()),
            ("bar_visibility", format!("{:?}", self.bar_visibility)),
            ("v_offset", format!("{:.1}", self.v.offset)),
            ("h_offset", format!("{:.1}", self.h.offset)),
            ("max_scroll", format!("{:.1}", self.v.max_scroll(vh))),
            ("h_max_scroll", format!("{:.1}", self.h.max_scroll(vw))),
            ("v_content", format!("{:.1}", self.v.content)),
            ("h_content", format!("{:.1}", self.h.content)),
        ]
    }
}

impl ScrollView {
    /// Paint a gradient fade at the scroll-axis edges using thin horizontal or
    /// vertical strips with linearly interpolated alpha.  The strip closest to
    /// the clip edge is most opaque; the strip furthest inside the viewport is
    /// transparent — giving a smooth dissolve into the surrounding background.
    fn paint_fade(&self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        // Default to window_fill (correct only when the ScrollView sits
        // directly on a window).  Callers placing the view on a panel
        // / coloured container MUST pass `with_fade_color(...)` to
        // match the ancestor background — otherwise the fade looks
        // like a bright halo of the wrong colour.  See the doc-comment
        // on `ScrollView::with_fade_color`.
        let c = self.fade_color.unwrap_or(v.window_fill);
        let (vw, vh) = self.viewport();
        let strength = self.style.fade_strength.clamp(0.0, 1.0) as f32;
        let size = self.style.fade_size.max(0.0);
        let max_a = strength;

        // Fade appears only near edges where content is clipped.
        if self.v.enabled {
            if self.v.offset > 0.5 {
                // Top edge (Y-up: high Y).  Gradient transparent→opaque going up.
                Self::fill_v_gradient(
                    ctx,
                    c,
                    max_a,
                    0.0,
                    self.bounds.height - size,
                    vw,
                    size,
                    false,
                );
            }
            if (self.v.max_scroll(vh) - self.v.offset) > 0.5 {
                // Bottom edge.  Gradient transparent→opaque going down.
                let y_bottom = self.bounds.height - vh;
                Self::fill_v_gradient(ctx, c, max_a, 0.0, y_bottom, vw, size, true);
            }
        }
        if self.h.enabled {
            if self.h.offset > 0.5 {
                // Left edge.  Gradient transparent→opaque going left.
                Self::fill_h_gradient(ctx, c, max_a, 0.0, self.bounds.height - vh, size, vh, true);
            }
            if (self.h.max_scroll(vw) - self.h.offset) > 0.5 {
                // Right edge.  Gradient transparent→opaque going right.
                Self::fill_h_gradient(
                    ctx,
                    c,
                    max_a,
                    vw - size,
                    self.bounds.height - vh,
                    size,
                    vh,
                    false,
                );
            }
        }
    }

    /// Draw a vertical gradient rect using `STEPS` thin strips.
    ///
    /// When `opaque_at_bottom` is `true` the gradient runs opaque→transparent
    /// bottom-to-top (bottom edge fade); when `false` it runs
    /// transparent→opaque bottom-to-top (top edge fade).
    fn fill_v_gradient(
        ctx: &mut dyn DrawCtx,
        c: Color,
        max_alpha: f32,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        opaque_at_bottom: bool,
    ) {
        const STEPS: usize = 64;
        let strip_h = h / STEPS as f64;
        for i in 0..STEPS {
            // t = 0 at the transparent end, 1 at the opaque end.
            let t = (i as f32 + 0.5) / STEPS as f32;
            let a = if opaque_at_bottom { 1.0 - t } else { t };
            ctx.set_fill_color(Color::rgba(c.r, c.g, c.b, a * max_alpha));
            ctx.begin_path();
            ctx.rect(x, y + i as f64 * strip_h, w, strip_h + 0.5);
            ctx.fill();
        }
    }

    /// Draw a horizontal gradient rect using `STEPS` thin strips.
    ///
    /// When `opaque_at_left` is `true` the gradient runs opaque→transparent
    /// left-to-right (left edge fade); when `false` it runs
    /// transparent→opaque left-to-right (right edge fade).
    fn fill_h_gradient(
        ctx: &mut dyn DrawCtx,
        c: Color,
        max_alpha: f32,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        opaque_at_left: bool,
    ) {
        const STEPS: usize = 64;
        let strip_w = w / STEPS as f64;
        for i in 0..STEPS {
            let t = (i as f32 + 0.5) / STEPS as f32;
            let a = if opaque_at_left { 1.0 - t } else { t };
            ctx.set_fill_color(Color::rgba(c.r, c.g, c.b, a * max_alpha));
            ctx.begin_path();
            ctx.rect(x + i as f64 * strip_w, y, strip_w + 0.5, h);
            ctx.fill();
        }
    }
}
