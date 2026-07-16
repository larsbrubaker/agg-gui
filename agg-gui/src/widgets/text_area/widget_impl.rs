use super::*;
use crate::color::Color;

/// Split a highlighted line into gap-free, non-overlapping colour segments
/// covering `[0, text.len())` exactly once.
///
/// Bytes covered by a valid span take that span's colour; every uncovered
/// gap takes `base_color`. This is the segmentation the AA text path needs:
/// each glyph is emitted by exactly one segment, so highlighted tokens never
/// accumulate a second alpha pass on their fringes (which made them look
/// subtly bolder) and no fill work is duplicated.
///
/// Spans are validated defensively — reversed, out-of-range, or
/// non-char-boundary spans are dropped. Spans are processed in start order
/// and the first span to cover a byte wins any overlap, so the output stays
/// strictly non-overlapping even if a highlighter hands back sloppy ranges.
pub(crate) fn segment_highlight(
    text: &str,
    spans: &[(usize, usize, Color)],
    base_color: Color,
) -> Vec<(usize, usize, Color)> {
    let len = text.len();
    let mut valid: Vec<(usize, usize, Color)> = spans
        .iter()
        .copied()
        .filter(|&(s, e, _)| {
            s < e && e <= len && text.is_char_boundary(s) && text.is_char_boundary(e)
        })
        .collect();
    valid.sort_by_key(|&(s, _, _)| s);

    let mut out: Vec<(usize, usize, Color)> = Vec::new();
    let mut pos = 0usize;
    for (s, e, color) in valid {
        if e <= pos {
            // Fully behind already-emitted output — first span won this byte.
            continue;
        }
        // Clamp a partially overlapping start up to the emitted frontier.
        let s = s.max(pos);
        if s > pos {
            out.push((pos, s, base_color)); // uncovered gap
        }
        out.push((s, e, color));
        pos = e;
    }
    if pos < len {
        out.push((pos, len, base_color));
    }
    out
}

/// Clamp the caret's vertical span `[p_y, p_y + line_h]` (Y-up) to the padded
/// inner band `[inner_lo, inner_hi]`, returning the visible sub-segment. Yields
/// `None` when the caret's line has scrolled entirely outside the inner rect,
/// so the un-clipped `paint_overlay` never strokes the caret over the
/// border/padding.
pub(crate) fn caret_visible_segment(
    p_y: f64,
    line_h: f64,
    inner_lo: f64,
    inner_hi: f64,
) -> Option<(f64, f64)> {
    let y0 = p_y.max(inner_lo);
    let y1 = (p_y + line_h).min(inner_hi);
    if y1 > y0 {
        Some((y0, y1))
    } else {
        None
    }
}

