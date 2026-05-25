use super::*;

impl Widget for TextArea {
    fn type_name(&self) -> &'static str {
        "TextArea"
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

    fn is_focusable(&self) -> bool {
        true
    }

    fn accepts_text_input(&self) -> bool {
        true
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

    fn measure_min_height(&self, available_w: f64) -> f64 {
        // Wrap our text at the supplied width and report the total
        // visual height + vertical padding.  This is what an
        // ancestor `Window::tight_content_fit` sums to derive a
        // window minimum that prevents text from going off-screen,
        // even when this widget sits in a flex-fill slot whose
        // `layout` would otherwise just return the available area.
        //
        // Cheap: `wrap_text_indexed` is the same function `layout`
        // already calls; it doesn't mutate any cache.  Always at
        // least one line so the cursor has somewhere to sit.
        let inner_w = (available_w - self.padding * 2.0).max(1.0);
        let lines = wrap_text_indexed(
            &self.font,
            &self.edit.borrow().text,
            self.font_size,
            inner_w,
        );
        let line_h = self.font_size * 1.35;
        (lines.len().max(1) as f64) * line_h + self.padding * 2.0
    }

    fn layout(&mut self, available: Size) -> Size {
        // Fill the slot we're given.  A parent that allocates us via
        // a flex-weight gets everything; a parent that asks for our
        // natural size gets the same (the caller is opting into
        // "whatever you want" with `available`).
        let w = available.width.max(self.padding * 2.0 + 20.0);
        let h = available
            .height
            .max(self.padding * 2.0 + self.font_size * 1.6);
        self.bounds = Rect::new(0.0, 0.0, w, h);
        let inner_w = (w - self.padding * 2.0).max(1.0);
        self.refresh_wrap(inner_w);
        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // Background — theme widget fill.
        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 4.0);
        ctx.fill();

        // Clip content to the padded inner rect so overflow text can't
        // leak across the border.
        ctx.clip_rect(
            self.padding,
            self.padding,
            (w - self.padding * 2.0).max(0.0),
            (h - self.padding * 2.0).max(0.0),
        );

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);

        // ── Selection highlight ───────────────────────────────────
        let st = self.edit.borrow().clone();
        if st.cursor != st.anchor {
            let lo = st.cursor.min(st.anchor);
            let hi = st.cursor.max(st.anchor);
            let hl_color = if self.focused {
                v.selection_bg
            } else {
                v.selection_bg_unfocused
            };
            ctx.set_fill_color(hl_color);
            for (i, line) in self.cached_lines.iter().enumerate() {
                if line.end < lo || line.start > hi {
                    continue;
                }
                let sel_s = lo.max(line.start) - line.start;
                let sel_e = hi.min(line.end) - line.start;
                let sel_e = sel_e.min(line.text.len());
                if sel_e <= sel_s {
                    continue;
                }
                let x0 =
                    self.padding + measure_advance(&self.font, &line.text[..sel_s], self.font_size);
                let x1 =
                    self.padding + measure_advance(&self.font, &line.text[..sel_e], self.font_size);
                let line_top = h - self.padding - i as f64 * self.cached_line_h;
                let line_bottom = line_top - self.cached_line_h;
                ctx.begin_path();
                ctx.rect(x0, line_bottom, x1 - x0, self.cached_line_h);
                ctx.fill();
            }
        }

        // ── Text ───────────────────────────────────────────────────
        ctx.set_fill_color(v.text_color);
        // Tight metrics for baseline positioning — the glyph baseline
        // sits `descent` above each line's bottom edge.
        let m = ctx.measure_text("Ag").unwrap_or_default();
        for (i, line) in self.cached_lines.iter().enumerate() {
            if line.text.is_empty() {
                continue;
            }
            let line_top = h - self.padding - i as f64 * self.cached_line_h;
            let line_bottom = line_top - self.cached_line_h;
            let baseline_y =
                line_bottom + (self.cached_line_h - (m.ascent - m.descent)) * 0.5 + m.descent;
            ctx.fill_text(&line.text, self.padding, baseline_y);
        }

        // ── Placeholder when empty + unfocused ─────────────────────
        if st.text.is_empty() && !self.focused {
            ctx.set_fill_color(v.text_dim);
            let line_top = h - self.padding;
            let line_bottom = line_top - self.cached_line_h;
            let baseline_y =
                line_bottom + (self.cached_line_h - (m.ascent - m.descent)) * 0.5 + m.descent;
            ctx.fill_text("Type here…", self.padding, baseline_y);
        }

        ctx.reset_clip();

