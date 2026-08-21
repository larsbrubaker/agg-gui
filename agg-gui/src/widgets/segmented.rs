//! `SegmentedControl` — a macOS-style segmented picker: one joined track,
//! several labelled segments, exactly one selected.
//!
//! The SwiftUI `Picker(...).pickerStyle(.segmented)` / AppKit
//! `NSSegmentedControl` look: a single rounded track whose OUTER corners
//! are rounded, segments butted together with no gap and 1-px hairline
//! dividers between them, and the selected segment filled with the
//! theme's accent surface. Compared with composing `Button`s in a
//! `FlexRow` (the `with_subtle` + `with_active_fn` recipe), this widget
//! owns the whole strip, so the corners, dividers and keyboard model
//! (Left/Right move the selection) are right by construction.
//!
//! Selection is bound to an `Rc<Cell<usize>>` the caller owns — the same
//! bidirectional-cell pattern as `RadioGroup::with_selected_cell` and
//! `ToggleSwitch::with_state_cell`: the cell is read every layout / paint
//! and written whenever the user picks a segment, so external writes show
//! up on the next frame without rebuilding the widget.
//!
//! Each segment label is a real [`Label`] child (like `RadioGroup`) so
//! glyph rasterization is cached and the inspector tree mirrors the
//! visible structure; this widget's `paint()` only draws the chrome and
//! retints the labels.
//!
//! Unit tests live in `segmented_tests.rs` (pulled in as a child module)
//! to keep this file under the 800-line cap.

#[cfg(test)]
#[path = "segmented_tests.rs"]
mod tests;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::theme::Visuals;
use crate::widget::Widget;
use crate::widgets::label::{Label, LabelAlign};

/// Control height in the regular size.
const REGULAR_H: f64 = 24.0;
/// Control height in the compact size (`with_compact`).
const COMPACT_H: f64 = 20.0;
/// Default label font size, regular / compact.
const REGULAR_FONT: f64 = 13.0;
const COMPACT_FONT: f64 = 11.0;
/// Horizontal padding inside a segment on each side of its label.
const REGULAR_PAD: f64 = 12.0;
const COMPACT_PAD: f64 = 8.0;
/// Outer corner radius of the track.
const REGULAR_R: f64 = 6.0;
const COMPACT_R: f64 = 5.0;
/// Gap between the track edge and the selected / hovered segment fill,
/// so the filled segment reads as a raised pill inside the track.
const FILL_INSET: f64 = 1.0;
/// Vertical inset of the hairline dividers from the track edges.
const DIVIDER_INSET: f64 = 4.0;
/// Bezier circle approximation constant for the per-corner rounded path.
const KAPPA: f64 = 0.552_284_75;

/// A macOS-style segmented control. See the module docs.
pub struct SegmentedControl {
    bounds: Rect,
    /// One `Label` child per segment, in order.
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    labels: Vec<String>,
    /// Shared selection cell — the source of truth for the selected index.
    selected: Rc<Cell<usize>>,
    font: Arc<Font>,
    font_size: f64,
    compact: bool,
    /// `true`: each segment takes its intrinsic label width. `false`
    /// (default): all segments share the width of the widest label, the
    /// macOS default.
    fit_width: bool,
    on_change: Option<Box<dyn FnMut(usize)>>,
    /// Live enabled gate for the whole control (`None` = enabled).
    enabled_fn: Option<Rc<dyn Fn() -> bool>>,
    /// Live per-segment enabled gate, by index (`None` = all enabled).
    segment_enabled_fn: Option<Rc<dyn Fn(usize) -> bool>>,
    /// Segment rects in widget-local coordinates, computed at layout.
    segments: Vec<Rect>,
    hovered: Option<usize>,
    pressed: Option<usize>,
    focused: bool,
}

