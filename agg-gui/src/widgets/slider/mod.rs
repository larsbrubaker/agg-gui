//! `Slider` — a range slider with a draggable thumb.
//!
//! Supports linear and logarithmic value mapping (including ranges that span
//! zero and infinity), three clamping modes, integer/step snapping, "smart aim"
//! round-value snapping, an optional value suffix, horizontal or vertical
//! orientation, a toggleable trailing fill, and circle/rect handle shapes.
//!
//! The numeric core (value ↔ normalized-position mapping and smart aim) lives
//! in the sibling [`crate::widgets::slider_math`] module so it can be unit
//! tested in isolation and to keep this file within the project's size limit.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, MouseButton};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::label::{Label, LabelAlign};
use crate::widgets::slider_math::{
    best_in_range_f64, clamp_value_to_range, normalized_from_value, value_from_normalized,
    SliderSpec,
};

mod paint;

/// Orientation of a [`Slider`]. Horizontal is the default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SliderOrientation {
    Horizontal,
    Vertical,
}

/// When a [`Slider`] clamps values to its range. Mirrors egui's `SliderClamping`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SliderClamping {
    /// Never clamp — the value may sit outside the range (thumb pins to the edge).
    Never,
    /// Only clamp values produced by interacting with the slider; pre-existing
    /// out-of-range values are left intact.
    Edits,
    /// Always clamp, including a pre-existing out-of-range value.
    #[default]
    Always,
}

/// Shape of a [`Slider`]'s draggable handle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HandleShape {
    /// A circle (the default).
    Circle,
    /// A rectangle whose extent along the slider axis is `aspect_ratio` times
    /// its cross-axis extent.
    Rect { aspect_ratio: f64 },
}

impl Default for HandleShape {
    fn default() -> Self {
        Self::Circle
    }
}

/// Pixels of "aim radius" used when smart aim is on: the drag samples the value
/// a little to each side of the pointer and snaps to the roundest value between.
/// Deviation from egui: there this comes from `InputState::aim_radius()` (varies
/// per input device); we use a fixed mouse-like radius since agg-gui's event
/// layer doesn't carry per-device precision yet.
const AIM_RADIUS: f64 = 1.5;
/// Track length (px) of a vertical slider.
const VERT_LEN: f64 = 140.0;

const TRACK_H: f64 = 4.0;
const THUMB_R: f64 = 7.0;
/// Total widget height.  Needs to fit the thumb (diameter `2 * THUMB_R`)
/// plus a little breathing room for the focus ring — `22 px` keeps rows
/// compact in settings-style panels while still being easy to grab.
const WIDGET_H: f64 = 22.0;
/// Default pixel budget reserved on the right for the numeric value
/// label.  Wide enough for 4-5 glyphs at the slider's default font
/// size.  Set to `0.0` via [`Slider::with_show_value(false)`] to hide.
const VALUE_W: f64 = 44.0;
/// Gap between the track's right edge and the value label's left edge.
const VALUE_GAP: f64 = 6.0;

/// Inspector-visible properties of a [`Slider`].
///
/// **The "companion props" pattern:** widgets that opt into the reflection-
/// driven inspector hold a small `*Props` struct exposing only their
/// directly-editable values.  This sidesteps two structural problems with
/// deriving `Reflect` on the whole widget:
///   1. `bevy_reflect::Reflect` requires `Send + Sync`, which widgets violate
///      because they carry `Rc<Cell<…>>` and non-`Sync` callbacks.
///   2. Sub-widgets (`Label` here) and `Arc<Font>` would force a cascading
///      `Reflect` derive across types that don't have it.
///
/// The companion struct contains plain values (`f64`, `bool`, `Option<usize>`)
/// — `Send + Sync + Reflect`-friendly — and the widget routes all reads/writes
/// through it.  The inspector edits the companion live and the widget reacts.
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[derive(Clone, Debug)]
pub struct SliderProps {
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub show_value: bool,
    /// Fixed decimals for the value label — overrides the step-based
    /// auto-format when `Some`.
    pub decimals: Option<usize>,
    pub font_size: f64,
}

