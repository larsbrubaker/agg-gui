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

    fn backbuffer_band(&self) -> Option<crate::widget::BackbufferBand> {
        self.backbuffer_band_impl()
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
        // Reset the wrap-change range for this pass; `refresh_wrap` records the
        // edited line range only when it actually re-wraps. (`sync_scroll` calls
        // `refresh_wrap` a second time below; a scroll-only frame must leave the
        // range cleared so no stale strip is planned.)
        self.wrap_change = None;
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

        // Re-plan the over-scan band now that wrap + offset are settled. This
        // only re-anchors (and thus changes the sig → re-raster) when the offset
        // has left the band; ordinary in-band scrolling is a no-op here.
        self.recompute_band();

        // Decide whether the next re-raster can be confined to the edited line
        // strip. Must run before `last_sig` is overwritten below (it diffs the
        // pending state against the previous signature).
        self.plan_dirty_strip();

        // The signature is the single source of truth for whether the cached
        // bitmap must re-raster: it captures every paint-affecting input (text,
        // cursor, selection, band anchor, focus/hover, size, alignment, font
        // size). Blink phase, the scrollbar and the border paint in
        // `paint_overlay`; an in-band scroll only shifts the blit; and
        // theme / typography / async invalidation is tracked by separate epochs
        // in `paint_subtree_backbuffered`.
        //
        // On a sig CHANGE we invalidate. On NO change we also *clear* any dirty
        // flag the event dispatcher set: a consumed wheel/scrollbar scroll bumps
        // the invalidation epoch (so retained ancestors like a Window FBO
        // recomposite the moved blit), which `dispatch_event` turns into a
        // `mark_dirty` on this widget — but for an in-band scroll that must be a
        // pure re-blit, NOT an expensive LCD re-raster. Making the sig
        // authoritative here is what lets the over-scan band actually pay off.
        let sig = self.cache_sig();
        if self.last_sig.as_ref() != Some(&sig) {
            self.last_sig = Some(sig);
            self.cache.invalidate();
        } else {
            self.cache.dirty = false;
        }
        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // The framework only calls `paint` when the backbuffer is dirty, so this
        // counts real re-rasters. The band tests assert in-band scrolling never
        // reaches here.
        self.raster_count.set(self.raster_count.get() + 1);

        // Band mode: raster at the band's fixed anchor (not the live offset) so
        // the bitmap is scroll-stable, and paint over-scan content above/below.
        let band = self.band.active;
        if band {
            self.render_band_offset.set(Some(self.band.anchor));
        }
        // Dirty-line-strip re-raster: an in-place edit repaints ONLY the changed
        // line strip into the retained band buffer (see `plan_dirty_strip`). It
        // engages only when the widget planned a strip AND the framework was
        // able to reuse the retained buffer (`partial_allowed`); otherwise this
        // is a full band repaint that covers every pixel with opaque content.
        let strip = if band && self.cache.partial_allowed {
            self.render_dirty_lines.get()
        } else {
            None
        };
        if strip.is_some() {
            self.strip_raster_count.set(self.strip_raster_count.get() + 1);
        }

        // Vertical extent painted into the buffer: the padded inner rect,
        // expanded by the over-scan margins in band mode, then narrowed to the
        // edited line strip when doing a partial re-raster.
        let (clip_lo, clip_hi) = match strip {
            Some((first, last)) => self.dirty_strip_y(first, last),
            None => self.band_clip_y(),
        };

        // Background — theme widget fill. In band mode fill the WHOLE buffer
        // (incl. the over-scan margins that sit outside the widget bounds) with
        // a plain opaque rect so scrolling never reveals an un-painted gap; the
        // rounded frame + border + padding-ring are re-established, fixed, in
        // `paint_overlay`. For a strip re-raster fill only the strip rows (full
        // width, opaque) so the changed lines' old pixels are replaced while the
        // retained buffer keeps every other row. Otherwise bake the rounded fill
        // into the cache.
        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        if strip.is_some() {
            ctx.rect(0.0, clip_lo, w, (clip_hi - clip_lo).max(0.0));
        } else if band {
            let bg_lo = -self.band.over_bottom;
            let bg_h = h + self.band.over_top + self.band.over_bottom;
            ctx.rect(0.0, bg_lo, w, bg_h.max(0.0));
        } else {
            ctx.rounded_rect(0.0, 0.0, w, h, 4.0);
        }
        ctx.fill();

        // Clip content to the padded inner width and the (band-expanded, or
        // strip-narrowed) inner height so overflow text can't leak sideways
        // across the border. Text that bleeds into the top/bottom padding while
        // scrolling is covered by the padding-ring fill in `paint_overlay`.
        ctx.clip_rect(
            self.padding,
            clip_lo,
            (w - self.padding * 2.0).max(0.0),
            (clip_hi - clip_lo).max(0.0),
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
                // Visible-range culling: skip lines wholly outside the (band-
                // expanded) clip band. Clipping already guarantees correctness;
                // this just avoids the per-line work for the thousands of lines
                // a large document keeps off-screen.
                let line_top = self.line_top_y(i);
                if line_top < clip_lo || line_top - self.cached_line_h > clip_hi {
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
            // Visible-range culling (see the selection loop): skip lines wholly
            // outside the clip band.
            let line_top = self.line_top_y(i);
            if line_top < clip_lo || line_top - self.cached_line_h > clip_hi {
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

        // Border: baked into the cache for the plain (non-scrolling) path so it
        // stays byte-identical. In band mode the border would scroll with the
        // blitted content, so it (plus the padding-ring bg that hides text bled
        // into the padding while scrolling) is drawn fixed in `paint_overlay`.
        if !band {
            self.paint_border(ctx, &v);
        }

        // Clear the band anchor override so hit-test / caret / scroll math
        // outside `paint` see the live offset again, and drop the strip request
        // so a later full re-raster is never mistaken for a strip repaint.
        self.render_band_offset.set(None);
        self.render_dirty_lines.set(None);
    }

    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        // Band mode: the border + padding-ring bg live here (fixed) instead of
        // in the scrolling cache. Drawn first so the scrollbar + caret land on
        // top, matching the plain path's z-order.
        if self.band.active {
            let v = ctx.visuals();
            self.paint_band_frame(ctx, &v);
        }

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
        // While the right-click menu is open it captures events (see
        // `has_active_modal`); route them through it first.
        if let Some(result) = self.route_context_menu(event) {
            return result;
        }
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
            Event::MouseDown {
                button: MouseButton::Right,
                pos,
                ..
            } => {
                if self.context_menu_enabled {
                    self.open_context_menu(*pos);
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
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
                // Word-wise navigation/deletion: Ctrl or Alt, matching egui
                // (`alt || ctrl`) and TextField.
                let word = modifiers.ctrl || modifiers.alt;
                match key {
                    Key::ArrowLeft => {
                        if word {
                            self.move_word(-1, shift);
                        } else {
                            self.move_char(-1, shift);
                        }
                    }
                    Key::ArrowRight => {
                        if word {
                            self.move_word(1, shift);
                        } else {
                            self.move_char(1, shift);
                        }
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
                        if word {
                            self.delete_word(-1);
                        } else {
                            self.delete(-1);
                        }
                    }
                    Key::Delete => {
                        if word {
                            self.delete_word(1);
                        } else {
                            self.delete(1);
                        }
                    }
                    Key::Enter => {
                        self.insert_str("\n");
                    }
                    Key::Tab => {
                        // Code-editor Tab: Shift+Tab always outdents; a plain
                        // Tab over a multi-line selection indents every touched
                        // line, otherwise it inserts a single indent (replacing
                        // any within-line selection).
                        let multi_line = {
                            let st = self.edit.borrow();
                            let (lo, hi) = (st.cursor.min(st.anchor), st.cursor.max(st.anchor));
                            hi > lo && st.text[lo..hi].contains('\n')
                        };
                        if shift {
                            self.outdent_selection();
                        } else if multi_line {
                            self.indent_selection();
                        } else {
                            self.insert_str("    ");
                        }
                    }
                    Key::Char('a') | Key::Char('A') if cmd => {
                        self.select_all_text();
                    }
                    Key::Char('c') | Key::Char('C') if cmd => {
                        self.clipboard_copy();
                    }
                    Key::Char('x') | Key::Char('X') if cmd => {
                        self.clipboard_cut();
                    }
                    Key::Char('v') | Key::Char('V') if cmd => {
                        self.clipboard_paste();
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
        // Scrollbar fade/expand tween is a genuine per-frame animation while it
        // runs, so it keeps requesting frames.  The cursor blink, by contrast,
        // is transition-scheduled: report dirty only when wall-clock time has
        // crossed a 500 ms flip boundary since the last painted phase.  An idle
        // focused editor therefore wakes twice a second (see
        // `next_draw_deadline`) instead of every frame.
        if self.scrollbar_animating() {
            return true;
        }
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

    /// While the right-click menu is open it must capture every event (clicks
    /// outside dismiss it, Escape closes it).
    fn has_active_modal(&self) -> bool {
        self.context_menu.is_open()
    }

    /// The context menu paints at app level so it can overflow the editor
    /// bounds and clamp to the viewport.
    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        let font = crate::font_settings::current_system_font()
            .unwrap_or_else(|| Arc::clone(&self.font));
        self.context_menu.paint(ctx, font, self.font_size);
    }
}
