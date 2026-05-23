//! `impl Widget for ColorWheelPicker` — layout, paint, paint_overlay,
//! and on_event split out from `color_wheel_picker.rs` to keep each
//! file comfortably below the 800-line cap.
//!
//! The four gradient surfaces (`HueWheel`, `SvTriangle`, `AlphaTrack`,
//! `PreviewSwatches`) are concrete fields on the parent, NOT entries
//! in `children`.  We dispatch their paint manually via
//! [`paint_subtree`] — same pattern `Window` uses for its
//! `title_bar` sub-widget.  Events don't need to reach them (they're
//! paint-only) so leaving them out of the children tree keeps the
//! framework's hit-test path clean: clicks anywhere over the wheel
//! land on the picker itself, which interprets them as wheel /
//! triangle / alpha drag.

use std::sync::Arc;

use super::hsv_math::{
    barycentric, format_hex, parse_hex, rgb_to_hsv, sv_triangle_vertices, sv_to_point,
};
use super::*;
use crate::animation::request_draw;
use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::{paint_subtree, Widget};

/// Per-frame local-coord rectangles for each interactive region.
struct Regions {
    wheel: Rect,
    triangle: Rect,
    alpha: Rect,
    alpha_pct: Rect,
    hex: Rect,
    preview: Rect,
    nocolor: Option<Rect>,
    cancel: Rect,
    select: Rect,
}

impl ColorWheelPicker {
    /// Compute child rectangles in widget-local Y-up coords for the
    /// current `self.bounds` and configuration.  The wheel sits at the
    /// TOP, buttons at the bottom.
    ///
    /// The picker's content always occupies its *intrinsic*
    /// [`picker_width`] / [`picker_height`] box.  When the assigned
    /// bounds are larger (e.g. a `Window` wrapper stretches the
    /// picker to its full content width) the intrinsic box is
    /// centered horizontally and pinned to the top so the picker
    /// hugs the window's title bar.  This keeps `paint()`,
    /// `paint_overlay()`, and `on_event()` reading from the same
    /// layout regardless of what the parent assigns to
    /// `self.bounds` after `layout()` runs.
    fn regions(&self) -> Regions {
        let assigned_w = self.bounds.width.max(picker_width());
        let assigned_h = self.bounds.height.max(picker_height(self.allow_none, self.show_alpha));
        let inner_w = picker_width();
        let inner_h = picker_height(self.allow_none, self.show_alpha);
        let origin_x = ((assigned_w - inner_w) * 0.5).max(0.0);
        // Pin to the TOP of the assigned bounds in Y-up coords so the
        // wheel sits just below the window title bar even when the
        // window stretches taller than the intrinsic picker height.
        let origin_y = (assigned_h - inner_h).max(0.0);

        let w = inner_w;
        let h = inner_h;

        // Cursor walks downward from the top edge (Y-up: higher Y first).
        let mut y_top = h - PAD;

        let wheel = Rect::new(
            origin_x + (w - WHEEL_SIZE) * 0.5,
            origin_y + y_top - WHEEL_SIZE,
            WHEEL_SIZE,
            WHEEL_SIZE,
        );
        let triangle = wheel; // inscribed — same rect, painted on top
        y_top -= WHEEL_SIZE + ROW_GAP;

        let alpha = if self.show_alpha {
            let track_w = w - PAD * 2.0 - ALPHA_PCT_W - ROW_GAP;
            let r = Rect::new(origin_x + PAD, origin_y + y_top - ALPHA_H, track_w, ALPHA_H);
            y_top -= ALPHA_H + ROW_GAP;
            r
        } else {
            Rect::new(0.0, 0.0, 0.0, 0.0)
        };
        let alpha_pct = if self.show_alpha {
            Rect::new(
                alpha.x + alpha.width + ROW_GAP,
                alpha.y,
                ALPHA_PCT_W,
                ALPHA_H,
            )
        } else {
            Rect::new(0.0, 0.0, 0.0, 0.0)
        };

        let hex = Rect::new(origin_x + PAD, origin_y + y_top - HEX_H, w - PAD * 2.0, HEX_H);
        y_top -= HEX_H + ROW_GAP;

        let preview = Rect::new(origin_x + PAD, origin_y + y_top - PREVIEW_H, w - PAD * 2.0, PREVIEW_H);
        y_top -= PREVIEW_H + ROW_GAP;

        let nocolor = if self.allow_none {
            let r = Rect::new(origin_x + PAD, origin_y + y_top - NOCOLOR_H, w - PAD * 2.0, NOCOLOR_H);
            y_top -= NOCOLOR_H + ROW_GAP;
            Some(r)
        } else {
            None
        };
        let _ = y_top;

        let btn_y = origin_y + PAD;
        let btn_w = (w - PAD * 2.0 - ROW_GAP) * 0.5;
        let cancel = Rect::new(origin_x + PAD, btn_y, btn_w, BTN_H);
        let select = Rect::new(origin_x + PAD + btn_w + ROW_GAP, btn_y, btn_w, BTN_H);

        Regions {
            wheel,
            triangle,
            alpha,
            alpha_pct,
            hex,
            preview,
            nocolor,
            cancel,
            select,
        }
    }