impl TextArea {
    /// Paint one wrapped line as gap-free, non-overlapping colour segments so
    /// every glyph is filled exactly once (see [`segment_highlight`]). Byte
    /// offsets in `spans` are relative to `text`.
    fn paint_highlighted_line(
        &self,
        ctx: &mut dyn DrawCtx,
        text: &str,
        spans: &[(usize, usize, Color)],
        x0: f64,
        baseline_y: f64,
        base_color: Color,
    ) {
        for (s, e, color) in segment_highlight(text, spans, base_color) {
            let x = x0 + measure_advance(&self.font, &text[..s], self.font_size);
            ctx.set_fill_color(color);
            ctx.fill_text(&text[s..e], x, baseline_y);
        }
    }
}

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

    fn focus_id(&self) -> Option<crate::focus::FocusId> {
        self.focus_request_id
    }

    fn accepts_text_input(&self) -> bool {
        true
    }

    fn text_input_value(&self) -> Option<String> {
        Some(self.text())
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
        // Same decision as `Label` / `TextField`: LCD-subpixel coverage buffer
        // when the global toggle is on (default at scale ≤ 1.25), grayscale
        // RGBA otherwise. `backbuffer_mode`'s mode-flip detection in
        // `paint_subtree_backbuffered` re-rasters when the toggle changes.
        if crate::font_settings::lcd_enabled() {
            BackbufferMode::LcdCoverage
        } else {
            BackbufferMode::Rgba
        }
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
        self.sync_scroll();
        // Catch cursor moves made straight through the shared TextEditState
        // (e.g. the demo's "start"/"end" buttons) that bypass the edit funnel
        // and thus never ran ensure_cursor_visible. The first layout only
        // records the baseline so seeding a caret at the end doesn't force an
        // unexpected scroll before the user interacts.
        let cursor = self.edit.borrow().cursor;
        if matches!(self.last_layout_cursor, Some(prev) if prev != cursor) {
            self.ensure_cursor_visible();
        }
        self.last_layout_cursor = Some(cursor);

        // Drop the cached bitmap when any painted input changed (text, cursor,
        // selection, scroll, focus/hover, size, alignment). Blink phase and the
        // floating scrollbar are excluded — they paint in `paint_overlay`.
        let sig = self.cache_sig();
        if self.last_sig.as_ref() != Some(&sig) {
            self.last_sig = Some(sig);
            self.cache.invalidate();
        }
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
                let line_x = self.line_x_start(line);
                let x0 = line_x + measure_advance(&self.font, &line.text[..sel_s], self.font_size);
                let x1 = line_x + measure_advance(&self.font, &line.text[..sel_e], self.font_size);
                let line_top = self.line_top_y(i);
                let line_bottom = line_top - self.cached_line_h;
                ctx.begin_path();
                ctx.rect(x0, line_bottom, x1 - x0, self.cached_line_h);
                ctx.fill();
            }
        }

        // ── Text ───────────────────────────────────────────────────
        ctx.set_fill_color(v.text_color);
        for (i, line) in self.cached_lines.iter().enumerate() {
            if line.text.is_empty() {
                continue;
            }
            let baseline_y = self.line_baseline_y(i);
            let x0 = self.line_x_start(line);
            match &self.highlighter {
                // Syntax-highlighted path: paint each coloured run at its
                // measured x offset; gaps stay in the ambient text colour.
                Some(hl) => {
                    let spans = hl(&line.text);
                    self.paint_highlighted_line(ctx, &line.text, &spans, x0, baseline_y, v.text_color);
                }
                None => {
                    ctx.fill_text(&line.text, x0, baseline_y);
                }
            }
        }

        // ── Hint / placeholder when empty ──────────────────────────
        // Shown whenever the buffer is empty (egui shows hint text until the
        // first character is typed, regardless of focus), and honours the
        // same content alignment as real text.
        if st.text.is_empty() && !self.hint.is_empty() {
            ctx.set_fill_color(v.text_dim);
            let hint_w = measure_advance(&self.font, &self.hint, self.font_size);
            let slack = (self.inner_width() - hint_w).max(0.0);
            let hint_x = self.padding
                + match self.resolved_h_align() {
                    TextHAlign::Left => 0.0,
                    TextHAlign::Center => slack * 0.5,
                    TextHAlign::Right => slack,
                };
            let baseline_y = self.line_baseline_y(0);
            ctx.fill_text(&self.hint, hint_x, baseline_y);
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
        // Vertical scroll bar (floating overlay). Painted here — after the
        // cache blit — so its hover/fade animation stays live without forcing
        // the text bitmap to re-raster each animation frame.
        self.paint_scrollbar(ctx);

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
        // `paint_overlay` is drawn AFTER the cached content blit and is NOT
        // clipped by the framework, so a caret whose line has been scrolled
        // (wheel/drag) outside the padded inner rect would otherwise streak
        // over the border/padding. Clamp its vertical span to the inner band
        // and skip entirely when the line is fully off-screen.
        let inner_lo = self.padding;
        let inner_hi = (self.bounds.height - self.padding).max(inner_lo);
        let Some((y0, y1)) =
            caret_visible_segment(p.y, self.cached_line_h, inner_lo, inner_hi)
        else {
            return;
        };
        let v = ctx.visuals();
        ctx.set_stroke_color(v.text_color);
        ctx.set_line_width(1.5);
        ctx.begin_path();
        ctx.move_to(p.x, y0);
        ctx.line_to(p.x, y1);
        ctx.stroke();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                // Scroll bar takes priority: continue a thumb drag, or update
                // its hover state before the text-hover / selection logic.
                let bar_hover_changed = match self.scrollbar_on_mouse_move(*pos) {
                    super::scroll::ScrollMove::Dragging(moved) => {
                        if moved {
                            crate::animation::request_draw();
                        }
                        return EventResult::Consumed;
                    }
                    super::scroll::ScrollMove::Hover(changed) => changed,
                };
                let was = self.hovered;
                self.hovered = self.hit_test(*pos);
                if self.hovered {
                    set_cursor_icon(CursorIcon::Text);
                }
                if self.selecting_drag {
                    let off = self.byte_offset_at(*pos);
                    self.extend_selection_drag(off);
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                if was != self.hovered || bar_hover_changed {
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
                // Grabbing the scroll thumb must not also place the caret.
                if self.scrollbar_begin_drag(*pos) {
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                let off = self.byte_offset_at(*pos);
                let clicks = self.multi_click.register(*pos);
                self.begin_pointer_selection(off, clicks, modifiers.shift);
                self.selecting_drag = true;
                self.focus_time = Some(Instant::now());
                crate::animation::request_draw();
                EventResult::Consumed
            }
            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                self.scrollbar_end_drag();
                self.selecting_drag = false;
                EventResult::Consumed
            }
            Event::MouseWheel { delta_y, .. } => {
                if self.scroll_by_wheel(*delta_y) {
                    crate::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
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
                // Pre-default key-chord interception (e.g. the demo's Ctrl/Cmd+Y
                // case-toggle). A `true` return consumes the event and skips
                // built-in handling. Cloned out of `self` so the callback can
                // freely borrow the shared edit state we also hold.
                if let Some(cb) = self.on_key_chord.clone() {
                    let epoch_before = self.edit.borrow().epoch;
                    if (cb.borrow_mut())(key, modifiers) {
                        // An interceptor that edits text advances the content
                        // epoch (via `note_text_change`); mirror the built-in
                        // funnels by re-wrapping, firing `on_change`, and
                        // scrolling the (possibly moved) caret back into view.
                        if self.edit.borrow().epoch != epoch_before {
                            self.mark_dirty();
                            self.notify_change();
                            self.ensure_cursor_visible();
                        }
                        crate::animation::request_draw();
                        return EventResult::Consumed;
                    }
                }
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
                        // Ctrl/Cmd+Home → document start; plain Home → start of
                        // the current visual (wrapped) line.
                        let target = if cmd {
                            0
                        } else {
                            let cur = self.edit.borrow().cursor;
                            let line = self.line_for_cursor(cur);
                            self.cached_lines[line].start
                        };
                        self.move_cursor_to(target, shift);
                    }
                    Key::End => {
                        // Ctrl/Cmd+End → document end; plain End → end of the
                        // current visual (wrapped) line.
                        let target = if cmd {
                            self.edit.borrow().text.len()
                        } else {
                            let cur = self.edit.borrow().cursor;
                            let line = self.line_for_cursor(cur);
                            self.cached_lines[line].end
                        };
                        self.move_cursor_to(target, shift);
                    }
                    Key::PageUp => {
                        let n = self.page_lines() as isize;
                        self.move_lines(-n, shift);
                    }
                    Key::PageDown => {
                        let n = self.page_lines() as isize;
                        self.move_lines(n, shift);
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
                // Keep the caret on-screen after any edit or navigation
                // (re-wraps if the edit dirtied the cache, then scrolls).
                self.ensure_cursor_visible();
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
        // Focused → cursor blink; scrollbar animating → fade/expand tween.
        self.focused || self.scrollbar_animating()
    }
}
