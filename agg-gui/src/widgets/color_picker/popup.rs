//! Round-swatch popup mode for [`super::ColorPicker`], like agg-sharp's
//! `ItemColorButton`: the widget is a circular swatch of a given diameter
//! and clicking it opens the colour panel as a floating popup instead of
//! expanding the widget in place.
//!
//! The popup is painted in the global overlay pass (`paint_global_overlay`,
//! clip reset, so it floats above and outside every ancestor) and claimed
//! for the pointer through `hit_test_global_overlay`, the same mechanism
//! `ComboBox` uses for its dropdown.  The picker claims the pointer for its
//! whole area and forwards clicks to the Cancel / Select / No Color
//! sub-widgets itself, so they never take focus; the picker is focusable in
//! this mode and a `FocusLost` (a click anywhere else) closes the popup,
//! keeping the colour picked so far, as agg-sharp's popup keeps the colour
//! its selector already applied.  Escape cancels.

use super::widget_impl::contains;
use super::*;
use crate::event::Key;

/// Gap between the swatch and the popup panel.
const POPUP_GAP: f64 = 4.0;

impl ColorPicker {
    /// Switch to the compact round-swatch mode: the widget is a circle of
    /// `diameter` showing the colour, and the panel opens as a popup below
    /// it (above it when there is no room below).
    pub fn with_round_popup_swatch(mut self, diameter: f64) -> Self {
        self.popup_swatch = Some(diameter.max(0.0));
        self
    }

    /// Outline the swatch with `color` at `width` px instead of
    /// `Visuals::widget_stroke` at 1 px (both modes).
    pub fn with_swatch_outline(mut self, color: Color, width: f64) -> Self {
        self.swatch_outline = Some((color, width));
        self
    }

    /// Whether the colour panel is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Popup mode: the panel's rect in widget-local coordinates — the
    /// panel's width and height, left-aligned with the swatch, below it
    /// (or above when `popup_opens_up`).
    pub(super) fn popup_panel_rect(&self) -> Rect {
        let d = self.popup_swatch.unwrap_or(0.0);
        let h = panel_body_h(self.allow_none);
        let y = if self.popup_opens_up {
            d + POPUP_GAP
        } else {
            -POPUP_GAP - h
        };
        Rect::new(0.0, y, PANEL_W, h)
    }

    /// Whether `p` (widget-local) is on the open popup panel.
    pub(super) fn pos_in_popup(&self, p: Point) -> bool {
        self.popup_swatch.is_some() && self.open && contains(&self.popup_panel_rect(), p)
    }

    /// Paint the round swatch: the colour in a circle with an outline (a
    /// fully transparent colour shows just the outline).
    pub(super) fn paint_round_swatch(&mut self, ctx: &mut dyn DrawCtx, swatch: Rect) {
        let v = ctx.visuals();
        let cur = if self.open {
            self.sync_color_from_hsva()
        } else {
            self.color_cell.get()
        };
        let r = swatch.width.min(swatch.height) * 0.5;
        let (cx, cy) = (
            swatch.x + swatch.width * 0.5,
            swatch.y + swatch.height * 0.5,
        );
        ctx.set_fill_color(cur);
        ctx.begin_path();
        ctx.circle(cx, cy, r);
        ctx.fill();
        let (outline, width) = self.swatch_outline.unwrap_or((v.widget_stroke, 1.0));
        if width > 0.0 {
            ctx.set_stroke_color(outline);
            ctx.set_line_width(width);
            ctx.begin_path();
            ctx.circle(cx, cy, (r - width * 0.5).max(0.0));
            ctx.stroke();
        }
    }

    /// Paint the open popup in the global overlay pass: pick the side with
    /// room, then the panel background, border and rows.
    pub(super) fn paint_popup(&mut self, ctx: &mut dyn DrawCtx) {
        // The widget's bottom in logical root space (Y-up), as ComboBox
        // derives it: the root transform carries the device scale, which
        // the logical geometry must not.
        let (mut x, mut y) = (0.0, 0.0);
        ctx.root_transform().transform(&mut x, &mut y);
        let _ = x;
        let y = y / crate::device_scale::device_scale().max(1e-6);
        let room_below = y - POPUP_GAP;
        let opens_up = room_below < panel_body_h(self.allow_none);
        if opens_up != self.popup_opens_up {
            self.popup_opens_up = opens_up;
            self.layout_panel_children();
        }

        let v = ctx.visuals();
        let r = self.regions();
        let p = r.panel;
        ctx.save();
        ctx.reset_clip();
        // Opaque backing first so the popup always hides what is beneath.
        ctx.set_fill_color(v.window_fill);
        ctx.begin_path();
        ctx.rounded_rect(p.x, p.y, p.width, p.height, 6.0);
        ctx.fill();
        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rounded_rect(p.x, p.y, p.width, p.height, 6.0);
        ctx.fill();
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(p.x, p.y, p.width, p.height, 6.0);
        ctx.stroke();
        self.paint_panel_rows(ctx, &r);
        ctx.restore();
    }

    /// Popup-mode events that differ from the inline picker: the swatch
    /// toggles the popup, Escape cancels, focus loss commits and closes.
    /// `None` hands the event to the shared handling.
    pub(super) fn handle_popup_event(&mut self, event: &Event) -> Option<EventResult> {
        let d = self.popup_swatch?;
        match event {
            Event::MouseDown {
                button: MouseButton::Left,
                pos,
                ..
            } if self.open && contains(&Rect::new(0.0, 0.0, d, d), *pos) => {
                self.close_committing();
                Some(EventResult::Consumed)
            }
            Event::MouseDown {
                button: MouseButton::Left,
                pos,
                ..
            } if self.open && !self.pos_in_popup(*pos) => {
                self.close_committing();
                Some(EventResult::Consumed)
            }
            Event::KeyDown {
                key: Key::Escape, ..
            } if self.open => {
                self.cancel();
                crate::animation::request_draw();
                Some(EventResult::Consumed)
            }
            Event::FocusLost if self.open => {
                self.drag = Drag::None;
                self.close_committing();
                Some(EventResult::Consumed)
            }
            _ => None,
        }
    }

    /// Close the popup keeping the working colour, as Select does: write
    /// the bound cell and fire `on_select`.
    pub(super) fn close_committing(&mut self) {
        let c = if self.none_cell.get() {
            Color::transparent()
        } else {
            let (r, g, b) = hsv_to_rgb(self.h, self.s, self.v);
            Color::rgba(r, g, b, self.a)
        };
        self.color_cell.set(c);
        if let Some(cb) = self.on_select.borrow_mut().as_mut() {
            cb(c);
        }
        self.finish_select();
        crate::animation::request_draw();
    }
}