    fn drain_pending_hex(&mut self) -> bool {
        let pending = self.pending_hex.borrow_mut().take();
        let Some(s) = pending else { return false };
        let Some(c) = parse_hex(&s) else {
            return false;
        };
        let (h, sat, val) = rgb_to_hsv(c.r, c.g, c.b);
        self.h = h;
        self.s = sat;
        self.v = val;
        self.a = c.a;
        if self.allow_none && self.pass_through {
            // Editing hex implies "I want THIS colour", not the
            // pass-through marker — disable the No-Color toggle so
            // both UIs stay consistent.
            self.pass_through = false;
            self.nocolor_cell.set(false);
        }
        true
    }

    fn drain_nocolor_toggle(&mut self) -> bool {
        if !self.allow_none {
            return false;
        }
        let cell_val = self.nocolor_cell.get();
        if cell_val != self.pass_through {
            self.pass_through = cell_val;
            true
        } else {
            false
        }
    }

    fn drain_button_flags(&mut self) -> ButtonOutcome {
        if self.cancel_flag.replace(false) {
            return ButtonOutcome::Cancel;
        }
        if self.select_flag.replace(false) {
            return ButtonOutcome::Select;
        }
        ButtonOutcome::None
    }

    fn commit_via_select(&mut self) {
        let snap = self.current_color();
        if let Some(cb) = self.on_select.as_mut() {
            cb(snap);
        }
    }

    fn revert_via_cancel(&mut self) {
        self.revert_to_saved();
        let restored_hex = format_hex(self.saved);
        if let Some(child) = self.children.get_mut(self.idx_hex) {
            child.set_label_text(&restored_hex);
        }
        self.fire_on_change();
        if let Some(cb) = self.on_cancel.as_mut() {
            cb();
        }
    }

    /// Push the current working colour into every gradient surface so
    /// their caches stay in sync.
    fn sync_gradients(&mut self) {
        let (r, g, b) = self.current_rgb();
        let working = if self.pass_through {
            Color::transparent()
        } else {
            Color::rgba(r, g, b, self.a)
        };
        self.triangle.set_hue(self.h);
        self.alpha_track.set_base_rgb(r, g, b);
        self.preview.set_old(self.saved);
        self.preview.set_new(working);
    }

    /// Re-place every gradient surface + interactive child against the
    /// current `self.bounds` + `regions()`.  Idempotent — safe to call
    /// repeatedly per frame.
    pub(crate) fn reposition_children_in_bounds(&mut self) {
        let r = self.regions();
        self.wheel.set_bounds(r.wheel);
        self.wheel.layout(Size::new(r.wheel.width, r.wheel.height));
        self.triangle.set_bounds(r.triangle);
        self.triangle.layout(Size::new(r.triangle.width, r.triangle.height));
        self.alpha_track.set_bounds(r.alpha);
        self.alpha_track.layout(Size::new(r.alpha.width, r.alpha.height));
        self.preview.set_bounds(r.preview);
        self.preview.layout(Size::new(r.preview.width, r.preview.height));

        let placements: Vec<(usize, Rect)> = {
            let mut v = vec![
                (self.idx_hex, r.hex),
                (self.idx_cancel, r.cancel),
                (self.idx_select, r.select),
            ];
            if let (Some(nc), Some(idx)) = (r.nocolor, self.idx_nocolor) {
                v.push((idx, nc));
            }
            v
        };
        for (idx, rect) in placements {
            if let Some(child) = self.children.get_mut(idx) {
                child.layout(Size::new(rect.width, rect.height));
                child.set_bounds(rect);
            }
        }
    }

