//! The `Widget` impl for [`TextArea`]: focus and input-method hooks, layout
//! and the cached paint, the overlay pass (frame in band mode, scroll bar,
//! caret), and the app-level overlays (context menu, touch selection
//! handles and toolbar). Event handling is `event_impl.rs`.

use super::*;

pub(crate) use super::geometry::caret_visible_segment;

impl Widget for TextArea {
    crate::widgets::widget_as_any!();
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
        self.enabled_now()
    }

    fn is_enabled(&self) -> bool {
        self.enabled_now()
    }

    fn try_set_enabled(&mut self, enabled: bool) -> bool {
        // The inspector pins the state: a bound cell no longer overrides it.
        self.enabled_cell = None;
        self.set_enabled(enabled);
        true
    }

    fn focus_id(&self) -> Option<crate::focus::FocusId> {
        self.focus_request_id
    }

    fn accepts_text_input(&self) -> bool {
        self.enabled_now()
    }

    fn ime_cursor_area(&self) -> Option<Rect> {
        let st = self.edit.borrow();
        let cursor = st.cursor;
        drop(st);
        let p = self.pos_for_cursor(cursor);
        Some(Rect::new(p.x, p.y, 1.0, self.cached_line_h.max(1.0)))
    }

    fn text_input_value(&self) -> Option<String> {
        Some(self.committed_text())
    }

    fn text_before_caret(&self) -> Option<String> {
        self.text_up_to_caret()
    }

    fn resume_text_input(&mut self, caret: usize, composition: &str, in_text: bool) -> bool {
        self.resume_composition(caret, composition, in_text)
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
        // when the global toggle is on (default at scale ≤ 1.25; an explicit choice at any scale), grayscale
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
        let line_h = self.line_advance();
        (lines.len().max(1) as f64) * line_h + self.padding * 2.0
    }

    fn scroll_rect_into_view(&mut self, rect: Rect) -> bool {
        self.scroll_local_rect_into_view(rect)
    }

    fn layout(&mut self, available: Size) -> Size {
        // An owner's cell can disable a focused area without a call into it.
        if !self.enabled_now() && self.focused {
            self.release_focus_state();
        }
        if !self.enabled_now() {
            self.touch_selection.hide();
        }
        // Fill the slot we're given.  A parent that allocates us via
        // a flex-weight gets everything; a parent that asks for our
        // natural size gets the same (the caller is opting into
        // "whatever you want" with `available`).
        let w = available.width.max(self.padding * 2.0 + 20.0);
        let h = available
            .height
            .max(self.padding * 2.0 + self.min_text_height());
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

        // A touch handle held past the top or bottom edge keeps scrolling
        // (before the band is planned, so it plans for the new offset).
        crate::widgets::touch_selection::tick(self);

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
            self.sig_unrastered = true;
            self.cache.invalidate();
        } else if !self.sig_unrastered {
            // Only a dirty mark with no signature change behind it is cleared: a second layout
            // before the paint finds the same signature as the first, whose change the bitmap
            // still has to show.
            self.cache.dirty = false;
        }
        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        // TEMPORARY env-gated (AGG_PAINT_TIMING) per-section timing; see the emit
        // at the end of this fn (strip frames only). Cheap when off.
        use crate::widget::paint_timing as pt;
        let mut tt = pt::TextAreaPaintTiming::default();
        let mut t = pt::start();

        let v = ctx.visuals();
        let w = self.bounds.width;

        // The framework only calls `paint` when the backbuffer is dirty, so this
        // counts real re-rasters. The band tests assert in-band scrolling never
        // reaches here.
        self.raster_count.set(self.raster_count.get() + 1);
        self.sig_unrastered = false;

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
            self.strip_raster_count
                .set(self.strip_raster_count.get() + 1);
        }

        // Vertical extent painted into the buffer: the padded inner rect,
        // expanded by the over-scan margins in band mode, then narrowed to the
        // edited line strip when doing a partial re-raster.
        let (clip_lo, clip_hi) = match strip {
            Some((first, last)) => self.dirty_strip_y(first, last),
            None => self.band_clip_y(),
        };

        // Background — theme widget fill, shaped for band / strip / plain
        // rasters (none when frameless); see `frame.rs`.
        tt.head_ms = pt::ms(&t);
        t = pt::start();
        self.paint_background(ctx, &v, strip.map(|_| (clip_lo, clip_hi)));
        tt.bg_fill_ms = pt::ms(&t);

        // Clip content to the padded inner width and the (band-expanded, or
        // strip-narrowed) inner height so overflow text can't leak sideways
        // across the border. Text that bleeds into the top/bottom padding while
        // scrolling is covered by the padding-ring fill in `paint_overlay`.
        t = pt::start();
        ctx.clip_rect(
            self.padding,
            clip_lo,
            (w - self.padding * 2.0).max(0.0),
            (clip_hi - clip_lo).max(0.0),
        );

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        tt.clip_ms = pt::ms(&t);

        // ── Selection highlight ───────────────────────────────────
        t = pt::start();
        let st = self.edit.borrow().clone();
        tt.state_clone_ms = pt::ms(&t);
        t = pt::start(); // sel_loop
        if st.cursor != st.anchor {
            let lo = st.cursor.min(st.anchor);
            let hi = st.cursor.max(st.anchor);
            let hl_color = if self.focused {
                self.theme.selection_bg.unwrap_or(v.selection_bg)
            } else {
                self.theme
                    .selection_bg_unfocused
                    .unwrap_or(v.selection_bg_unfocused)
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
        tt.sel_loop_ms = pt::ms(&t);
        t = pt::start();
        let text_color = self.text_color(&v);
        // Muted text is the hint's colour, the theme's "not real text yet".
        let muted_color = self.theme.placeholder_color_for(self.focused, v.text_dim);
        ctx.set_fill_color(text_color);
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
            tt.painted_lines += 1;
            let baseline_y = self.line_baseline_y(i);
            let x0 = self.line_x_start(line);
            match self.line_spans(line, muted_color) {
                // Highlighted or muted path (`muted.rs`): paint each coloured
                // run at its measured x offset; gaps stay in the ambient
                // text colour.
                Some(spans) => {
                    let th = pt::start();
                    self.paint_highlighted_line(
                        ctx, &line.text, &spans, x0, baseline_y, text_color,
                    );
                    tt.hl_ms += pt::ms(&th);
                }
                None => {
                    ctx.fill_text(&line.text, x0, baseline_y);
                }
            }
        }
        tt.text_loop_ms = pt::ms(&t);

        // ── Hint / placeholder when empty ──────────────────────────
        // Shown whenever the buffer is empty (egui shows hint text until the
        // first character is typed, regardless of focus), and honours the
        // same content alignment as real text.
        t = pt::start();
        if st.text.is_empty() && !self.hint.is_empty() {
            let hint = self.theme.placeholder_color_for(self.focused, v.text_dim);
            ctx.set_fill_color(if self.enabled_now() {
                hint
            } else {
                Self::disabled_hint_color(hint)
            });
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

        tt.tail_ms = pt::ms(&t);
        if strip.is_some() && pt::enabled() {
            tt.emit();
        }
    }

    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        // Where the touch handles go, and whether a scrolled ancestor hides
        // the area (this pass runs inside the tree's clips).
        self.touch_selection.note_paint(ctx);
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
        self.paint_preedit_underline(ctx);
        if let Some(t) = self.focus_time {
            let phase = (crate::clock::since(t).as_millis() / 500) as u64;
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
        let Some((y0, y1)) = caret_visible_segment(p.y, self.cached_line_h, inner_lo, inner_hi)
        else {
            return;
        };
        let v = ctx.visuals();
        ctx.set_stroke_color(self.theme.cursor_color.unwrap_or(v.text_color));
        ctx.set_line_width(1.5);
        ctx.begin_path();
        ctx.move_to(p.x, y0);
        ctx.line_to(p.x, y1);
        ctx.stroke();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        self.handle_event(event)
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
        if self.touch_selection.needs_draw() {
            return true;
        }
        if !self.focused {
            return false;
        }
        let Some(t) = self.focus_time else {
            return false;
        };
        let current_phase = (crate::clock::since(t).as_millis() / 500) as u64;
        current_phase != self.blink_last_phase.get()
    }

    fn next_draw_deadline(&self) -> Option<web_time::Instant> {
        if !self.focused {
            return None;
        }
        let t = self.focus_time?;
        let ms = crate::clock::since(t).as_millis() as u64;
        let next_phase = (ms / 500) + 1;
        let blink = t + std::time::Duration::from_millis(next_phase * 500);
        // Android's caret handle fades on its own schedule.
        Some(match self.touch_selection.deadline() {
            Some(fade) => blink.min(fade),
            None => blink,
        })
    }

    /// While the right-click menu is open it must capture every event (clicks
    /// outside dismiss it, Escape closes it).
    fn has_active_modal(&self) -> bool {
        self.context_menu.is_open()
    }

    /// The open right-click menu's rows ("Cut Menu Item", ...) for GUI
    /// automation.
    fn find_named_targets(&self, name: &str, out: &mut Vec<crate::NamedTarget>) {
        self.context_menu.named_row_targets(name, out);
    }

    /// The context menu and the touch selection's handles and toolbar paint
    /// at app level so they can overflow the editor bounds.
    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        crate::widgets::touch_selection::paint(self, ctx);
        let font =
            crate::font_settings::current_system_font().unwrap_or_else(|| Arc::clone(&self.font));
        self.context_menu.paint(ctx, font, self.font_size);
    }

    /// Presses on the touch handles and toolbar, which reach outside the
    /// editor, come to it.
    fn hit_test_global_overlay(&self, local_pos: Point) -> bool {
        crate::widgets::touch_selection::hit_test(self, local_pos)
    }
}