        // ── Border ────────────────────────────────────────────────
        let border = if self.focused {
            v.accent
        } else if self.hovered {
            v.widget_stroke_active
        } else {
            v.widget_stroke
        };
        let line_width = if self.focused { 2.0 } else { 1.0 };
        ctx.set_stroke_color(border);
        ctx.set_line_width(line_width);
        ctx.begin_path();
        let inset = line_width * 0.5;
        ctx.rounded_rect(
            inset,
            inset,
            (w - line_width).max(0.0),
            (h - line_width).max(0.0),
            4.0,
        );
        ctx.stroke();
    }

    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        // Cursor blink (drawn in overlay so the blink doesn't
        // invalidate a cached text bitmap).  500 ms half-cycle.
        if !self.focused {
            return;
        }
        if let Some(t) = self.focus_time {
            let phase = (t.elapsed().as_millis() / 500) as u64;
            self.blink_last_phase.set(phase);
            if phase % 2 == 1 {
                return;
            }
        }
        let st = self.edit.borrow().clone();
        let p = self.pos_for_cursor(st.cursor);
        let v = ctx.visuals();
        ctx.set_stroke_color(v.text_color);
        ctx.set_line_width(1.5);
        ctx.begin_path();
        ctx.move_to(p.x, p.y);
        ctx.line_to(p.x, p.y + self.cached_line_h);
        ctx.stroke();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let was = self.hovered;
                self.hovered = self.hit_test(*pos);
                if self.hovered {
                    set_cursor_icon(CursorIcon::Text);
                }
                if self.selecting_drag {
                    let off = self.byte_offset_at(*pos);
                    self.move_cursor_to(off, /*with_selection=*/ true);
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                if was != self.hovered {
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                button: MouseButton::Left,
                pos,
                modifiers,
            } => {
                let off = self.byte_offset_at(*pos);
                self.move_cursor_to(off, /*with_selection=*/ modifiers.shift);
                self.selecting_drag = true;
                crate::animation::request_draw();
                EventResult::Consumed
            }
            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                self.selecting_drag = false;
                EventResult::Consumed
            }
            Event::FocusGained => {
                self.focused = true;
                self.focus_time = Some(Instant::now());
                crate::animation::request_draw();
                EventResult::Ignored
            }
            Event::FocusLost => {
                self.focused = false;
                self.selecting_drag = false;
                crate::animation::request_draw();
                EventResult::Ignored
            }
            Event::KeyDown { key, modifiers } => {
                let shift = modifiers.shift;
                let cmd = modifiers.ctrl || modifiers.meta;
                match key {
                    Key::ArrowLeft => {
                        self.move_char(-1, shift);
                    }
                    Key::ArrowRight => {
                        self.move_char(1, shift);
                    }
                    Key::ArrowUp => {
                        self.move_line(-1, shift);
                    }
                    Key::ArrowDown => {
                        self.move_line(1, shift);
                    }
                    Key::Home => {
                        let cur = self.edit.borrow().cursor;
                        let line = self.line_for_cursor(cur);
                        let start = self.cached_lines[line].start;
                        self.move_cursor_to(start, shift);
                    }
                    Key::End => {
                        let cur = self.edit.borrow().cursor;
                        let line = self.line_for_cursor(cur);
                        let end = self.cached_lines[line].end;
                        self.move_cursor_to(end, shift);
                    }
                    Key::Backspace => {
                        self.delete(-1);
                    }
                    Key::Delete => {
                        self.delete(1);
                    }
                    Key::Enter => {
                        self.insert_str("\n");
                    }
                    Key::Tab => {
                        self.insert_str("    ");
                    }
                    Key::Char('a') | Key::Char('A') if cmd => {
                        // Select-all — set anchor to start, cursor to
                        // end.  Common Ctrl+A shortcut.
                        let len = self.edit.borrow().text.len();
                        self.move_cursor_to(0, false);
                        self.move_cursor_to(len, true);
                    }
                    Key::Char('c') | Key::Char('C') if cmd => {
                        let st = self.edit.borrow();
                        let (lo, hi) = (st.cursor.min(st.anchor), st.cursor.max(st.anchor));
                        if hi > lo {
                            let sel = st.text[lo..hi].to_string();
                            drop(st);
                            clipboard_set(&sel);
                        }
                    }
                    Key::Char('x') | Key::Char('X') if cmd => {
                        let st = self.edit.borrow();
                        let (lo, hi) = (st.cursor.min(st.anchor), st.cursor.max(st.anchor));
                        if hi > lo {
                            let sel = st.text[lo..hi].to_string();
                            drop(st);
                            clipboard_set(&sel);
                            self.delete(0);
                        }
                    }
                    Key::Char('v') | Key::Char('V') if cmd => {
                        if let Some(t) = clipboard_get() {
                            self.insert_str(&t);
                        }
                    }
                    Key::Char(c) if !cmd => {
                        let mut s = [0u8; 4];
                        self.insert_str(c.encode_utf8(&mut s));
                    }
                    _ => return EventResult::Ignored,
                }
                self.focus_time = Some(Instant::now());
                crate::animation::request_draw();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        local_pos.x >= 0.0
            && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0
            && local_pos.y <= self.bounds.height
    }

    fn properties(&self) -> Vec<(&'static str, String)> {
        let st = self.edit.borrow();
        vec![
            ("len", st.text.len().to_string()),
            ("cursor", st.cursor.to_string()),
            ("lines", self.cached_lines.len().to_string()),
            ("focused", self.focused.to_string()),
        ]
    }

    fn needs_draw(&self) -> bool {
        self.focused
    }
}