    fn push_hex_text_if_idle(&mut self) {
        if self.drag != Drag::None {
            return;
        }
        let hex = if self.pass_through {
            format_hex(Color::transparent())
        } else {
            let (r, g, b) = self.current_rgb();
            format_hex(Color::rgba(r, g, b, self.a))
        };
        if let Some(child) = self.children.get_mut(self.idx_hex) {
            child.set_label_text(&hex);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ButtonOutcome {
    None,
    Cancel,
    Select,
}

// ── Widget impl ──────────────────────────────────────────────────────────────

impl Widget for ColorWheelPicker {
    fn type_name(&self) -> &'static str {
        "ColorWheelPicker"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        let resized = (b.width - self.bounds.width).abs() > f64::EPSILON
            || (b.height - self.bounds.height).abs() > f64::EPSILON;
        self.bounds = b;
        // Parents (notably `Window`) reset our bounds AFTER calling
        // `layout()`, which leaves the child gradient surfaces holding
        // their pre-resize positions.  Re-place every child against
        // the new bounds so `paint()` and `paint_overlay()` stay in
        // lockstep — without this the dots/handles drift out of the
        // wheel after the Window stretches the picker to its content
        // width.
        if resized {
            self.reposition_children_in_bounds();
        }
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
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
    fn margin(&self) -> Insets {
        self.base.margin
    }

    fn min_size(&self) -> Size {
        Size::new(
            picker_width(),
            picker_height(self.allow_none, self.show_alpha),
        )
    }

    fn layout(&mut self, available: Size) -> Size {
        // Drain external mutations BEFORE positioning children so bounds
        // and gradient caches reflect the post-drain colour.  We also
        // drain the Cancel / Select button flags here: clicks on those
        // buttons get consumed by the Button child, so the picker's
        // `on_event` never runs for that input — the next `layout()` is
        // therefore the earliest reliable point to act on them.
        match self.drain_button_flags() {
            ButtonOutcome::Cancel => self.revert_via_cancel(),
            ButtonOutcome::Select => self.commit_via_select(),
            ButtonOutcome::None => {}
        }
        let hex_changed = self.drain_pending_hex();
        let nocolor_changed = self.drain_nocolor_toggle();
        if hex_changed || nocolor_changed {
            self.fire_on_change();
        }
        self.sync_gradients();
        self.push_hex_text_if_idle();

        let want_w = picker_width().min(available.width.max(picker_width()));
        let want_h = picker_height(self.allow_none, self.show_alpha)
            .min(available.height.max(picker_height(self.allow_none, self.show_alpha)));
        self.bounds = Rect::new(0.0, 0.0, want_w, want_h);

        self.reposition_children_in_bounds();

        Size::new(want_w, want_h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();

        // Panel background.
        ctx.set_fill_color(v.window_fill);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, self.bounds.width, self.bounds.height, 6.0);
        ctx.fill();
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, self.bounds.width, self.bounds.height, 6.0);
        ctx.stroke();

        let r = self.regions();

        // Paint the gradient surfaces.  paint_subtree handles the ctx
        // translation + back-buffer cache machinery for each child.
        paint_field(ctx, &mut self.wheel);
        paint_field(ctx, &mut self.triangle);
        if self.show_alpha {
            paint_field(ctx, &mut self.alpha_track);
        }
        paint_field(ctx, &mut self.preview);

        // Alpha percentage label.
        if self.show_alpha {
            ctx.set_font(Arc::clone(&self.font));
            ctx.set_font_size(self.font_size);
            ctx.set_fill_color(v.text_color);
            let pct = (self.a * 100.0).round() as i32;
            let label = format!("{pct}%");
            if let Some(m) = ctx.measure_text(&label) {
                let tx = r.alpha_pct.x + ((r.alpha_pct.width - m.width) * 0.5).max(0.0);
                let ty = r.alpha_pct.y
                    + ((r.alpha_pct.height - self.font_size) * 0.5 + 2.0).max(0.0);
                ctx.fill_text(&label, tx, ty);
            }
        }
    }

    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        let r = self.regions();

        // Wheel selector dot — centred in the ring annulus at the
        // current hue angle.
        let wheel_outer = 0.5 * r.wheel.width.min(r.wheel.height) * WHEEL_OUTER_RATIO;
        let wheel_inner = 0.5 * r.wheel.width.min(r.wheel.height) * WHEEL_INNER_RATIO;
        let wheel_ring_r = (wheel_outer + wheel_inner) * 0.5;
        let wheel_cx = r.wheel.x + r.wheel.width * 0.5;
        let wheel_cy = r.wheel.y + r.wheel.height * 0.5;
        let hue_rad = (self.h as f64).to_radians();
        let dot_x = wheel_cx + wheel_ring_r * hue_rad.cos();
        let dot_y = wheel_cy + wheel_ring_r * hue_rad.sin();
        paint_handle_dot(ctx, dot_x, dot_y, 6.0);

        // SV crosshair.
        let tri_radius = 0.5 * r.triangle.width.min(r.triangle.height) * TRIANGLE_RATIO;
        let tri_cx = r.triangle.x + r.triangle.width * 0.5;
        let tri_cy = r.triangle.y + r.triangle.height * 0.5;
        let (v1, v2, v3) = sv_triangle_vertices(tri_cx, tri_cy, tri_radius, self.h);
        let (sx, sy) = sv_to_point(self.s, self.v, v1, v2, v3);
        paint_handle_dot(ctx, sx, sy, 5.0);

        // Alpha thumb (vertical bar).
        if self.show_alpha && r.alpha.width > 0.0 {
            let thumb_x = r.alpha.x + (self.a as f64).clamp(0.0, 1.0) * r.alpha.width;
            ctx.set_stroke_color(Color::white());
            ctx.set_line_width(3.0);
            ctx.begin_path();
            ctx.move_to(thumb_x, r.alpha.y);
            ctx.line_to(thumb_x, r.alpha.y + r.alpha.height);
            ctx.stroke();
            ctx.set_stroke_color(Color::black());
            ctx.set_line_width(1.5);
            ctx.begin_path();
            ctx.move_to(thumb_x, r.alpha.y);
            ctx.line_to(thumb_x, r.alpha.y + r.alpha.height);
            ctx.stroke();
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        // Process side-effects from child widgets (buttons / checkbox /
        // hex field) BEFORE our own region tests.  The flags were set
        // during the dispatch_event descent into our children.
        let outcome = self.drain_button_flags();
        match outcome {
            ButtonOutcome::Cancel => {
                self.revert_via_cancel();
                request_draw();
                return EventResult::Consumed;
            }
            ButtonOutcome::Select => {
                self.commit_via_select();
                request_draw();
                return EventResult::Consumed;
            }
            ButtonOutcome::None => {}
        }
        if self.drain_nocolor_toggle() {
            self.fire_on_change();
            request_draw();
        }
        if self.drain_pending_hex() {
            self.fire_on_change();
            request_draw();
        }

        let r = self.regions();

        match event {
            Event::MouseDown {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                let wheel_cx = r.wheel.x + r.wheel.width * 0.5;
                let wheel_cy = r.wheel.y + r.wheel.height * 0.5;
                let local = Point::new(pos.x - wheel_cx, pos.y - wheel_cy);
                let outer = 0.5 * r.wheel.width.min(r.wheel.height) * WHEEL_OUTER_RATIO;
                let inner = 0.5 * r.wheel.width.min(r.wheel.height) * WHEEL_INNER_RATIO;
                let r2 = local.x * local.x + local.y * local.y;
                let in_ring = r2 >= inner * inner && r2 <= outer * outer;
                if in_ring {
                    let mut deg = local.y.atan2(local.x).to_degrees();
                    if deg < 0.0 {
                        deg += 360.0;
                    }
                    self.h = deg as f32;
                    self.drag = Drag::Hue;
                    self.sync_gradients();
                    self.push_hex_text_if_idle();
                    self.fire_on_change();
                    request_draw();
                    return EventResult::Consumed;
                }
                if point_in_triangle(*pos, &r.triangle, self.h) {
                    if let Some((s, v)) = sv_from_point(*pos, &r.triangle, self.h) {
                        self.s = s;
                        self.v = v;
                        self.drag = Drag::Sv;
                        self.sync_gradients();
                        self.push_hex_text_if_idle();
                        self.fire_on_change();
                        request_draw();
                        return EventResult::Consumed;
                    }
                }
                if self.show_alpha && rect_contains(&r.alpha, *pos) {
                    self.a = ((pos.x - r.alpha.x) / r.alpha.width).clamp(0.0, 1.0) as f32;
                    self.drag = Drag::Alpha;
                    self.sync_gradients();
                    self.push_hex_text_if_idle();
                    self.fire_on_change();
                    request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseMove { pos } => {
                if self.drag == Drag::None {
                    return EventResult::Ignored;
                }
                match self.drag {
                    Drag::Hue => {
                        let wheel_cx = r.wheel.x + r.wheel.width * 0.5;
                        let wheel_cy = r.wheel.y + r.wheel.height * 0.5;
                        let local = Point::new(pos.x - wheel_cx, pos.y - wheel_cy);
                        let mut deg = local.y.atan2(local.x).to_degrees();
                        if deg < 0.0 {
                            deg += 360.0;
                        }
                        self.h = deg as f32;
                    }
                    Drag::Sv => {
                        let (s, v) = clamped_sv_from_point(*pos, &r.triangle, self.h);
                        self.s = s;
                        self.v = v;
                    }
                    Drag::Alpha => {
                        self.a = ((pos.x - r.alpha.x) / r.alpha.width).clamp(0.0, 1.0) as f32;
                    }
                    Drag::None => {}
                }
                self.sync_gradients();
                self.fire_on_change();
                request_draw();
                EventResult::Consumed
            }
            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                let was_dragging = self.drag != Drag::None;
                self.drag = Drag::None;
                if was_dragging {
                    self.push_hex_text_if_idle();
                    request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }
}

// ── Tiny helpers (kept here so the impl block stays self-contained) ──────────

fn rect_contains(r: &Rect, p: Point) -> bool {
    p.x >= r.x && p.x <= r.x + r.width && p.y >= r.y && p.y <= r.y + r.height
}

fn paint_field(ctx: &mut dyn DrawCtx, field: &mut dyn Widget) {
    let b = field.bounds();
    if b.width <= 0.0 || b.height <= 0.0 {
        return;
    }
    ctx.save();
    ctx.translate(b.x, b.y);
    paint_subtree(field, ctx);
    ctx.restore();
}

fn paint_handle_dot(ctx: &mut dyn DrawCtx, x: f64, y: f64, r: f64) {
    ctx.set_stroke_color(Color::white());
    ctx.set_line_width(3.0);
    ctx.begin_path();
    ctx.circle(x, y, r);
    ctx.stroke();
    ctx.set_stroke_color(Color::black());
    ctx.set_line_width(1.5);
    ctx.begin_path();
    ctx.circle(x, y, r);
    ctx.stroke();
}

fn point_in_triangle(p: Point, triangle_rect: &Rect, hue_deg: f32) -> bool {
    let cx = triangle_rect.x + triangle_rect.width * 0.5;
    let cy = triangle_rect.y + triangle_rect.height * 0.5;
    let radius = 0.5 * triangle_rect.width.min(triangle_rect.height) * TRIANGLE_RATIO;
    let (v1, v2, v3) = sv_triangle_vertices(cx, cy, radius, hue_deg);
    let (w1, w2, w3) = barycentric((p.x, p.y), v1, v2, v3);
    w1 >= 0.0 && w2 >= 0.0 && w3 >= 0.0
}

fn sv_from_point(p: Point, triangle_rect: &Rect, hue_deg: f32) -> Option<(f32, f32)> {
    let cx = triangle_rect.x + triangle_rect.width * 0.5;
    let cy = triangle_rect.y + triangle_rect.height * 0.5;
    let radius = 0.5 * triangle_rect.width.min(triangle_rect.height) * TRIANGLE_RATIO;
    let (v1, v2, v3) = sv_triangle_vertices(cx, cy, radius, hue_deg);
    let (w1, w2, _w3) = barycentric((p.x, p.y), v1, v2, v3);
    let val = (w1 + w2).clamp(0.0, 1.0);
    if val <= f64::EPSILON {
        return Some((0.0, 0.0));
    }
    let sat = (w1 / val).clamp(0.0, 1.0);
    Some((sat as f32, val as f32))
}

/// Like [`sv_from_point`] but always returns a value — used during a
/// drag where the cursor may stray outside the triangle.  Clamps the
/// barycentric weights then renormalises so `(s, v) ∈ [0, 1]²`.
fn clamped_sv_from_point(p: Point, triangle_rect: &Rect, hue_deg: f32) -> (f32, f32) {
    let cx = triangle_rect.x + triangle_rect.width * 0.5;
    let cy = triangle_rect.y + triangle_rect.height * 0.5;
    let radius = 0.5 * triangle_rect.width.min(triangle_rect.height) * TRIANGLE_RATIO;
    let (v1, v2, v3) = sv_triangle_vertices(cx, cy, radius, hue_deg);
    let (mut w1, mut w2, mut w3) = barycentric((p.x, p.y), v1, v2, v3);
    w1 = w1.clamp(0.0, 1.0);
    w2 = w2.clamp(0.0, 1.0);
    w3 = w3.clamp(0.0, 1.0);
    let sum = w1 + w2 + w3;
    if sum > 0.0 {
        w1 /= sum;
        w2 /= sum;
        w3 /= sum;
    }
    let val = (w1 + w2).clamp(0.0, 1.0);
    let sat = if val > f64::EPSILON {
        (w1 / val).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let _ = w3;
    (sat as f32, val as f32)
}
