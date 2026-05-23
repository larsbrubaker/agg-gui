//! `ColorWheelPicker` — circular hue wheel + saturation/value triangle
//! colour picker, modelled on NodeDesigner's `color-picker.js`.
//!
//! # Composition
//!
//! ```text
//! ColorWheelPicker (composite, drag-state owner)
//!   ├── HueWheel        (paint-only, GPU-cached annular ring)
//!   ├── SvTriangle      (paint-only, GPU-cached SV gradient, keyed on hue)
//!   ├── AlphaTrack      (paint-only, GPU-cached checkerboard + alpha ramp)
//!   ├── PreviewSwatches (paint-only, GPU-cached old | new split)
//!   ├── TextField       (#RRGGBB[AA] hex input)
//!   ├── Button          (Cancel)
//!   ├── Button          (Select)
//!   └── Checkbox        (No Color (Pass Through), optional)
//! ```
//!
//! Each gradient surface caches an `Arc<Vec<u8>>` of pre-rendered RGBA
//! pixels and blits it via `draw_image_rgba_arc`, which keys the GL
//! backend's texture cache on the `Arc`'s pointer identity — that's
//! the **hardware back-buffer** promised by the spec.  Selector handles
//! (wheel cursor, triangle crosshair, alpha thumb) are painted in
//! [`paint_overlay`](Widget::paint_overlay) so they never invalidate
//! the cached pixel buffers below.
//!
//! The gradient surfaces are stored as concrete fields (not in
//! `children`) so this widget can call typed setters on them
//! (`set_hue`, `set_base_rgb`, `set_old`/`set_new`) without
//! downcasting.  Window does the same with its title-bar sub-widget;
//! see `widget_impl.rs` for the paint dispatch.  The hex `TextField`,
//! the Cancel / Select buttons, and the optional `No Color` checkbox
//! all sit in `children` because they need standard event / focus
//! routing.
//!
//! # Coordinates
//!
//! Local Y-up: the wheel sits at the **top** of the picker, the
//! Cancel/Select buttons at the **bottom**.  Wheel angles increase
//! counter-clockwise from `+X` (`atan2(local_y, local_x)`), so
//! `hue = 0°` lives at 3 o'clock and `hue = 90°` at 12 o'clock.
//!
//! # Public API
//!
//! ```ignore
//! let picker = ColorWheelPicker::new(initial, font)
//!     .with_allow_none(true)              // include the No Color checkbox
//!     .with_show_alpha(true)              // alpha track + percent label
//!     .on_change(|c| { /* live preview */ })
//!     .on_select(|c| { /* commit */ })
//!     .on_cancel(|| { /* restore */ });
//!
//! // Standalone:
//! let widget: Box<dyn Widget> = Box::new(picker);
//!
//! // Popup dialog:
//! let dialog = color_wheel_picker_dialog(picker, "Color Picker");
//! ```
//!
//! `None` represents the "pass-through" choice when `allow_none` is on;
//! `Some(color)` is the explicit colour the user committed.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::geometry::Rect;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::button::Button;
use crate::widgets::checkbox::Checkbox;
use crate::widgets::text_field::TextField;

pub mod alpha_track;
pub mod dialog;
pub mod hsv_math;
pub mod hue_wheel;
pub mod preview_swatches;
pub mod sv_triangle;
mod widget_impl;

#[cfg(test)]
mod tests;

pub use alpha_track::AlphaTrack;
pub use dialog::color_wheel_picker_dialog;
pub use hue_wheel::HueWheel;
pub use preview_swatches::PreviewSwatches;
pub use sv_triangle::SvTriangle;

use hsv_math::{format_hex, hsv_to_rgb, rgb_to_hsv};

// ── Layout constants ─────────────────────────────────────────────────────────

pub(crate) const PAD: f64 = 10.0;
pub(crate) const ROW_GAP: f64 = 6.0;
pub(crate) const WHEEL_SIZE: f64 = 200.0;
pub(crate) const ALPHA_H: f64 = 22.0;
/// Matches agg-gui `TextField`'s natural height at `font_size = 13`:
/// `max(13 * 2.4, 28) ≈ 31.2`, rounded up so a tighter row doesn't
/// clip the field's focus ring.
pub(crate) const HEX_H: f64 = 32.0;
pub(crate) const PREVIEW_H: f64 = 32.0;
/// Matches agg-gui `Checkbox`'s natural height
/// (`BOX_SIZE + FOCUS_PAD * 2 = 20`).
pub(crate) const NOCOLOR_H: f64 = 20.0;
/// Matches agg-gui `Button`'s natural height at `font_size = 13`:
/// `max(13 * 1.7, 24) = 24`.  Reserving a larger height here would
/// push the label off-centre because Button's `layout` returns the
/// natural size and centres the label inside it — the leftover
/// vertical space ends up below the label in Y-up coords.
pub(crate) const BTN_H: f64 = 24.0;
pub(crate) const ALPHA_PCT_W: f64 = 44.0;
/// Outer hue-ring radius as a fraction of `min(w, h) / 2` — NodeDesigner
/// uses `85 / 95` on its 190 / 95 canvas.
pub(crate) const WHEEL_OUTER_RATIO: f64 = 85.0 / 95.0;
/// Inner hue-ring radius as a fraction of `min(w, h) / 2`.
pub(crate) const WHEEL_INNER_RATIO: f64 = 60.0 / 95.0;
/// SV triangle radius as a fraction of `min(w, h) / 2`.  NodeDesigner
/// inscribes at `inner_radius - 5` on a 95-half canvas (~`58/95`); we
/// match 55 / 95 so the triangle vertices sit just inside the ring.
pub(crate) const TRIANGLE_RATIO: f64 = 55.0 / 95.0;