impl SegmentedControl {
    /// Create a control with one segment per label, bound to `selected`.
    /// `font` renders the labels (the crate has no default typeface —
    /// same as `Button::new` / `RadioGroup::new`).
    pub fn new(labels: Vec<impl Into<String>>, selected: Rc<Cell<usize>>, font: Arc<Font>) -> Self {
        let labels: Vec<String> = labels.into_iter().map(Into::into).collect();
        let font_size = REGULAR_FONT;
        let children = Self::build_labels(&labels, &font, font_size);
        let ctl = Self {
            bounds: Rect::default(),
            children,
            base: WidgetBase::new(),
            labels,
            selected,
            font,
            font_size,
            compact: false,
            fit_width: false,
            on_change: None,
            enabled_fn: None,
            segment_enabled_fn: None,
            segments: Vec::new(),
            hovered: None,
            pressed: None,
            focused: false,
        };
        ctl.clamp_selection();
        ctl
    }

    fn build_labels(labels: &[String], font: &Arc<Font>, font_size: f64) -> Vec<Box<dyn Widget>> {
        labels
            .iter()
            .map(|text| {
                Box::new(
                    Label::new(text.as_str(), Arc::clone(font))
                        .with_font_size(font_size)
                        .with_align(LabelAlign::Center),
                ) as Box<dyn Widget>
            })
            .collect()
    }

