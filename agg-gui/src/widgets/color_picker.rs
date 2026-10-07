//! `ColorPicker` — an inline-expanding colour selection widget.
//!
//! Click the swatch to open a panel with a hue slider, a saturation/value
//! rectangle, an alpha slider, a hex readout, an optional "No Color (Pass
//! Through)" checkbox, and Cancel / Select buttons.  Bound to an
//! `Rc<Cell<Color>>` so callers observe changes through the standard shared
//! state pattern.
//!
//! Layout mirrors `ComboBox`: when closed the widget reports a compact height;
//! when open it returns the full expanded height so sibling widgets are pushed
//! down (works naturally inside a `ScrollView` or a `Window::with_auto_size`).
//!
//! [`ColorPicker::with_round_popup_swatch`] switches to a compact mode like
//! agg-sharp's `ItemColorButton`: the widget is a round swatch of a given
//! diameter, and the panel opens as a floating popup below (or above) it,
//! painted and hit-tested in the global overlay pass instead of growing the
//! widget, kept inside the viewport.  That mode lives in
//! `color_picker/popup.rs`.
//!
//! Edits preview live into the bound cell; [`ColorPicker::on_change`] also
//! reports them live (agg-sharp's colour popup applies as the user drags),
//! with Select committing through `on_select` and Cancel / Escape restoring.
//!
//! # Composition
//!
//! ```text
//! ColorPicker (swatch + custom gradients)
//!   ├── Checkbox   (No Color)
//!   ├── Button     (Cancel)
//!   └── Button     (Select)
//! ```
//!
//! Gradients (hue/SV/alpha) are painted directly as stacks of thin coloured
//! slices — agg-gui has no gradient primitive, but 1-px slices at this scale
//! are cheap and banding-free.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::button::Button;
use crate::widgets::checkbox::Checkbox;

// ── Layout constants ─────────────────────────────────────────────────────────

const SWATCH_H: f64 = 22.0;
const SWATCH_MIN_W: f64 = 48.0;

const PANEL_W: f64 = 228.0;
const PAD: f64 = 8.0;
const ROW_GAP: f64 = 6.0;

const HUE_H: f64 = 16.0;
const SV_H: f64 = 140.0;
const ALPHA_H: f64 = 16.0;
const HEX_H: f64 = 20.0;
const CHECK_H: f64 = 20.0;
const BTN_H: f64 = 26.0;

/// Height of the expanded panel below the swatch (does NOT include the swatch).
fn panel_body_h(allow_none: bool) -> f64 {
    let mut h = PAD;
    h += HUE_H + ROW_GAP;
    h += SV_H + ROW_GAP;
    h += ALPHA_H + ROW_GAP;
    h += HEX_H + ROW_GAP;
    if allow_none {
        h += CHECK_H + ROW_GAP;
    }
    h += BTN_H + PAD;
    h
}

// ── HSV / RGB helpers ────────────────────────────────────────────────────────

fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let v = max;
    let s = if max <= 0.0 { 0.0 } else { d / max };
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * (((b - r) / d) + 2.0)
    } else {
        60.0 * (((r - g) / d) + 4.0)
    };
    let h = if h < 0.0 { h + 360.0 } else { h };
    (h / 360.0, s, v)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let h6 = (h * 6.0) % 6.0;
    let c = v * s;
    let x = c * (1.0 - (h6 % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match h6 as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    (r1 + m, g1 + m, b1 + m)
}

fn format_hex(c: Color) -> String {
    let r = (c.r * 255.0).clamp(0.0, 255.0) as u32;
    let g = (c.g * 255.0).clamp(0.0, 255.0) as u32;
    let b = (c.b * 255.0).clamp(0.0, 255.0) as u32;
    let a = (c.a * 255.0).clamp(0.0, 255.0) as u32;
    format!("#{:02X}{:02X}{:02X}{:02X}", r, g, b, a)
}

// ── Drag mode ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    None,
    Hue,
    Sv,
    Alpha,
}

// ── Widget ───────────────────────────────────────────────────────────────────

/// Select callback shared between [`ColorPicker`] and its Select button.
type SharedSelectCallback = Rc<RefCell<Option<Box<dyn FnMut(Color)>>>>;