pub(crate) fn picker_width() -> f64 {
    WHEEL_SIZE + PAD * 2.0
}

pub(crate) fn picker_height(allow_none: bool, show_alpha: bool) -> f64 {
    let mut h = PAD;
    h += WHEEL_SIZE + ROW_GAP;
    if show_alpha {
        h += ALPHA_H + ROW_GAP;
    }
    h += HEX_H + ROW_GAP;
    h += PREVIEW_H + ROW_GAP;
    if allow_none {
        h += NOCOLOR_H + ROW_GAP;
    }
    h += BTN_H + PAD;
    h
}

// ── Drag mode ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Drag {
    None,
    Hue,
    Sv,
    Alpha,
}

// ── Composite widget ─────────────────────────────────────────────────────────

/// Circular hue + SV-triangle colour picker.
pub struct ColorWheelPicker {
    pub(crate) bounds: Rect,
    pub(crate) base: WidgetBase,
    /// Children that participate in framework event + focus routing:
    /// `[TextField, Cancel Button, Select Button, optional Checkbox]`.
    pub(crate) children: Vec<Box<dyn Widget>>,

    /// Paint-only gradient surfaces — kept concrete so we can call
    /// typed setters (`set_hue` etc.) without downcasting.  Manually
    /// painted from `paint()` via `paint_subtree`.
    pub(crate) wheel: HueWheel,
    pub(crate) triangle: SvTriangle,
    pub(crate) alpha_track: AlphaTrack,
    pub(crate) preview: PreviewSwatches,

    pub(crate) font: Arc<Font>,
    pub(crate) font_size: f64,

    // ── HSV state (canonical) ────────────────────────────────────────────
    pub(crate) h: f32,
    pub(crate) s: f32,
    pub(crate) v: f32,
    pub(crate) a: f32,
    pub(crate) pass_through: bool,

    /// Colour observed at `new()` time — restored on Cancel.
    pub(crate) saved: Color,

    // ── Configuration ────────────────────────────────────────────────────
    pub(crate) allow_none: bool,
    pub(crate) show_alpha: bool,

    // ── Interaction state ────────────────────────────────────────────────
    pub(crate) drag: Drag,

    // ── Child indices in `children` ──────────────────────────────────────
    pub(crate) idx_hex: usize,
    pub(crate) idx_cancel: usize,
    pub(crate) idx_select: usize,
    pub(crate) idx_nocolor: Option<usize>,

    // ── Shared flags / cells written by child callbacks ──────────────────
    pub(crate) cancel_flag: Rc<Cell<bool>>,
    pub(crate) select_flag: Rc<Cell<bool>>,
    pub(crate) nocolor_cell: Rc<Cell<bool>>,
    /// Hex string the TextField pushes here on every text change.  The
    /// picker layout pass picks it up, attempts to parse, and resets the
    /// Option to `None` once handled.
    pub(crate) pending_hex: Rc<RefCell<Option<String>>>,

    // ── Callbacks ────────────────────────────────────────────────────────
    pub(crate) on_change: Option<Box<dyn FnMut(Option<Color>)>>,
    pub(crate) on_select: Option<Box<dyn FnMut(Option<Color>)>>,
    pub(crate) on_cancel: Option<Box<dyn FnMut()>>,
}

impl ColorWheelPicker {
    /// Build a picker initialised to `initial`.
    pub fn new(initial: Color, font: Arc<Font>) -> Self {
        let (h, s, v) = rgb_to_hsv(initial.r, initial.g, initial.b);
        let mut me = Self {
            bounds: Rect::default(),
            base: WidgetBase::new(),
            children: Vec::new(),
            wheel: HueWheel::new(),
            triangle: SvTriangle::new(),
            alpha_track: AlphaTrack::new(),
            preview: PreviewSwatches::new(),
            font,
            font_size: 13.0,
            h,
            s,
            v,
            a: initial.a,
            pass_through: initial.a <= 0.0,
            saved: initial,
            allow_none: false,
            show_alpha: true,
            drag: Drag::None,
            idx_hex: 0,
            idx_cancel: 1,
            idx_select: 2,
            idx_nocolor: None,
            cancel_flag: Rc::new(Cell::new(false)),
            select_flag: Rc::new(Cell::new(false)),
            nocolor_cell: Rc::new(Cell::new(false)),
            pending_hex: Rc::new(RefCell::new(None)),
            on_change: None,
            on_select: None,
            on_cancel: None,
        };
        me.build_children();
        me
    }