impl Default for SliderProps {
    fn default() -> Self {
        Self {
            value: 0.0,
            min: 0.0,
            max: 1.0,
            step: 0.01,
            show_value: true,
            decimals: None,
            font_size: 12.0,
        }
    }
}

/// A horizontal slider for a `f64` value within `[min, max]`.
pub struct Slider {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    base: WidgetBase,
    /// Reflectable, inspector-editable values — see [`SliderProps`].
    pub props: SliderProps,
    dragging: bool,
    focused: bool,
    hovered: bool,
    on_change: Option<Box<dyn FnMut(f64)>>,
    /// Optional external mirror of `value`.  When `Some`, `layout()` re-reads
    /// the cell every frame so a second widget that writes the same cell
    /// drives this slider live; `set_value` writes back.  Mirrors the
    /// `ToggleSwitch::with_state_cell` pattern — the cell is the source-of-
    /// truth so multiple widgets can reflect the same value bidirectionally.
    value_cell: Option<Rc<Cell<f64>>>,
    /// Backbuffered Label that renders the numeric value.  Updated in
    /// `layout()` with the current formatted value so the text follows
    /// drags live.
    value_label: Label,
    /// Tracks the string last pushed into `value_label` so we only
    /// invalidate its cache when the displayed value actually changes.
    last_value_text: String,

    // ── Extended configuration (egui-parity options) ──────────────────────
    /// Log/linear mapping spec. `logarithmic` off by default.
    spec: SliderSpec,
    clamping: SliderClamping,
    smart_aim: bool,
    orientation: SliderOrientation,
    handle_shape: HandleShape,
    /// When true, values are rounded to integers and formatted with no decimals.
    integer: bool,
    /// Text appended after the numeric value (e.g. "°" or " m").
    suffix: String,
    /// Whether to paint the accent-colored fill up to the handle. Defaults to
    /// `true` to preserve the widget's historical appearance across the app;
    /// egui's per-slider default is off, so demos set this explicitly.
    trailing_fill: bool,
}

