use super::*;

impl Widget for TextField {
    fn type_name(&self) -> &'static str {
        "TextField"
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

    /// Composite parents (e.g. `ColorWheelPicker`) use this to push a live
    /// value into the field without bypassing `set_text`'s cell sync and
    /// cache invalidation.  Skipped while the field is focused so the user
    /// isn't fighting the parent for cursor position mid-edit; the parent
    /// is expected to read back via [`TextField::text`] after focus is
    /// released to pick up any user-typed value.
    fn set_label_text(&mut self, text: &str) {
        if self.focused {
            return;
        }
        if self.edit.borrow().text == text {
            return;
        }
        self.set_text(text);
    }

    /// While focused, the cursor blinks at 500 ms half-period.  The field
    /// itself drives its own repaint cadence: [`needs_draw`] reports dirty
    /// whenever wall-clock time has crossed a flip boundary since the last
    /// paint, and [`next_draw_deadline`] returns the exact wall-clock
    /// instant of the next boundary so the host can `WaitUntil` it.
    ///
    /// Losing focus makes both return `None` / `false`, and the tree walk's
    /// visibility check drops the field entirely when its enclosing window
    /// is closed / collapsed / tab not selected — so an invisible focused
    /// field does NOT keep the loop awake.
    fn needs_draw(&self) -> bool {
        if !self.focused {
            return false;
        }
        let Some(t) = self.focus_time else {
            return false;
        };
        let current_phase = (t.elapsed().as_millis() / 500) as u64;
        current_phase != self.blink_last_phase.get()
    }

