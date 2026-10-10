//! `ToastHost` — the widget that shows a [`Toasts`] queue: it lays out one
//! child over its whole area and, in `paint_overlay` (after the child), paints
//! the toasts stacked at a corner, newest nearest the corner.
//!
//! The host claims the pointer only over a toast
//! (`claims_pointer_exclusively`), so everywhere else the child gets it as if
//! the host weren't there.  The pointer over a toast pauses every countdown
//! (resumed on `MouseOut` / `MouseLeave`, or when the toast under it goes
//! away); a left press on a toast dismisses it.  Toast geometry depends on
//! text measurement, so it is computed during paint and kept in
//! [`ToastHost::painted`] for hit testing.  The timing lives in `queue.rs`.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{Insets, WidgetBase};
use crate::text::{elide_text, EllipsisMode, Font};
use crate::widget::Widget;
use crate::widgets::popup::{Align, Align2};

use super::{ToastId, ToastKind, Toasts};

const PAD_X: f64 = 12.0;
const PAD_Y: f64 = 8.0;
/// Space between stacked toasts.
const GAP: f64 = 8.0;
/// Space between the icon and the text.
const ICON_GAP: f64 = 8.0;
/// Width of the accent stripe at a kind toast's leading edge.
const STRIPE_W: f64 = 4.0;
const RADIUS: f64 = 6.0;

/// One toast as the last paint drew it, in host-local coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct PaintedToast {
    pub id: ToastId,
    pub rect: Rect,
    /// Opacity it was drawn with (`0..=1`; below 1 while fading).
    pub alpha: f64,
    /// The text drawn — the toast's text, ellipsized when too wide.
    pub text: String,
}

/// See the module docs.
pub struct ToastHost {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    toasts: Toasts,
    font: Arc<Font>,
    icon_font: Option<Arc<Font>>,
    font_size: f64,
    anchor: Align2,
    inset: Insets,
    max_toast_width: f64,
    painted: Vec<PaintedToast>,
    /// Last pointer position over a toast, while it is there.
    pointer: Option<Point>,
}

impl ToastHost {
    /// Host `toasts` over `child`.  `font` draws the text — and the kind
    /// icons, unless [`Self::with_icon_font`] is set, so it should carry the
    /// Font Awesome face as a fallback ([`crate::fonts::standard_ui_font`]).
    pub fn new(child: Box<dyn Widget>, toasts: Toasts, font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![child],
            base: WidgetBase::new(),
            toasts,
            font,
            icon_font: None,
            font_size: crate::font_settings::default_font_size_or(13.0),
            anchor: Align2::RIGHT_BOTTOM,
            inset: Insets::all(16.0),
            max_toast_width: 460.0,
            painted: Vec::new(),
            pointer: None,
        }
    }

    /// Corner the toasts stack at (default bottom-right).  An `x` of
    /// `Center` centres them horizontally; a `y` of `Max` stacks down from
    /// the top, anything else up from the bottom.
    pub fn with_anchor(mut self, anchor: Align2) -> Self {
        self.anchor = anchor;
        self
    }

    /// Distance of the stack from the host's edges (default 16 all round) —
    /// e.g. a larger bottom inset to clear a status bar.
    pub fn with_inset(mut self, inset: Insets) -> Self {
        self.inset = inset;
        self
    }

    /// Widest a toast gets (default 460); longer text ends in "...".
    pub fn with_max_toast_width(mut self, w: f64) -> Self {
        self.max_toast_width = w;
        self
    }

    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self
    }

    /// Font for the kind icons (a Font Awesome face).
    pub fn with_icon_font(mut self, font: Arc<Font>) -> Self {
        self.icon_font = Some(font);
        self
    }

    /// The queue this host shows.
    pub fn toasts(&self) -> &Toasts {
        &self.toasts
    }

    /// The toasts the last paint drew, newest first.
    pub fn painted(&self) -> &[PaintedToast] {
        &self.painted
    }

    /// The toast drawn at `pos` (host-local), if any.
    pub fn toast_at(&self, pos: Point) -> Option<ToastId> {
        self.painted
            .iter()
            .find(|t| contains(t.rect, pos))
            .map(|t| t.id)
    }

    fn set_paused(&self, paused: bool) {
        let mut q = self.toasts.queue();
        if q.is_paused() != paused {
            q.set_paused(paused, crate::clock::now());
            // The resumed countdowns need their wake-up re-armed.
            crate::animation::request_draw_without_invalidation();
        }
    }

    /// The text font: the system font when one is set (like `Label`).
    fn text_font(&self) -> Arc<Font> {
        crate::font_settings::current_system_font().unwrap_or_else(|| Arc::clone(&self.font))
    }
}