    /// Label font size (default 13, or 11 in compact mode).
    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self.children = Self::build_labels(&self.labels, &self.font, size);
        self
    }

    /// Compact size: shorter track, smaller type, tighter padding
    /// (SwiftUI `.controlSize(.small)`). Keeps an explicit
    /// `with_font_size` if one was set before this call.
    pub fn with_compact(mut self) -> Self {
        self.compact = true;
        if (self.font_size - REGULAR_FONT).abs() < f64::EPSILON {
            self.font_size = COMPACT_FONT;
            self.children = Self::build_labels(&self.labels, &self.font, COMPACT_FONT);
        }
        self
    }

    /// Size each segment to its own label instead of giving every segment
    /// the width of the widest one.
    pub fn with_fit_width(mut self, fit: bool) -> Self {
        self.fit_width = fit;
        self
    }

    /// Called with the new index whenever the user changes the selection
    /// (pointer or keyboard). Not called for external cell writes.
    pub fn on_change(mut self, cb: impl FnMut(usize) + 'static) -> Self {
        self.on_change = Some(Box::new(cb));
        self
    }

    /// Gate the whole control on a live predicate: while it returns
    /// `false` the control paints disabled and ignores input.
    pub fn with_enabled_fn(mut self, f: impl Fn() -> bool + 'static) -> Self {
        self.enabled_fn = Some(Rc::new(f));
        self
    }

    /// Gate individual segments on a live predicate over the segment
    /// index. Disabled segments paint dimmed, don't hover, can't be
    /// clicked, and are skipped by Left/Right.
    pub fn with_segment_enabled_fn(mut self, f: impl Fn(usize) -> bool + 'static) -> Self {
        self.segment_enabled_fn = Some(Rc::new(f));
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

    /// Currently selected index (clamped to the label count).
    pub fn selected(&self) -> usize {
        let n = self.labels.len();
        if n == 0 {
            0
        } else {
            self.selected.get().min(n - 1)
        }
    }

    /// Programmatic selection: writes the cell, does NOT fire `on_change`.
    pub fn set_selected(&mut self, idx: usize) {
        if idx < self.labels.len() {
            self.selected.set(idx);
            crate::animation::request_draw();
        }
    }

    /// Shared selection cell.
    pub fn selected_cell(&self) -> Rc<Cell<usize>> {
        Rc::clone(&self.selected)
    }

    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    /// Clamp an out-of-range cell value back into the label range.
    fn clamp_selection(&self) {
        let n = self.labels.len();
        if n > 0 && self.selected.get() >= n {
            self.selected.set(n - 1);
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled_fn.as_ref().map(|f| f()).unwrap_or(true)
    }

    fn segment_enabled(&self, i: usize) -> bool {
        self.is_enabled()
            && self
                .segment_enabled_fn
                .as_ref()
                .map(|f| f(i))
                .unwrap_or(true)
    }

    fn height(&self) -> f64 {
        let base = if self.compact { COMPACT_H } else { REGULAR_H };
        base.max(self.font_size * 1.6)
            .max(self.base.min_size.height)
    }

    fn corner_radius(&self) -> f64 {
        if self.compact {
            COMPACT_R
        } else {
            REGULAR_R
        }
    }

    fn pad_h(&self) -> f64 {
        if self.compact {
            COMPACT_PAD
        } else {
            REGULAR_PAD
        }
    }

    /// Index of the segment containing `pos`, if any.
    fn segment_at(&self, pos: Point) -> Option<usize> {
        self.segments.iter().position(|r| {
            pos.x >= r.x && pos.x < r.x + r.width && pos.y >= r.y && pos.y < r.y + r.height
        })
    }

    /// Select `idx` as a user action: writes the cell and fires
    /// `on_change` when the value actually changed.
    fn choose(&mut self, idx: usize) {
        if idx >= self.labels.len() || !self.segment_enabled(idx) {
            return;
        }
        let was = self.selected();
        self.selected.set(idx);
        if was != idx {
            if let Some(cb) = self.on_change.as_mut() {
                cb(idx);
            }
        }
        crate::animation::request_draw();
    }

    /// Next enabled segment from `from` in direction `dir` (+1 / -1).
    fn step_selection(&self, from: usize, dir: i64) -> Option<usize> {
        let n = self.labels.len() as i64;
        let mut i = from as i64 + dir;
        while i >= 0 && i < n {
            if self.segment_enabled(i as usize) {
                return Some(i as usize);
            }
            i += dir;
        }
        None
    }

    /// Compute segment widths for the given available width. Pure w.r.t.
    /// widget state except for the label children's measured widths.
    fn segment_widths(&mut self, available: Size) -> Vec<f64> {
        let n = self.labels.len();
        if n == 0 {
            return Vec::new();
        }
        let h = self.height();
        let pad = self.pad_h();
        let natural: Vec<f64> = self
            .children
            .iter_mut()
            .map(|c| c.layout(Size::new(available.width.max(0.0), h)).width + 2.0 * pad)
            .collect();
        let mut widths = if self.fit_width {
            natural
        } else {
            let w = natural.iter().cloned().fold(0.0, f64::max);
            vec![w; n]
        };
        let total: f64 = widths.iter().sum();
        let target = if self.base.h_anchor.is_stretch() {
            available.width.max(self.base.min_size.width)
        } else {
            total.max(self.base.min_size.width)
        };
        let target = if available.width > 0.0 {
            target.min(available.width)
        } else {
            target
        };
        if total > 0.0 && (total - target).abs() > 1e-6 {
            if self.fit_width {
                // Share the surplus (or deficit) proportionally so the
                // relative label room is preserved.
                let k = target / total;
                for w in widths.iter_mut() {
                    *w *= k;
                }
            } else {
                let each = target / n as f64;
                for w in widths.iter_mut() {
                    *w = each;
                }
            }
        }
        widths
    }

    /// Append a rectangle with independent left / right corner radii to
    /// the current path. Built from lines and cubic arcs so it renders
    /// identically on every `DrawCtx` backend.
    fn path_rect_corners(ctx: &mut dyn DrawCtx, r: Rect, rl: f64, rr: f64) {
        let rl = rl.min(r.width * 0.5).min(r.height * 0.5).max(0.0);
        let rr = rr.min(r.width * 0.5).min(r.height * 0.5).max(0.0);
        let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.width, r.y + r.height);
        let kl = rl * KAPPA;
        let kr = rr * KAPPA;
        ctx.move_to(x0 + rl, y0);
        ctx.line_to(x1 - rr, y0);
        if rr > 0.0 {
            ctx.cubic_to(x1 - rr + kr, y0, x1, y0 + rr - kr, x1, y0 + rr);
        }
        ctx.line_to(x1, y1 - rr);
        if rr > 0.0 {
            ctx.cubic_to(x1, y1 - rr + kr, x1 - rr + kr, y1, x1 - rr, y1);
        }
        ctx.line_to(x0 + rl, y1);
        if rl > 0.0 {
            ctx.cubic_to(x0 + rl - kl, y1, x0, y1 - rl + kl, x0, y1 - rl);
        }
        ctx.line_to(x0, y0 + rl);
        if rl > 0.0 {
            ctx.cubic_to(x0, y0 + rl - kl, x0 + rl - kl, y0, x0 + rl, y0);
        }
        ctx.close_path();
    }

    /// Fill segment `i`'s rect (inset by [`FILL_INSET`]) with `color`,
    /// rounding only the corners that coincide with the track's outer
    /// corners.
    fn fill_segment(&self, ctx: &mut dyn DrawCtx, i: usize, color: Color) {
        let Some(seg) = self.segments.get(i) else {
            return;
        };
        let n = self.segments.len();
        let outer_r = (self.corner_radius() - FILL_INSET).max(0.0);
        let rl = if i == 0 { outer_r } else { 0.0 };
        let rr = if i + 1 == n { outer_r } else { 0.0 };
        let inset = Rect::new(
            seg.x + if i == 0 { FILL_INSET } else { 0.0 },
            seg.y + FILL_INSET,
            (seg.width
                - if i == 0 { FILL_INSET } else { 0.0 }
                - if i + 1 == n { FILL_INSET } else { 0.0 })
            .max(0.0),
            (seg.height - 2.0 * FILL_INSET).max(0.0),
        );
        ctx.set_fill_color(color);
        ctx.begin_path();
        Self::path_rect_corners(ctx, inset, rl, rr);
        ctx.fill();
    }

    /// Foreground / chrome colours for the disabled control, mirroring
    /// `Button`'s disabled palette so mixed rows of controls grey out alike.
    fn disabled_colors(v: &Visuals) -> (Color, Color, Color) {
        if v.is_dark() {
            (
                v.window_fill,
                Color::rgba(1.0, 1.0, 1.0, 0.22),
                v.text_dim.with_alpha(0.42),
            )
        } else {
            (v.track_bg, v.widget_stroke.with_alpha(0.45), v.text_dim)
        }
    }

    /// Ink colour a segment label should use for the current state.
    fn label_color(&self, v: &Visuals, i: usize, selected: usize, enabled: bool) -> Color {
        if !enabled || !self.segment_enabled(i) {
            Self::disabled_colors(v).2
        } else if i == selected {
            // Light ink on the accent fill, like `ButtonTheme::default`.
            Color::white()
        } else {
            v.text_color
        }
    }

    fn handle_key(&mut self, key: &Key) -> EventResult {
        let n = self.labels.len();
        if n == 0 {
            return EventResult::Ignored;
        }
        let cur = self.selected();
        let next = match key {
            Key::ArrowLeft | Key::ArrowUp => self.step_selection(cur, -1),
            Key::ArrowRight | Key::ArrowDown => self.step_selection(cur, 1),
            Key::Home => (0..n).find(|&i| self.segment_enabled(i)),
            Key::End => (0..n).rev().find(|&i| self.segment_enabled(i)),
            _ => return EventResult::Ignored,
        };
        match next {
            Some(i) if i != cur => {
                self.choose(i);
                EventResult::Consumed
            }
            // A consumed no-op keeps Left at the first segment from
            // scrolling an enclosing ScrollView, matching RadioGroup.
            Some(_) => EventResult::Consumed,
            None => EventResult::Ignored,
        }
    }
}