    fn next_draw_deadline(&self) -> Option<web_time::Instant> {
        if !self.focused {
            return None;
        }
        let t = self.focus_time?;
        let ms = t.elapsed().as_millis() as u64;
        let next_phase = (ms / 500) + 1;
        Some(t + std::time::Duration::from_millis(next_phase * 500))
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

    fn backbuffer_cache_mut(&mut self) -> Option<&mut BackbufferCache> {
        Some(&mut self.cache)
    }

    fn backbuffer_mode(&self) -> BackbufferMode {
        if crate::font_settings::lcd_enabled() {
            BackbufferMode::LcdCoverage
        } else {
            BackbufferMode::Rgba
        }
    }

    fn layout(&mut self, available: Size) -> Size {
        self.sync_from_text_cell();
        // Sig excludes cursor-blink phase.  Cursor paints in
        // `paint_overlay` after cache blit — no blink-driven
        // invalidation.
        let st = self.edit.borrow();
        let font = self.active_font();
        let sig = TextFieldSig {
            text: st.text.clone(),
            cursor: st.cursor,
            anchor: st.anchor,
            focused: self.focused,
            hovered: self.hovered,
            scroll_x_bits: self.scroll_x.to_bits(),
            w_bits: self.bounds.width.to_bits(),
            h_bits: self.bounds.height.to_bits(),
            font_ptr: Arc::as_ptr(&font) as usize,
            font_size_bits: self.font_size.to_bits(),
        };
        drop(st);
        if self.last_sig.as_ref() != Some(&sig) {
            self.last_sig = Some(sig);
            self.cache.invalidate();
        }
        Size::new(available.width, (self.font_size * 2.4).max(28.0))
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let r = self.theme.border_radius.unwrap_or(6.0);
        let pad = self.padding;
        let (raw_text, raw_cursor, raw_anchor) = {
            let st = self.edit.borrow();
            (st.text.clone(), st.cursor, st.anchor)
        };
        // In password mode render '•' for every character, but keep byte positions
        // consistent by recomputing them against the masked string.
        let (text, cursor, anchor) = if self.password_mode {
            const BULLET: char = '•';
            const BULLET_LEN: usize = 3; // '•' is 3 bytes in UTF-8
            let n = raw_text.chars().count();
            let masked = BULLET.to_string().repeat(n);
            let cur = raw_text[..raw_cursor].chars().count() * BULLET_LEN;
            let anc = raw_text[..raw_anchor].chars().count() * BULLET_LEN;
            (masked, cur, anc)
        } else {
            (raw_text, raw_cursor, raw_anchor)
        };

        let v = ctx.visuals();
        let t = &self.theme;

        // ── Background ────────────────────────────────────────────────────
        ctx.set_fill_color(t.background.unwrap_or(v.widget_bg));
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, r);
        ctx.fill();

        // ── Text area clip ────────────────────────────────────────────────
        ctx.clip_rect(pad, 0.0, (w - pad * 2.0).max(0.0), h);

        let font = self.active_font();
        ctx.set_font(Arc::clone(&font));
        ctx.set_font_size(self.font_size);

        let m = ctx.measure_text("Ag").unwrap_or_default();
        let baseline_y = h * 0.5 - (m.ascent - m.descent) * 0.5;
        let text_x = pad - self.scroll_x;

        // ── Selection highlight ───────────────────────────────────────────
        if cursor != anchor {
            let lo = cursor.min(anchor);
            let hi = cursor.max(anchor);
            let lo_x = measure_advance(&font, &text[..lo], self.font_size);
            let hi_x = measure_advance(&font, &text[..hi], self.font_size);
            let sx = (text_x + lo_x).max(pad);
            let sw = (text_x + hi_x).min(w - pad) - sx;
            if sw > 0.0 {
                let hl_bot = baseline_y - m.descent;
                let hl_h = (m.ascent + m.descent) * 1.2;
                ctx.set_fill_color(if self.focused {
                    t.selection_bg.unwrap_or(v.selection_bg)
                } else {
                    t.selection_bg_unfocused.unwrap_or(v.selection_bg_unfocused)
                });
                ctx.begin_path();
                ctx.rect(sx, hl_bot - hl_h * 0.1, sw, hl_h);
                ctx.fill();
            }
        }

        // ── Text or placeholder ───────────────────────────────────────────
        if text.is_empty() && !self.focused {
            ctx.set_fill_color(t.placeholder_color.unwrap_or(v.text_dim));
            ctx.fill_text(&self.placeholder, text_x, baseline_y);
        } else {
            ctx.set_fill_color(t.text_color.unwrap_or(v.text_color));
            ctx.fill_text(&text, text_x, baseline_y);
        }

        // Cursor draws in `paint_overlay` — skipped here so blink
        // state doesn't force the cache to re-raster twice per second.

        ctx.reset_clip();

        // ── Border ────────────────────────────────────────────────────────
        let border_color = if self.focused {
            t.border_color_focused.unwrap_or(v.accent)
        } else if self.hovered {
            t.border_color_hovered.unwrap_or(v.widget_stroke_active)
        } else {
            t.border_color.unwrap_or(v.widget_stroke)
        };
        ctx.set_stroke_color(border_color);
        ctx.set_line_width(if self.focused { 2.0 } else { 1.0 });
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, r);
        ctx.stroke();
    }

    /// Cursor overlay — runs AFTER the cache blit on every frame, so
    /// blink-phase flips don't invalidate the backbuffer.  Reads the
    /// same edit state `paint()` does so cursor lands on the glyph the
    /// cached text shows.
    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        // Record the blink phase being drawn this frame.  The next tree
        // walk's `needs_draw` will compare against this and report dirty
        // once wall-clock time crosses the next 500 ms boundary — no
        // host-side deadline bookkeeping, the widget drives itself.
        if self.focused {
            if let Some(t) = self.focus_time {
                let phase = (t.elapsed().as_millis() / 500) as u64;
                self.blink_last_phase.set(phase);
            }
        }

        let cursor_visible = self.focused
            && {
                let st = self.edit.borrow();
                st.cursor == st.anchor
            }
            && match self.focus_time {
                Some(t) => (t.elapsed().as_millis() / 500) % 2 == 0,
                None => false,
            };
        if !cursor_visible {
            return;
        }

        let (text, cursor) = {
            let st = self.edit.borrow();
            let text = if self.password_mode {
                const BULLET: char = '•';
                let n = st.text.chars().count();
                BULLET.to_string().repeat(n)
            } else {
                st.text.clone()
            };
            let cursor = if self.password_mode {
                const BULLET_LEN: usize = 3;
                st.text[..st.cursor].chars().count() * BULLET_LEN
            } else {
                st.cursor
            };
            (text, cursor)
        };

        let h = self.bounds.height;
        let pad = self.padding;
        let v = ctx.visuals();

        let font = self.active_font();
        ctx.set_font(Arc::clone(&font));
        ctx.set_font_size(self.font_size);
        let m = ctx.measure_text("Ag").unwrap_or_default();
        let baseline_y = h * 0.5 - (m.ascent - m.descent) * 0.5;
        let text_x = pad - self.scroll_x;
        let cx = text_x + measure_advance(&font, &text[..cursor], self.font_size);
        let top = baseline_y + m.ascent;
        let bot = baseline_y - m.descent;

        // Clip to the text area so the cursor can't spill past the
        // padding or the border.
        ctx.save();
        ctx.clip_rect(pad, 0.0, (self.bounds.width - pad * 2.0).max(0.0), h);
        ctx.set_stroke_color(self.theme.cursor_color.unwrap_or(v.accent));
        ctx.set_line_width(1.5);
        ctx.begin_path();
        ctx.move_to(cx, bot);
        ctx.line_to(cx, top);
        ctx.stroke();
        ctx.restore();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let was = self.hovered;
                self.hovered = self.hit_test(*pos);
                if self.mouse_down && self.focused {
                    let tx = pos.x - self.padding + self.scroll_x;
                    let text = self.edit.borrow().text.clone();
                    let new_cur = self.click_to_cursor(&text, tx);
                    self.edit.borrow_mut().cursor = new_cur;
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
                pos,
                button: MouseButton::Left,
                modifiers: mods,
            } => {
                self.mouse_down = true;
                let tx = pos.x - self.padding + self.scroll_x;
                let text = self.edit.borrow().text.clone();
                let new_cur = self.click_to_cursor(&text, tx);

                // Double-click: select word
                let is_double = self
                    .last_click_time
                    .map(|t| t.elapsed().as_millis() < 350)
                    .unwrap_or(false);
                self.last_click_time = Some(Instant::now());

                if is_double && !mods.shift {
                    let (ws, we) = word_range_at(&text, new_cur);
                    self.edit.borrow_mut().anchor = ws;
                    self.edit.borrow_mut().cursor = we;
                } else if mods.shift {
                    self.edit.borrow_mut().cursor = new_cur;
                } else {
                    self.edit.borrow_mut().cursor = new_cur;
                    self.edit.borrow_mut().anchor = new_cur;
                }
                // Reset blink phase on click so cursor is immediately visible.
                self.focus_time = Some(Instant::now());
                crate::animation::request_draw();
                EventResult::Consumed
            }

            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                self.mouse_down = false;
                EventResult::Ignored
            }

            Event::FocusGained => {
                self.focused = true;
                self.focus_time = Some(Instant::now());
                self.text_on_focus = self.text();
                if self.select_all_on_focus {
                    let len = self.edit.borrow().text.len();
                    self.edit.borrow_mut().anchor = 0;
                    self.edit.borrow_mut().cursor = len;
                }
                crate::animation::request_draw();
                EventResult::Ignored
            }

            Event::FocusLost => {
                let was_focused = self.focused;
                self.focused = false;
                self.focus_time = None;
                self.mouse_down = false;
                self.flush_pending();
                if self.text() != self.text_on_focus {
                    self.notify_edit_complete();
                }
                if was_focused {
                    crate::animation::request_draw();
                }
                EventResult::Ignored
            }

            Event::KeyDown { key, modifiers } if self.focused => {
                // Reset blink on any keypress so cursor is visible immediately.
                self.focus_time = Some(Instant::now());
                let result = self.handle_key(key, *modifiers);
                // Any text-editing keystroke that reached the focused field
                // visibly mutates the text / cursor / selection; repaint.
                if result == EventResult::Consumed {
                    crate::animation::request_draw();
                }
                result
            }

            _ => EventResult::Ignored,
        }
    }
}