/// Inline colour picker bound to a shared `Color` cell.
pub struct ColorPicker {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // [no_color_check?, cancel, select]
    base: WidgetBase,

    font: Arc<Font>,
    font_size: f64,

    /// Authoritative colour the caller observes.  Only written on Select (or
    /// when "No Color" toggles, depending on wiring).
    color_cell: Rc<Cell<Color>>,

    /// Snapshot taken when the picker was opened — restored on Cancel.
    /// Shared with the Cancel button's click callback.
    saved: Rc<Cell<Color>>,
    /// The working HSVA colour (ignoring "No Color"), kept current after
    /// every `on_event` so the Select button's click callback can commit it
    /// without going through `ColorPicker::on_event`.
    working: Rc<Cell<Color>>,

    /// Working state while the panel is open.
    open: bool,
    h: f32,
    s: f32,
    v: f32,
    a: f32,
    /// True when "No Color (Pass Through)" is checked — working state; applied
    /// to the cell on Select as `Color::transparent()`.
    no_color: bool,
    allow_none: bool,

    /// None means not currently dragging anything.
    drag: Drag,

    /// Last local mouse position — fed into child widget layout for hit tests.
    hovered: bool,

    /// Optional callback invoked on Select with the final colour.  Shared
    /// with the Select button's click callback, which commits directly.
    on_select: SharedSelectCallback,

    // ── Sub-widget indices into `children` ───────────────────────────────────
    /// Set during `build_children` so paint/layout can find them quickly.
    idx_cancel: usize,
    idx_select: usize,
    idx_none: Option<usize>,

    /// Shared "no color" checkbox state.  Owned by `ColorPicker` so `on_event`
    /// can react to changes without going through a callback chain.
    none_cell: Rc<Cell<bool>>,
    /// Shared flags the sub-buttons flip after committing / restoring the
    /// colour cell themselves; read + cleared by `on_event` / `layout` to
    /// close the panel and resync the working HSVA state.
    cancel_flag: Rc<Cell<bool>>,
    select_flag: Rc<Cell<bool>>,

    /// Round-swatch popup mode: the swatch diameter.  `None` (the default)
    /// is the inline-expanding picker.  See `color_picker/popup.rs`.
    popup_swatch: Option<f64>,
    /// Outline drawn around the swatch: colour and width.  `None` uses
    /// `Visuals::widget_stroke` at 1 px.
    swatch_outline: Option<(Color, f64)>,
    /// Popup mode: whether the panel opens above the swatch (decided from
    /// the room around it when the popup opens and each time it paints).
    popup_opens_up: bool,
    /// Popup mode: the panel's x offset from the swatch's left edge, shifted
    /// left when the panel would run past the viewport's right edge.
    popup_dx: f64,
    /// Optional live-apply callback: fired with the working colour on every
    /// edit while the panel is open, and with the restored colour on Cancel
    /// / Escape.  Shared with the Cancel button's click callback.
    on_change: SharedSelectCallback,
    /// Whether `on_change` reported an edit since the panel opened, so
    /// Cancel knows to report the restored colour.
    live_dirty: Rc<Cell<bool>>,
    /// Round swatch fill for a fully transparent colour; `None` paints the
    /// transparent colour itself (only the outline shows).
    transparent_swatch_fill: Option<Color>,
}