impl Widget for SegmentedControl {
    fn type_name(&self) -> &'static str {
        "SegmentedControl"
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
        self.is_enabled() && !self.labels.is_empty()
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

    fn measure_min_height(&self, _available_w: f64) -> f64 {
        self.height()
    }

    fn layout(&mut self, available: Size) -> Size {
        self.clamp_selection();
        let h = self.height();
        let widths = self.segment_widths(available);
        let mut x = 0.0;
        self.segments.clear();
        for (i, w) in widths.iter().enumerate() {
            let seg = Rect::new(x, 0.0, *w, h);
            self.segments.push(seg);
            if let Some(child) = self.children.get_mut(i) {
                let s = child.layout(Size::new(*w, h));
                let lw = s.width.min(*w);
                child.set_bounds(Rect::new(
                    x + ((*w - lw) * 0.5).max(0.0),
                    ((h - s.height) * 0.5).max(0.0),
                    lw,
                    s.height,
                ));
            }
            x += w;
        }
        let size = Size::new(x, h);
        self.bounds = Rect::new(self.bounds.x, self.bounds.y, size.width, size.height);
        size
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;
        let r = self.corner_radius();
        let enabled = self.is_enabled();
        let selected = self.selected();
        let n = self.segments.len();
        let (disabled_bg, disabled_stroke, _) = Self::disabled_colors(&v);

        // Focus ring just inside the bounds (see Button for why inside).
        if enabled && self.focused {
            ctx.set_stroke_color(v.accent_focus);
            ctx.set_line_width(1.5);
            ctx.begin_path();
            ctx.rounded_rect(0.75, 0.75, (w - 1.5).max(0.0), (h - 1.5).max(0.0), r);
            ctx.stroke();
        }

        // Track.
        ctx.set_fill_color(if enabled { v.widget_bg } else { disabled_bg });
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, r);
        ctx.fill();

