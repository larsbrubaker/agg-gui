use super::*;

impl Widget for ColorPicker {
    fn type_name(&self) -> &'static str {
        "ColorPicker"
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

    fn layout(&mut self, available: Size) -> Size {
        // Sync no_color from cell if someone else flipped it.
        self.no_color = self.none_cell.get();

        let w = if self.open {
            PANEL_W.min(available.width.max(PANEL_W))
        } else {
            available.width.max(SWATCH_MIN_W).min(PANEL_W)
        };

        let h = if self.open {
            SWATCH_H + panel_body_h(self.allow_none)
        } else {
            SWATCH_H
        };

        self.bounds = Rect::new(0.0, 0.0, w, h);

        if self.open {
            let r = self.regions();
            // Position sub-widgets.
            if let Some(none_rect) = r.none {
                if let Some(idx) = self.idx_none {
                    let cb = &mut self.children[idx];
                    cb.layout(Size::new(none_rect.width, none_rect.height));
                    cb.set_bounds(none_rect);
                }
            }
            let cb = &mut self.children[self.idx_cancel];
            cb.layout(Size::new(r.cancel.width, r.cancel.height));
            cb.set_bounds(r.cancel);

            let sb = &mut self.children[self.idx_select];
            sb.layout(Size::new(r.select.width, r.select.height));
            sb.set_bounds(r.select);
        } else {
            // Give children zero bounds off-panel so they don't paint.
            for c in self.children.iter_mut() {
                c.set_bounds(Rect::new(0.0, 0.0, 0.0, 0.0));
            }
        }

        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let r = self.regions();

        // Outer panel background (if open).
        if self.open {
            ctx.set_fill_color(v.widget_bg);
            ctx.begin_path();
            ctx.rounded_rect(0.0, 0.0, self.bounds.width, self.bounds.height, 6.0);
            ctx.fill();
            ctx.set_stroke_color(v.widget_stroke);
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.rounded_rect(0.0, 0.0, self.bounds.width, self.bounds.height, 6.0);
            ctx.stroke();
        }

        // ── Swatch ──────────────────────────────────────────────────────────
        paint_checker_bg(ctx, r.swatch, 6.0);
        let cur = self.sync_color_from_hsva();
        ctx.set_fill_color(cur);
        ctx.begin_path();
        ctx.rounded_rect(r.swatch.x, r.swatch.y, r.swatch.width, r.swatch.height, 4.0);
        ctx.fill();
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(r.swatch.x, r.swatch.y, r.swatch.width, r.swatch.height, 4.0);
        ctx.stroke();

        if !self.open {
            return;
        }

        // ── Hue slider ──────────────────────────────────────────────────────
        paint_hue_strip(ctx, r.hue);
        paint_vertical_marker(ctx, r.hue, self.h, v.widget_stroke_active);

        // ── SV rectangle ────────────────────────────────────────────────────
        paint_sv_rect(ctx, r.sv, self.h);
        let mx = r.sv.x + self.s as f64 * r.sv.width;
        // Y-up: value 1 → TOP of the rect (y = r.sv.y + r.sv.height).
        let my = r.sv.y + self.v as f64 * r.sv.height;
        paint_crosshair(ctx, mx, my, v.widget_stroke_active);

        // ── Alpha slider ────────────────────────────────────────────────────
        paint_checker_bg(ctx, r.alpha, 4.0);
        paint_alpha_strip(ctx, r.alpha, cur);
        paint_vertical_marker(ctx, r.alpha, self.a, v.widget_stroke_active);

        // ── Hex readout ─────────────────────────────────────────────────────
        ctx.set_fill_color(v.widget_bg_hovered);
        ctx.begin_path();
        ctx.rounded_rect(r.hex.x, r.hex.y, r.hex.width, r.hex.height, 3.0);
        ctx.fill();
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(r.hex.x, r.hex.y, r.hex.width, r.hex.height, 3.0);
        ctx.stroke();

        let hex = format_hex(cur);
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        ctx.set_fill_color(v.text_color);
        let text_w = ctx.measure_text(&hex).map(|m| m.width).unwrap_or(0.0);
        let tx = r.hex.x + (r.hex.width - text_w) * 0.5;
        let ty = r.hex.y + (r.hex.height - self.font_size) * 0.5 + 2.0;
        ctx.fill_text(&hex, tx, ty);

        // ── Sub-widgets (No Color + Cancel/Select) ───────────────────────────
        for child in self.children.iter_mut() {
            let b = child.bounds();
            if b.width <= 0.0 || b.height <= 0.0 {
                continue;
            }
            ctx.save();
            ctx.translate(b.x, b.y);
            paint_subtree(child.as_mut(), ctx);
            ctx.restore();
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        // Let sub-widgets (No Color, Cancel, Select) see pointer events that
        // actually land on them.  `Button::on_event` consumes every
        // MouseDown / MouseUp regardless of hit-test, so we MUST gate by
        // `contains(child.bounds, pos)` before dispatching — otherwise the
        // Cancel/Select buttons swallow clicks anywhere else in the panel
        // (hue / sv / alpha), breaking colour picking.
        if self.open {
            let local_pt = match event {
                Event::MouseMove { pos } => Some(*pos),
                Event::MouseDown { pos, .. } => Some(*pos),
                Event::MouseUp { pos, .. } => Some(*pos),
                _ => None,
            };

            for child in self.children.iter_mut() {
                let b = child.bounds();
                if b.width <= 0.0 || b.height <= 0.0 {
                    continue;
                }
                // Only dispatch pointer events when the cursor is over the
                // child.  Non-pointer events (focus, keys) we always route,
                // since the framework already knows who should receive them.
                if let Some(p) = local_pt {
                    if !contains(&b, p) {
                        continue;
                    }
                    let lp = Point::new(p.x - b.x, p.y - b.y);
                    let translated = translate_mouse_event(event, lp);
                    let res = child.on_event(&translated);
                    if res == EventResult::Consumed {
                        self.handle_btn_flags();
                        return EventResult::Consumed;
                    }
                } else {
                    let res = child.on_event(event);
                    if res == EventResult::Consumed {
                        self.handle_btn_flags();
                        return EventResult::Consumed;
                    }
                }
            }
            self.handle_btn_flags();
        }

        match event {
            Event::MouseDown {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                let r = self.regions();
                if !self.open {
                    if contains(&r.swatch, *pos) {
                        // Open.
                        self.open = true;
                        self.saved = self.color_cell.get();
                        let (h, s, v) = rgb_to_hsv(self.saved.r, self.saved.g, self.saved.b);
                        self.h = h;
                        self.s = s;
                        self.v = v;
                        self.a = self.saved.a;
                        self.no_color = self.saved.a <= 0.0;
                        self.none_cell.set(self.no_color);
                        crate::animation::request_draw();
                        return EventResult::Consumed;
                    }
                    return EventResult::Ignored;
                }
                if contains(&r.hue, *pos) {
                    self.drag = Drag::Hue;
                    self.h = ((pos.x - r.hue.x) / r.hue.width).clamp(0.0, 1.0) as f32;
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                if contains(&r.sv, *pos) {
                    self.drag = Drag::Sv;
                    self.s = ((pos.x - r.sv.x) / r.sv.width).clamp(0.0, 1.0) as f32;
                    self.v = ((pos.y - r.sv.y) / r.sv.height).clamp(0.0, 1.0) as f32;
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                if contains(&r.alpha, *pos) {
                    self.drag = Drag::Alpha;
                    self.a = ((pos.x - r.alpha.x) / r.alpha.width).clamp(0.0, 1.0) as f32;
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseMove { pos } => {
                self.hovered = self.hit_test(*pos);
                if self.drag == Drag::None {
                    return EventResult::Ignored;
                }
                let r = self.regions();
                match self.drag {
                    Drag::Hue => {
                        self.h = ((pos.x - r.hue.x) / r.hue.width).clamp(0.0, 1.0) as f32;
                    }
                    Drag::Sv => {
                        self.s = ((pos.x - r.sv.x) / r.sv.width).clamp(0.0, 1.0) as f32;
                        self.v = ((pos.y - r.sv.y) / r.sv.height).clamp(0.0, 1.0) as f32;
                    }
                    Drag::Alpha => {
                        self.a = ((pos.x - r.alpha.x) / r.alpha.width).clamp(0.0, 1.0) as f32;
                    }
                    Drag::None => {}
                }
                // Live-preview: push working colour to the cell so the demo's
                // preview updates as the user drags.  Cancel restores `saved`.
                if !self.no_color {
                    let c = self.sync_color_from_hsva();
                    self.color_cell.set(c);
                }
                crate::animation::request_draw();
                EventResult::Consumed
            }
            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                let was_dragging = self.drag != Drag::None;
                self.drag = Drag::None;
                if was_dragging {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }
}

impl ColorPicker {
    fn handle_btn_flags(&mut self) {
        if self.cancel_flag.get() {
            self.cancel_flag.set(false);
            self.cancel();
        }
        if self.select_flag.get() {
            self.select_flag.set(false);
            self.commit();
        }
        if self.open {
            let want = self.none_cell.get();
            if want != self.no_color {
                self.no_color = want;
                if want {
                    // Preview transparent immediately.
                    self.color_cell.set(Color::transparent());
                } else {
                    let c = self.sync_color_from_hsva();
                    self.color_cell.set(c);
                }
                crate::animation::request_draw();
            }
        }
    }
}

// ── Drawing helpers ──────────────────────────────────────────────────────────

fn contains(r: &Rect, p: Point) -> bool {
    p.x >= r.x && p.x <= r.x + r.width && p.y >= r.y && p.y <= r.y + r.height
}

fn translate_mouse_event(e: &Event, p: Point) -> Event {
    match e {
        Event::MouseMove { .. } => Event::MouseMove { pos: p },
        Event::MouseDown {
            button, modifiers, ..
        } => Event::MouseDown {
            button: *button,
            pos: p,
            modifiers: *modifiers,
        },
        Event::MouseUp {
            button, modifiers, ..
        } => Event::MouseUp {
            button: *button,
            pos: p,
            modifiers: *modifiers,
        },
        _ => e.clone(),
    }
}

fn paint_checker_bg(ctx: &mut dyn DrawCtx, r: Rect, tile: f64) {
    ctx.set_fill_color(Color::rgb(0.75, 0.75, 0.75));
    ctx.begin_path();
    ctx.rect(r.x, r.y, r.width, r.height);
    ctx.fill();

    ctx.set_fill_color(Color::rgb(0.45, 0.45, 0.45));
    let cols = (r.width / tile).ceil() as i32;
    let rows = (r.height / tile).ceil() as i32;
    for row in 0..rows {
        for col in 0..cols {
            if (row + col) & 1 == 0 {
                let x = r.x + col as f64 * tile;
                let y = r.y + row as f64 * tile;
                let w = (tile).min(r.x + r.width - x).max(0.0);
                let h = (tile).min(r.y + r.height - y).max(0.0);
                if w > 0.0 && h > 0.0 {
                    ctx.begin_path();
                    ctx.rect(x, y, w, h);
                    ctx.fill();
                }
            }
        }
    }
}

fn paint_hue_strip(ctx: &mut dyn DrawCtx, r: Rect) {
    let steps = r.width.ceil() as i32;
    let step_w = r.width / steps as f64;
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        let (cr, cg, cb) = hsv_to_rgb(t, 1.0, 1.0);
        ctx.set_fill_color(Color::rgb(cr, cg, cb));
        ctx.begin_path();
        ctx.rect(r.x + i as f64 * step_w, r.y, step_w + 1.0, r.height);
        ctx.fill();
    }
}

fn paint_sv_rect(ctx: &mut dyn DrawCtx, r: Rect, hue: f32) {
    // Paint horizontal strips from bottom (value=0 → black) to top (value=1 →
    // fully saturated hue), each strip ramps saturation left→right.
    // We approximate by stacking thin horizontal strips where each strip
    // interpolates from white→hue_at_row along X, then multiplying by value.
    //
    // Simpler two-pass approximation:
    //   1. Draw a hue→white horizontal gradient at full V.
    //   2. Overlay a black→transparent vertical gradient (from bottom).
    let (hr, hg, hb) = hsv_to_rgb(hue, 1.0, 1.0);

    let cols = r.width.ceil() as i32;
    let col_w = r.width / cols as f64;
    for i in 0..cols {
        let tx = i as f32 / cols as f32;
        // Hue colour at saturation=tx, value=1
        let cr = 1.0 * (1.0 - tx) + hr * tx;
        let cg = 1.0 * (1.0 - tx) + hg * tx;
        let cb = 1.0 * (1.0 - tx) + hb * tx;
        ctx.set_fill_color(Color::rgb(cr, cg, cb));
        ctx.begin_path();
        ctx.rect(r.x + i as f64 * col_w, r.y, col_w + 1.0, r.height);
        ctx.fill();
    }

    // Vertical value overlay: black (bottom) → transparent (top).
    let rows = r.height.ceil() as i32;
    let row_h = r.height / rows as f64;
    for j in 0..rows {
        // Y-up: j=0 is the BOTTOM row (value=0 → full black).
        let ty = j as f32 / rows as f32; // 0 at bottom, 1 at top
        let alpha = 1.0 - ty;
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, alpha));
        ctx.begin_path();
        ctx.rect(r.x, r.y + j as f64 * row_h, r.width, row_h + 1.0);
        ctx.fill();
    }
}

fn paint_alpha_strip(ctx: &mut dyn DrawCtx, r: Rect, c: Color) {
    let steps = r.width.ceil() as i32;
    let step_w = r.width / steps as f64;
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        ctx.set_fill_color(Color::rgba(c.r, c.g, c.b, t));
        ctx.begin_path();
        ctx.rect(r.x + i as f64 * step_w, r.y, step_w + 1.0, r.height);
        ctx.fill();
    }
}

fn paint_vertical_marker(ctx: &mut dyn DrawCtx, r: Rect, t: f32, col: Color) {
    let x = r.x + (t.clamp(0.0, 1.0) as f64) * r.width;
    ctx.set_stroke_color(Color::white());
    ctx.set_line_width(3.0);
    ctx.begin_path();
    ctx.move_to(x, r.y);
    ctx.line_to(x, r.y + r.height);
    ctx.stroke();
    ctx.set_stroke_color(col);
    ctx.set_line_width(1.5);
    ctx.begin_path();
    ctx.move_to(x, r.y);
    ctx.line_to(x, r.y + r.height);
    ctx.stroke();
}

fn paint_crosshair(ctx: &mut dyn DrawCtx, x: f64, y: f64, col: Color) {
    ctx.set_stroke_color(Color::white());
    ctx.set_line_width(3.0);
    ctx.begin_path();
    ctx.circle(x, y, 5.0);
    ctx.stroke();
    ctx.set_stroke_color(col);
    ctx.set_line_width(1.5);
    ctx.begin_path();
    ctx.circle(x, y, 5.0);
    ctx.stroke();
}