impl ColorPicker {
    pub fn new(color_cell: Rc<Cell<Color>>, font: Arc<Font>) -> Self {
        let initial = color_cell.get();
        let (h, s, v) = rgb_to_hsv(initial.r, initial.g, initial.b);
        let none_cell = Rc::new(Cell::new(false));
        let cancel_flag = Rc::new(Cell::new(false));
        let select_flag = Rc::new(Cell::new(false));

        let mut me = Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            font: Arc::clone(&font),
            font_size: 13.0,
            color_cell,
            saved: Rc::new(Cell::new(initial)),
            working: Rc::new(Cell::new(initial)),
            open: false,
            h,
            s,
            v,
            a: initial.a,
            no_color: initial.a <= 0.0,
            allow_none: false,
            drag: Drag::None,
            hovered: false,
            on_select: Rc::new(RefCell::new(None)),
            idx_cancel: 0,
            idx_select: 1,
            idx_none: None,
            none_cell,
            cancel_flag,
            select_flag,
            popup_swatch: None,
            swatch_outline: None,
            popup_opens_up: false,
            popup_dx: 0.0,
            on_change: Rc::new(RefCell::new(None)),
            live_dirty: Rc::new(Cell::new(false)),
            transparent_swatch_fill: None,
        };
        me.build_children();
        me
    }

    pub fn with_font_size(mut self, s: f64) -> Self {
        self.font_size = s;
        self
    }
    pub fn with_allow_none(mut self, allow: bool) -> Self {
        self.allow_none = allow;
        self.build_children();
        self
    }
    pub fn on_select(self, cb: impl FnMut(Color) + 'static) -> Self {
        *self.on_select.borrow_mut() = Some(Box::new(cb));
        self
    }

    /// Live apply, like agg-sharp's colour popup: `cb` receives the working
    /// colour on every edit while the panel is open (a click or drag on the
    /// hue, saturation/value or alpha area, or toggling No Color), and the
    /// colour the panel opened with when Cancel or Escape restores it.
    /// Select (and closing the popup by clicking elsewhere) still commits
    /// through [`on_select`](Self::on_select).  Without it nothing fires
    /// before Select.
    pub fn on_change(self, cb: impl FnMut(Color) + 'static) -> Self {
        *self.on_change.borrow_mut() = Some(Box::new(cb));
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

    fn build_children(&mut self) {
        self.children.clear();

        let cf = Rc::clone(&self.cancel_flag);
        let sf = Rc::clone(&self.select_flag);

        // The buttons commit / restore the bound colour cell from their own
        // click callbacks: the framework may route a click straight to the
        // child Button (it is in `children()`), bypassing
        // `ColorPicker::on_event`, so deferring the commit to the picker's
        // next event would leave Select doing nothing until a later pointer
        // event.  The flags only tell the picker to close + resync itself.
        let cancel = {
            let cell = Rc::clone(&self.color_cell);
            let saved = Rc::clone(&self.saved);
            let on_change = Rc::clone(&self.on_change);
            let dirty = Rc::clone(&self.live_dirty);
            Button::new("Cancel", Arc::clone(&self.font)).on_click(move || {
                cell.set(saved.get());
                report_restore(&on_change, &dirty, saved.get());
                cf.set(true);
                crate::animation::request_draw();
            })
        };
        let select = {
            let cell = Rc::clone(&self.color_cell);
            let working = Rc::clone(&self.working);
            let none = Rc::clone(&self.none_cell);
            let on_select = Rc::clone(&self.on_select);
            Button::new("Select", Arc::clone(&self.font)).on_click(move || {
                let c = if none.get() {
                    Color::transparent()
                } else {
                    working.get()
                };
                cell.set(c);
                if let Some(cb) = on_select.borrow_mut().as_mut() {
                    cb(c);
                }
                sf.set(true);
                crate::animation::request_draw();
            })
        };

        if self.allow_none {
            let none_check = Checkbox::new(
                "No Color (Pass Through)",
                Arc::clone(&self.font),
                self.no_color,
            )
            .with_font_size(self.font_size)
            .with_state_cell(Rc::clone(&self.none_cell));
            self.children.push(Box::new(none_check));
            self.idx_none = Some(0);
            self.idx_cancel = 1;
            self.idx_select = 2;
        } else {
            self.idx_none = None;
            self.idx_cancel = 0;
            self.idx_select = 1;
        }
        self.children.push(Box::new(cancel));
        self.children.push(Box::new(select));
    }

    fn sync_color_from_hsva(&self) -> Color {
        if self.no_color {
            Color::transparent()
        } else {
            let (r, g, b) = hsv_to_rgb(self.h, self.s, self.v);
            Color::rgba(r, g, b, self.a)
        }
    }

    /// Publish the working HSVA colour for the Select button's callback.
    fn sync_working(&self) {
        let (r, g, b) = hsv_to_rgb(self.h, self.s, self.v);
        self.working.set(Color::rgba(r, g, b, self.a));
    }

    /// Close after Select.  The Select button's callback has already written
    /// the colour cell and fired `on_select`.
    fn finish_select(&mut self) {
        self.no_color = self.none_cell.get();
        self.live_dirty.set(false);
        self.open = false;
    }

    /// Live preview of an edit: push the working colour into the bound cell
    /// (unless No Color is checked, which previews transparent itself) and
    /// report it to the live-apply listener.
    fn preview_edit(&mut self) {
        let c = self.sync_color_from_hsva();
        if !self.no_color {
            self.color_cell.set(c);
        }
        self.report_live(c);
    }

    /// Report a live edit to [`on_change`](Self::on_change), if set.
    fn report_live(&self, c: Color) {
        if let Some(cb) = self.on_change.borrow_mut().as_mut() {
            cb(c);
            self.live_dirty.set(true);
        }
    }

    /// Close after Cancel and resync the working state from the snapshot.
    /// The Cancel button's callback has already restored the colour cell.
    /// A live-apply listener hears the restored colour (once).
    fn cancel(&mut self) {
        let saved = self.saved.get();
        self.color_cell.set(saved);
        report_restore(&self.on_change, &self.live_dirty, saved);
        let (h, s, v) = rgb_to_hsv(saved.r, saved.g, saved.b);
        self.h = h;
        self.s = s;
        self.v = v;
        self.a = saved.a;
        self.no_color = saved.a <= 0.0;
        self.none_cell.set(self.no_color);
        self.open = false;
    }

    /// Local-coord rect for each interactive region of the open panel.
    /// Inline: Y-up, the swatch is at the TOP and the panel fills the rest
    /// of the bounds below it.  Popup mode: the swatch is the whole widget
    /// and the panel floats outside it (see `popup_panel_rect`).
    fn regions(&self) -> PanelRegions {
        let w = self.bounds.width;
        let h = self.bounds.height;

        let (swatch, panel) = match self.popup_swatch {
            Some(d) => (Rect::new(0.0, 0.0, d, d), self.popup_panel_rect()),
            None => (
                Rect::new(0.0, h - SWATCH_H, w, SWATCH_H),
                Rect::new(0.0, 0.0, w, h - SWATCH_H),
            ),
        };
        let (px, pw) = (panel.x, panel.width);

        // Rows run down from the panel's top (Y-up → smaller Y).
        let mut y = panel.y + panel.height - PAD;

        y -= HUE_H;
        let hue = Rect::new(px + PAD, y, pw - PAD * 2.0, HUE_H);
        y -= ROW_GAP;

        y -= SV_H;
        let sv = Rect::new(px + PAD, y, pw - PAD * 2.0, SV_H);
        y -= ROW_GAP;

        y -= ALPHA_H;
        let alpha = Rect::new(px + PAD, y, pw - PAD * 2.0, ALPHA_H);
        y -= ROW_GAP;

        y -= HEX_H;
        let hex = Rect::new(px + PAD, y, pw - PAD * 2.0, HEX_H);
        y -= ROW_GAP;

        let none = if self.allow_none {
            y -= CHECK_H;
            let r = Rect::new(px + PAD, y, pw - PAD * 2.0, CHECK_H);
            Some(r)
        } else {
            None
        };
        let _ = y;

        let btns_y = panel.y + PAD;
        let btn_w = (pw - PAD * 3.0) * 0.5;
        let cancel = Rect::new(px + PAD, btns_y, btn_w, BTN_H);
        let select = Rect::new(px + PAD + btn_w + PAD, btns_y, btn_w, BTN_H);

        PanelRegions {
            swatch,
            panel,
            hue,
            sv,
            alpha,
            hex,
            none,
            cancel,
            select,
        }
    }
}

/// Report the colour Cancel restored to a live-apply listener that heard
/// edits since the panel opened.
fn report_restore(on_change: &SharedSelectCallback, dirty: &Cell<bool>, saved: Color) {
    if dirty.replace(false) {
        if let Some(cb) = on_change.borrow_mut().as_mut() {
            cb(saved);
        }
    }
}

struct PanelRegions {
    swatch: Rect,
    /// The panel's own rect (the area behind the rows below).
    panel: Rect,
    hue: Rect,
    sv: Rect,
    alpha: Rect,
    hex: Rect,
    none: Option<Rect>,
    cancel: Rect,
    select: Rect,
}

mod popup;
mod widget_impl;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod popup_tests;