        if n > 0 {
            // Hovered (unselected, enabled) segment: subtle raised fill.
            if enabled {
                if let Some(hi) = self.hovered {
                    if hi != selected && self.segment_enabled(hi) {
                        self.fill_segment(ctx, hi, v.widget_bg_hovered);
                    }
                }
            }
            // Selected segment: accent fill (pressed / hovered shades).
            if enabled {
                let fill = if self.pressed == Some(selected) {
                    v.accent_pressed
                } else if self.hovered == Some(selected) {
                    v.accent_hovered
                } else {
                    v.accent
                };
                self.fill_segment(ctx, selected, fill);
            } else {
                self.fill_segment(ctx, selected, v.widget_bg_hovered);
            }
            // Hairline dividers between segments, skipped next to the
            // selected segment so its fill reads as one raised pill.
            ctx.set_stroke_color(if enabled {
                v.widget_stroke
            } else {
                disabled_stroke
            });
            ctx.set_line_width(1.0);
            for i in 1..n {
                if i == selected || i - 1 == selected {
                    continue;
                }
                let x = self.segments[i].x.round() + 0.5;
                ctx.begin_path();
                ctx.move_to(x, DIVIDER_INSET);
                ctx.line_to(x, h - DIVIDER_INSET);
                ctx.stroke();
            }
        }

        // Track outline.
        ctx.set_stroke_color(if enabled {
            v.widget_stroke
        } else {
            disabled_stroke
        });
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.5, 0.5, (w - 1.0).max(0.0), (h - 1.0).max(0.0), r);
        ctx.stroke();

        // Retint the label children for the current state; the framework
        // paints them after this returns.
        for i in 0..n {
            let color = self.label_color(&v, i, selected, enabled);
            if let Some(child) = self.children.get_mut(i) {
                child.set_label_color(color);
            }
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if !self.is_enabled() {
            // Drop transient state so the control looks idle the moment
            // it is disabled mid-interaction.
            if self.hovered.is_some() || self.pressed.is_some() {
                self.hovered = None;
                self.pressed = None;
                crate::animation::request_draw();
            }
            return EventResult::Ignored;
        }
        match event {
            Event::MouseMove { pos } => {
                let was = self.hovered;
                self.hovered = self.segment_at(*pos).filter(|&i| self.segment_enabled(i));
                if self.hovered.is_none() {
                    self.pressed = None;
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
            } => match self.segment_at(*pos) {
                Some(i) if self.segment_enabled(i) => {
                    self.pressed = Some(i);
                    crate::animation::request_draw();
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            },
            Event::MouseUp {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                let was = self.pressed.take();
                match was {
                    Some(i) => {
                        // Like Button: fire only when the release lands on
                        // the pressed segment (touch never sets hover).
                        if self.segment_at(*pos) == Some(i) {
                            self.choose(i);
                        }
                        crate::animation::request_draw();
                        EventResult::Consumed
                    }
                    None => EventResult::Ignored,
                }
            }
            Event::KeyDown { key, .. } => self.handle_key(key),
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
                let was = self.focused || self.pressed.is_some();
                self.focused = false;
                self.pressed = None;
                if was {
                    crate::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("segments", self.labels.join(" | ")),
            ("selected", self.selected().to_string()),
            ("compact", self.compact.to_string()),
            ("fit_width", self.fit_width.to_string()),
        ]
    }
}