fn contains(r: Rect, p: Point) -> bool {
    p.x >= r.x && p.x <= r.x + r.width && p.y >= r.y && p.y <= r.y + r.height
}

fn faded(c: Color, alpha: f64) -> Color {
    c.with_alpha(c.a * alpha as f32)
}

/// What one toast needs drawn, measured.
struct Measured {
    icon: Option<(String, Color, f64)>,
    text: String,
    text_w: f64,
}

impl ToastHost {
    /// Measure (and ellipsize) a toast's content with the fonts set on `ctx`.
    fn measure(
        &self,
        ctx: &mut dyn DrawCtx,
        text: &str,
        kind: Option<ToastKind>,
        text_font: &Arc<Font>,
        size: f64,
    ) -> Measured {
        let v = ctx.visuals();
        let icon = kind.map(|k| {
            let glyph = k.icon().to_string();
            ctx.set_font(Arc::clone(self.icon_font.as_ref().unwrap_or(text_font)));
            let w = ctx.measure_text(&glyph).map_or(size, |m| m.width);
            ctx.set_font(Arc::clone(text_font));
            (glyph, k.accent(&v), w)
        });
        let lead = icon.as_ref().map_or(0.0, |i| i.2 + ICON_GAP + STRIPE_W);
        let room = self
            .max_toast_width
            .min(self.bounds.width - self.inset.horizontal());
        let text_max = (room - lead - 2.0 * PAD_X).max(20.0);
        let measure = |t: &str| ctx.measure_text(t).map_or(0.0, |m| m.width);
        let text = elide_text(text, text_max, EllipsisMode::End, measure);
        let text_w = measure(&text);
        Measured { icon, text, text_w }
    }

    /// Draw every toast and record its geometry in `self.painted`.
    fn paint_toasts(&mut self, ctx: &mut dyn DrawCtx) {
        let now = crate::clock::now();
        let entries = {
            let mut q = self.toasts.queue();
            q.tick(now);
            q.entries.clone()
        };
        self.painted.clear();
        if entries.is_empty() {
            return;
        }
        let v = ctx.visuals();
        let font = self.text_font();
        let size = self.font_size * crate::font_settings::current_font_size_scale();
        ctx.set_font(Arc::clone(&font));
        ctx.set_font_size(size);
        let (ascent, descent) = ctx
            .measure_text("Ag")
            .map_or((size * 0.8, size * 0.2), |m| (m.ascent, m.descent));
        let h = ascent + descent + 2.0 * PAD_Y;
        let from_top = self.anchor.y == Align::Max;
        let mut offset = 0.0;
        for e in entries.iter().rev() {
            let m = self.measure(ctx, &e.text, e.kind, &font, size);
            let lead = m.icon.as_ref().map_or(0.0, |i| i.2 + ICON_GAP + STRIPE_W);
            let w = 2.0 * PAD_X + lead + m.text_w;
            let x = match self.anchor.x {
                Align::Min => self.inset.left,
                Align::Center => (self.bounds.width - w) * 0.5,
                Align::Max => self.bounds.width - self.inset.right - w,
            };
            let y = if from_top {
                self.bounds.height - self.inset.top - offset - h
            } else {
                self.inset.bottom + offset
            };
            if y < 0.0 || y + h > self.bounds.height {
                break;
            }
            offset += h + GAP;
            let alpha = e.alpha(now);
            let rect = Rect::new(x, y, w, h);
            paint_panel(ctx, rect, alpha, m.icon.as_ref().map(|i| i.1));
            let baseline = y + PAD_Y + descent;
            let mut tx = x + PAD_X;
            if let Some((glyph, accent, icon_w)) = &m.icon {
                tx += STRIPE_W;
                ctx.set_font(Arc::clone(self.icon_font.as_ref().unwrap_or(&font)));
                ctx.set_fill_color(faded(*accent, alpha));
                ctx.fill_text(glyph, tx, baseline);
                ctx.set_font(Arc::clone(&font));
                tx += icon_w + ICON_GAP;
            }
            ctx.set_fill_color(faded(v.text_color, alpha));
            ctx.fill_text(&m.text, tx, baseline);
            self.painted.push(PaintedToast {
                id: e.id,
                rect,
                alpha,
                text: m.text,
            });
        }
    }