    /// Show the **No Color (Pass Through)** checkbox.  Default: `false`.
    /// When checked, `on_change` / `on_select` deliver `None`.
    pub fn with_allow_none(mut self, allow: bool) -> Self {
        self.allow_none = allow;
        self.nocolor_cell.set(allow && self.pass_through);
        self.build_children();
        self
    }

    /// Show the alpha track + percent label below the wheel.
    /// Default: `true`.
    pub fn with_show_alpha(mut self, show: bool) -> Self {
        self.show_alpha = show;
        self
    }

    /// Optional font-size override (default 13 logical pixels).
    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self
    }

    /// Live preview callback — fires every time the working colour
    /// changes (wheel / triangle / alpha drag, hex edit, "No Color"
    /// toggle, etc.).  `None` payload = pass-through.
    pub fn on_change(mut self, cb: impl FnMut(Option<Color>) + 'static) -> Self {
        self.on_change = Some(Box::new(cb));
        self
    }

    /// Commit callback — fires when **Select** is clicked.  `None`
    /// payload = pass-through.
    pub fn on_select(mut self, cb: impl FnMut(Option<Color>) + 'static) -> Self {
        self.on_select = Some(Box::new(cb));
        self
    }

    /// Cancel callback — fires when **Cancel** is clicked.  The picker
    /// internally resets its HSV state to the colour observed at `new()`
    /// before invoking this; `on_change` is also re-fired with the
    /// restored colour so listeners see the snap-back.
    pub fn on_cancel(mut self, cb: impl FnMut() + 'static) -> Self {
        self.on_cancel = Some(Box::new(cb));
        self
    }

    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }

    /// Snapshot of the working colour (`Some` when not pass-through).
    pub fn current_color(&self) -> Option<Color> {
        if self.pass_through {
            None
        } else {
            let (r, g, b) = hsv_to_rgb(self.h, self.s, self.v);
            Some(Color::rgba(r, g, b, self.a))
        }
    }

    /// Saturated `(r, g, b)` for the current `(h, s, v)`.  Used by the
    /// alpha track and overlay handle painters.
    pub(crate) fn current_rgb(&self) -> (f32, f32, f32) {
        hsv_to_rgb(self.h, self.s, self.v)
    }

    fn build_children(&mut self) {
        self.children.clear();

        let cancel_flag = Rc::clone(&self.cancel_flag);
        let select_flag = Rc::clone(&self.select_flag);
        let pending_hex = Rc::clone(&self.pending_hex);

        let initial_hex = format_hex(self.saved);
        let hex_field = TextField::new(Arc::clone(&self.font))
            .with_font_size(self.font_size)
            .with_text(initial_hex)
            .with_placeholder("#RRGGBB")
            .on_change(move |s| {
                *pending_hex.borrow_mut() = Some(s.to_string());
            });

        let cancel = Button::new("Cancel", Arc::clone(&self.font))
            .with_font_size(self.font_size)
            .with_subtle()
            .on_click(move || cancel_flag.set(true));
        let select = Button::new("Select", Arc::clone(&self.font))
            .with_font_size(self.font_size)
            .on_click(move || select_flag.set(true));

        self.children.push(Box::new(hex_field));
        self.children.push(Box::new(cancel));
        self.children.push(Box::new(select));

        self.idx_hex = 0;
        self.idx_cancel = 1;
        self.idx_select = 2;

        if self.allow_none {
            let nocolor_cell = Rc::clone(&self.nocolor_cell);
            let check = Checkbox::new(
                "No Color (Pass Through)",
                Arc::clone(&self.font),
                self.pass_through,
            )
            .with_font_size(self.font_size)
            .with_state_cell(nocolor_cell);
            self.children.push(Box::new(check));
            self.idx_nocolor = Some(3);
        } else {
            self.idx_nocolor = None;
        }
    }

    /// Reset HSV state to the colour observed at construction.
    pub(crate) fn revert_to_saved(&mut self) {
        let (h, s, v) = rgb_to_hsv(self.saved.r, self.saved.g, self.saved.b);
        self.h = h;
        self.s = s;
        self.v = v;
        self.a = self.saved.a;
        self.pass_through = self.allow_none && self.saved.a <= 0.0;
        self.nocolor_cell.set(self.pass_through);
    }

    /// Helper: invoke `on_change` with the current working colour
    /// (`None` if pass-through).
    pub(crate) fn fire_on_change(&mut self) {
        let snap = self.current_color();
        if let Some(cb) = self.on_change.as_mut() {
            cb(snap);
        }
    }
}