impl Slider {
    pub fn new(value: f64, min: f64, max: f64, font: Arc<Font>) -> Self {
        // Not `f64::clamp` — that panics when min > max, and reversed
        // (high-to-low) ranges are supported by the mapping in `slider_math`.
        let v = clamp_value_to_range(value, min, max);
        let font_size = 12.0;
        let value_label = Label::new("", Arc::clone(&font))
            .with_font_size(font_size)
            .with_align(LabelAlign::Right);
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            props: SliderProps {
                value: v,
                min,
                max,
                step: (max - min) / 100.0,
                show_value: true,
                decimals: None,
                font_size,
            },
            dragging: false,
            focused: false,
            hovered: false,
            on_change: None,
            value_cell: None,
            value_label,
            last_value_text: String::new(),
            spec: SliderSpec::default(),
            clamping: SliderClamping::default(),
            smart_aim: true,
            orientation: SliderOrientation::Horizontal,
            handle_shape: HandleShape::Circle,
            integer: false,
            suffix: String::new(),
            trailing_fill: true,
        }
    }

    pub fn with_step(mut self, step: f64) -> Self {
        self.props.step = step;
        self
    }

    // ── Extended-configuration builders ────────────────────────────────────

    /// Enable/disable logarithmic value mapping. Great for spanning huge ranges
    /// (and, unlike naive log, tolerates ranges that include zero and infinity).
    pub fn with_logarithmic(mut self, logarithmic: bool) -> Self {
        self.spec.logarithmic = logarithmic;
        self
    }

    /// For logarithmic sliders including zero: the smallest positive value the
    /// user can select. Default `1e-6` (or `1` for integer sliders).
    pub fn with_smallest_positive(mut self, smallest_positive: f64) -> Self {
        self.spec.smallest_positive = smallest_positive;
        self
    }

    /// For logarithmic sliders whose high end is infinity: the largest finite
    /// value before the slider switches to `∞`. Default `∞`.
    pub fn with_largest_finite(mut self, largest_finite: f64) -> Self {
        self.spec.largest_finite = largest_finite;
        self
    }

    /// Choose when values are clamped to the range. Default [`SliderClamping::Always`].
    pub fn with_clamping(mut self, clamping: SliderClamping) -> Self {
        self.clamping = clamping;
        self
    }

    /// Turn smart aim (snap toward round values while dragging) on/off. Default on.
    pub fn with_smart_aim(mut self, smart_aim: bool) -> Self {
        self.smart_aim = smart_aim;
        self
    }

    /// Horizontal or vertical. Default horizontal.
    pub fn with_orientation(mut self, orientation: SliderOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Change the handle shape (circle or aspect-ratio'd rectangle).
    pub fn with_handle_shape(mut self, shape: HandleShape) -> Self {
        self.handle_shape = shape;
        self
    }

    /// Make this an integer slider: values round to whole numbers, formatting
    /// uses no decimals, and logarithmic mapping treats `1` as the smallest
    /// positive value.
    pub fn with_integer(mut self, integer: bool) -> Self {
        self.integer = integer;
        if integer {
            self.spec.smallest_positive = 1.0;
        }
        self
    }

    /// Append a suffix to the value label (e.g. "°" or " m").
    pub fn with_suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = suffix.into();
        self
    }

    /// Toggle the accent-colored trailing fill painted up to the handle.
    pub fn with_trailing_fill(mut self, trailing_fill: bool) -> Self {
        self.trailing_fill = trailing_fill;
        self
    }

    /// Bind this slider's value to an external `Rc<Cell<f64>>`.
    ///
    /// The cell becomes the source-of-truth: `layout()` reads it every
    /// frame so any other widget (or code path) that writes the cell
    /// will drive this slider live; drag interactions here write back
    /// to the cell too.  Pattern mirrors `ToggleSwitch::with_state_cell`.
    pub fn with_value_cell(mut self, cell: Rc<Cell<f64>>) -> Self {
        // Reversed-range/NaN tolerant — see the note in `new`.
        self.props.value = clamp_value_to_range(cell.get(), self.props.min, self.props.max);
        self.value_cell = Some(cell);
        self
    }
    pub fn with_show_value(mut self, show: bool) -> Self {
        self.props.show_value = show;
        self
    }

    /// Force a specific decimal count for the numeric value label.  When
    /// unset, the format falls back to a heuristic based on `step`.
    pub fn with_decimals(mut self, decimals: usize) -> Self {
        self.props.decimals = Some(decimals);
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
    pub fn with_min_size(mut self, s: Size) -> Self {
        self.base.min_size = s;
        self
    }
    pub fn with_max_size(mut self, s: Size) -> Self {
        self.base.max_size = s;
        self
    }

    pub fn on_change(mut self, cb: impl FnMut(f64) + 'static) -> Self {
        self.on_change = Some(Box::new(cb));
        self
    }

    pub fn value(&self) -> f64 {
        self.props.value
    }

    pub fn set_value(&mut self, v: f64) {
        self.props.value = self.commit(v);
        if let Some(cell) = &self.value_cell {
            cell.set(self.props.value);
        }
    }

    fn fire(&mut self) {
        let v = self.props.value;
        if let Some(cell) = &self.value_cell {
            cell.set(v);
        }
        if let Some(cb) = self.on_change.as_mut() {
            cb(v);
        }
    }

    fn is_vertical(&self) -> bool {
        matches!(self.orientation, SliderOrientation::Vertical)
    }

    /// Handle radius along the slider axis — the range is shrunk by this so the
    /// handle never overhangs the ends.
    fn handle_extent(&self) -> f64 {
        match self.handle_shape {
            HandleShape::Circle => THUMB_R,
            HandleShape::Rect { aspect_ratio } => THUMB_R * aspect_ratio,
        }
    }

    /// Pixel X of the track's right edge (horizontal).  The value label lives in
    /// a reserved strip to the right of this so a thumb at max doesn't overdraw
    /// the digits.
    fn track_right(&self) -> f64 {
        let reserved = if self.props.show_value {
            VALUE_W + VALUE_GAP
        } else {
            0.0
        };
        (self.bounds.width - reserved - THUMB_R).max(THUMB_R + 1.0)
    }

    /// The pixel positions (along the main axis) of normalized `0.0` and `1.0`.
    /// For vertical sliders the larger value maps to the top (smaller y).
    fn position_range(&self) -> (f64, f64) {
        let hr = self.handle_extent();
        if self.is_vertical() {
            let bottom = self.bounds.height - hr; // normalized 0
            let top = hr; // normalized 1
            (bottom, top)
        } else {
            (THUMB_R, self.track_right()) // normalized 0..1
        }
    }

    fn normalized(&self) -> f64 {
        normalized_from_value(self.props.value, self.props.min, self.props.max, &self.spec)
    }

    /// Center of the thumb along the main axis (px).
    fn thumb_pos(&self) -> f64 {
        let (p0, p1) = self.position_range();
        p0 + self.normalized().clamp(0.0, 1.0) * (p1 - p0)
    }

    fn value_from_pixel(&self, px: f64) -> f64 {
        let (p0, p1) = self.position_range();
        let n = if (p1 - p0).abs() < f64::EPSILON {
            0.0
        } else {
            ((px - p0) / (p1 - p0)).clamp(0.0, 1.0)
        };
        value_from_normalized(n, self.props.min, self.props.max, &self.spec)
    }

    /// Apply clamping, step snapping and integer rounding to a raw value.
    fn commit(&self, mut value: f64) -> f64 {
        if self.clamping != SliderClamping::Never {
            value = clamp_value_to_range(value, self.props.min, self.props.max);
        }
        if self.props.step > 0.0 {
            let start = self.props.min;
            value = start + ((value - start) / self.props.step).round() * self.props.step;
        }
        if self.integer {
            value = value.round();
        }
        value
    }

    /// Value the pointer maps to, with smart aim applied when enabled.
    fn pointer_value(&self, px: f64) -> f64 {
        let raw = if self.smart_aim {
            best_in_range_f64(
                self.value_from_pixel(px - AIM_RADIUS),
                self.value_from_pixel(px + AIM_RADIUS),
            )
        } else {
            self.value_from_pixel(px)
        };
        self.commit(raw)
    }

    /// Move the value one keyboard step in `dir` (+1 increases the value).
    fn nudge(&self, dir: f64) -> f64 {
        if self.props.step > 0.0 {
            return self.commit(self.props.value + dir * self.props.step);
        }
        let (p0, p1) = self.position_range();
        let cur = self.thumb_pos();
        let px = cur + dir * (p1 - p0).signum();
        let raw = if self.smart_aim {
            best_in_range_f64(
                self.value_from_pixel(px - 0.49),
                self.value_from_pixel(px + 0.49),
            )
        } else {
            self.value_from_pixel(px)
        };
        self.commit(raw)
    }

    /// Number of decimals to show when no explicit `decimals` override is set.
    fn auto_decimals(&self) -> usize {
        if self.integer {
            return 0;
        }
        if self.props.step >= 1.0 {
            0
        } else if self.props.step >= 0.1 {
            1
        } else if self.props.step >= 0.01 {
            2
        } else if self.props.step > 0.0 {
            3
        } else {
            // No step (e.g. logarithmic): pick precision from magnitude so a
            // value of 10000 shows no decimals and 0.001 shows several.
            let v = self.props.value.abs();
            if v == 0.0 || !v.is_finite() {
                2
            } else {
                (3 - v.log10().floor() as i32).clamp(0, 6) as usize
            }
        }
    }

    /// Format the slider's value (decimals + optional suffix).
    fn format_value(&self) -> String {
        let decimals = self.props.decimals.unwrap_or_else(|| self.auto_decimals());
        format!("{:.*}{}", decimals, self.props.value, self.suffix)
    }

}

impl Widget for Slider {
    fn type_name(&self) -> &'static str {
        "Slider"
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

    #[cfg(feature = "reflect")]
    fn as_reflect(&self) -> Option<&dyn bevy_reflect::Reflect> {
        Some(&self.props)
    }
    #[cfg(feature = "reflect")]
    fn as_reflect_mut(&mut self) -> Option<&mut dyn bevy_reflect::Reflect> {
        Some(&mut self.props)
    }

    fn is_focusable(&self) -> bool {
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

    fn layout(&mut self, available: Size) -> Size {
        // Re-read external cell every frame — another widget (e.g. the
        // System window's slider) may have written a new value.  Skip
        // while dragging so the user's in-flight drag isn't fought
        // back by rounding inside the source cell.
        if !self.dragging {
            if let Some(cell) = &self.value_cell {
                let raw = cell.get();
                self.props.value = if self.clamping == SliderClamping::Always {
                    clamp_value_to_range(raw, self.props.min, self.props.max)
                } else {
                    raw
                };
            }
        }

        // Refresh the value-label text only when the displayed string
        // actually changed — Label's `set_text` invalidates its cache
        // so we want to skip this when the value is unchanged (e.g.
        // idle frames between drags).
        if self.props.show_value {
            let new_text = self.format_value();
            if new_text != self.last_value_text {
                self.value_label.set_text(new_text.clone());
                self.last_value_text = new_text;
            }
            // Size the label to exactly the reserved strip; right-align
            // anchors the digits to the widget's right edge.
            let lh = self.props.font_size * 1.5;
            let _ = self.value_label.layout(Size::new(VALUE_W, lh));
            self.value_label
                .set_bounds(Rect::new(0.0, 0.0, VALUE_W, lh));
        }

        if self.is_vertical() {
            Size::new(available.width, VERT_LEN)
        } else {
            Size::new(available.width, WIDGET_H)
        }
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if self.is_vertical() {
            self.paint_vertical(ctx);
        } else {
            self.paint_horizontal(ctx);
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let was = self.hovered;
                self.hovered = self.hit_test(*pos);
                if self.dragging {
                    let px = if self.is_vertical() { pos.y } else { pos.x };
                    self.props.value = self.pointer_value(px);
                    self.fire();
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
                ..
            } => {
                self.dragging = true;
                let px = if self.is_vertical() { pos.y } else { pos.x };
                self.props.value = self.pointer_value(px);
                self.fire();
                crate::animation::request_draw();
                EventResult::Consumed
            }
            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                let was = self.dragging;
                self.dragging = false;
                if was {
                    crate::animation::request_draw();
                }
                EventResult::Consumed
            }
            Event::KeyDown { key, .. } => {
                // Arrows move the value along the slider's own axis: left/right
                // for horizontal, up/down for vertical (up = increase).
                let dir = match (key, self.is_vertical()) {
                    (Key::ArrowLeft, false) => -1.0,
                    (Key::ArrowRight, false) => 1.0,
                    (Key::ArrowDown, true) => -1.0,
                    (Key::ArrowUp, true) => 1.0,
                    _ => 0.0,
                };
                if dir != 0.0 {
                    self.props.value = self.nudge(dir);
                    self.fire();
                    crate::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::FocusGained => {
                let was = self.focused;
                self.focused = true;
                if !was {
                    crate::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::FocusLost => {
                let was_focused = self.focused;
                let was_dragging = self.dragging;
                self.focused = false;
                self.dragging = false;
                if was_focused || was_dragging {
                    crate::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

    fn test_font() -> Arc<Font> {
        Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
    }

    /// `slider_math` deliberately supports reversed (high-to-low) ranges, so
    /// constructing a slider with one must not panic — `f64::clamp` does when
    /// min > max.
    #[test]
    fn new_with_reversed_range_does_not_panic() {
        let s = Slider::new(5.0, 10.0, 0.0, test_font());
        assert_eq!(s.value(), 5.0);
        // Out-of-range values clamp to the nearer end of the ordered range.
        let s = Slider::new(-1.0, 10.0, 0.0, test_font());
        assert_eq!(s.value(), 0.0);
    }

    /// Same for the external-cell binding, which clamps on construction and on
    /// every layout read.
    #[test]
    fn value_cell_with_reversed_range_does_not_panic() {
        let cell = Rc::new(Cell::new(42.0));
        let mut s = Slider::new(5.0, 10.0, 0.0, test_font()).with_value_cell(Rc::clone(&cell));
        assert_eq!(s.value(), 10.0);
        cell.set(-3.0);
        let _ = s.layout(Size::new(200.0, 22.0));
        assert_eq!(s.value(), 0.0);
    }
}