    /// Keep frames coming while a fade runs, and wake for the next expiry.
    /// Resumes the countdowns when the toast under the pointer is gone.
    fn schedule(&mut self) {
        if self.pointer.is_some_and(|p| self.toast_at(p).is_none()) {
            self.pointer = None;
            self.set_paused(false);
        }
        let now = crate::clock::now();
        let q = self.toasts.queue();
        if q.is_animating(now) {
            crate::animation::request_draw_tagged("toast.fade");
        }
        if let Some(t) = q.next_deadline(now) {
            crate::animation::request_draw_after(t.saturating_duration_since(now));
        }
    }
}

/// The toast's panel: background, border, and a kind's accent stripe.
fn paint_panel(ctx: &mut dyn DrawCtx, r: Rect, alpha: f64, accent: Option<Color>) {
    let v = ctx.visuals();
    ctx.set_fill_color(faded(v.window_shadow, alpha));
    ctx.begin_path();
    ctx.rounded_rect(r.x + 1.0, r.y - 2.0, r.width, r.height, RADIUS);
    ctx.fill();
    ctx.set_fill_color(faded(v.window_fill, alpha));
    ctx.begin_path();
    ctx.rounded_rect(r.x, r.y, r.width, r.height, RADIUS);
    ctx.fill();
    ctx.set_stroke_color(faded(v.window_stroke, alpha));
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.rounded_rect(r.x + 0.5, r.y + 0.5, r.width - 1.0, r.height - 1.0, RADIUS);
    ctx.stroke();
    if let Some(accent) = accent {
        ctx.set_fill_color(faded(accent, alpha));
        ctx.begin_path();
        ctx.rounded_rect(r.x, r.y, STRIPE_W, r.height, STRIPE_W * 0.5);
        ctx.fill();
    }
}

impl Widget for ToastHost {
    crate::widgets::widget_as_any!();
    fn type_name(&self) -> &'static str {
        "ToastHost"
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
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn margin(&self) -> Insets {
        self.base.margin
    }

    fn layout(&mut self, available: Size) -> Size {
        for child in &mut self.children {
            child.layout(available);
            child.set_bounds(Rect::new(0.0, 0.0, available.width, available.height));
        }
        available
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        self.paint_toasts(ctx);
        self.schedule();
    }

    fn claims_pointer_exclusively(&self, local_pos: Point) -> bool {
        self.toast_at(local_pos).is_some()
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let over = self.toast_at(*pos).is_some();
                self.pointer = over.then_some(*pos);
                self.set_paused(over);
                if over {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => match self.toast_at(*pos) {
                Some(id) => {
                    self.toasts.dismiss(id);
                    EventResult::Consumed
                }
                None => EventResult::Ignored,
            },
            Event::MouseUp { pos, .. } if self.toast_at(*pos).is_some() => EventResult::Consumed,
            Event::MouseOut | Event::MouseLeave => {
                self.pointer = None;
                self.set_paused(false);
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    fn needs_draw(&self) -> bool {
        if !self.is_visible() {
            return false;
        }
        self.toasts.queue().is_animating(crate::clock::now())
            || self.children.iter().any(|c| c.needs_draw())
    }

    fn next_draw_deadline(&self) -> Option<web_time::Instant> {
        if !self.is_visible() {
            return None;
        }
        let own = self.toasts.queue().next_deadline(crate::clock::now());
        own.into_iter()
            .chain(self.children.iter().filter_map(|c| c.next_draw_deadline()))
            .min()
    }
}
