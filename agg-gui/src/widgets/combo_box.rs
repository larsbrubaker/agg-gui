//! `ComboBox` — a single-selection dropdown widget.
//!
//! The widget always occupies its compact closed height.  When open, options
//! are painted as a floating panel below the button in `paint_overlay()` so
//! sibling widgets are not pushed down by the dropdown.
//!
//! Text for the selected value and dropdown items is rendered through
//! backbuffered [`Label`] children maintained in `selected_label` and
//! `item_labels`.  Colors are updated from `ctx.visuals()` in `paint()` so the
//! widget responds correctly to dark / light mode switches.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::label::Label;

use super::scroll_view::{current_scroll_style, current_scroll_visibility, ScrollBarStyle};
use super::scrollbar::{
    paint_prepared_scrollbar, PreparedScrollbar, ScrollbarAxis, ScrollbarGeometry,
    ScrollbarOrientation, DEFAULT_GRAB_MARGIN,
};

const CLOSED_H: f64 = 24.0;
const ITEM_H: f64 = 22.0;
const PAD_X: f64 = 8.0;
const ARROW_W: f64 = 20.0;
const CORNER_R: f64 = 4.0;
const POPUP_MARGIN: f64 = 4.0;
const MIN_VISIBLE_ITEMS: usize = 3;
const DEFAULT_VISIBLE_ITEMS: usize = 8;
const SCROLLBAR_W: f64 = 6.0;

struct ComboPopupRequest {
    x: f64,
    y: f64,
    width: f64,
    popup_h: f64,
    opens_up: bool,
    first_item: usize,
    visible_count: usize,
    selected: usize,
    hovered_item: Option<usize>,
    scrollbar: Option<PreparedScrollbar>,
    options: Vec<String>,
    font: Arc<Font>,
    font_size: f64,
    item_fonts: Option<Vec<Arc<Font>>>,
}

thread_local! {
    static COMBO_POPUP_QUEUE: RefCell<Vec<ComboPopupRequest>> = const { RefCell::new(Vec::new()) };
    static CURRENT_COMBO_VIEWPORT: Cell<Option<Size>> = const { Cell::new(None) };
}

/// A single-selection dropdown.
///
/// # Example
/// ```ignore
/// ComboBox::new(vec!["Option A", "Option B", "Option C"], 0, font)
///     .on_change(|idx| println!("selected {idx}"))
/// ```
pub struct ComboBox {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty — labels stored separately
    base: WidgetBase,

    options: Vec<String>,
    selected: usize,
    open: bool,
    /// Index of the item the cursor is currently over (only meaningful when open).
    hovered_item: Option<usize>,

    font: Arc<Font>,
    font_size: f64,

    on_change: Option<Box<dyn FnMut(usize)>>,
    /// Optional external mirror of `selected` — same bidirectional-cell
    /// pattern as `Slider::with_value_cell` / `RadioGroup::with_selected_cell`.
    /// `layout()` re-reads the cell every frame so a sibling ComboBox bound
    /// to the same cell stays in lock-step; selection changes here write back.
    selected_cell: Option<Rc<Cell<usize>>>,

    // ── Backbuffered labels ──────────────────────────────────────────────────
    /// Label for the currently selected option (shown in the closed button area).
    selected_label: Label,
    /// One label per option, used when the dropdown is open.
    item_labels: Vec<Label>,
    /// Optional per-item font overrides, set via [`with_item_fonts`].
    /// `None` means every entry (and the selected label) uses `self.font`
    /// — the default.  `Some(vec)` means each entry uses `vec[i]` and
    /// the selected label uses `vec[selected]`, ignoring the system
    /// font override so font-preview UI stays stable.
    item_fonts: Option<Vec<Arc<Font>>>,

    popup_opens_up: bool,
    popup_visible_count: usize,
    scroll_offset: usize,
    scrollbar: ScrollbarAxis,
    middle_dragging: bool,
    middle_last_pos: Point,
}

impl ComboBox {
    /// Create a new `ComboBox`.
    ///
    /// `options` is the full list of choices; `selected` is the initial index
    /// (clamped to a valid range).
    pub fn new(options: Vec<impl Into<String>>, selected: usize, font: Arc<Font>) -> Self {
        let font_size = 13.0;
        let opts: Vec<String> = options.into_iter().map(|s| s.into()).collect();
        let sel = selected.min(opts.len().saturating_sub(1));

        let selected_label = Self::make_label(
            opts.get(sel).map(|s| s.as_str()).unwrap_or(""),
            font_size,
            Arc::clone(&font),
        );
        let item_labels = opts
            .iter()
            .map(|t| Self::make_label(t, font_size, Arc::clone(&font)))
            .collect();

        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            options: opts,
            selected: sel,
            open: false,
            hovered_item: None,
            font,
            font_size,
            on_change: None,
            selected_cell: None,
            selected_label,
            item_labels,
            item_fonts: None,
            popup_opens_up: false,
            popup_visible_count: DEFAULT_VISIBLE_ITEMS,
            scroll_offset: 0,
            scrollbar: ScrollbarAxis {
                enabled: true,
                ..ScrollbarAxis::default()
            },
            middle_dragging: false,
            middle_last_pos: Point::ORIGIN,
        }
    }

    /// Bind this combo's selection to an external `Rc<Cell<usize>>`.
    /// `layout()` reads the cell each frame so a sibling combo (e.g. the
    /// matching font picker in another window) sharing the same cell
    /// stays in lock-step; user selections here write back.  Mirrors the
    /// `Slider::with_value_cell` / `RadioGroup::with_selected_cell` pattern.
    pub fn with_selected_cell(mut self, cell: Rc<Cell<usize>>) -> Self {
        let n = self.options.len();
        let v = cell.get();
        if n > 0 {
            let clamped = v.min(n - 1);
            // Initialise self.selected from the cell so the closed combo
            // shows the right label on first paint.
            self.set_selected(clamped);
        }
        self.selected_cell = Some(cell);
        self
    }

    fn make_label(text: &str, font_size: f64, font: Arc<Font>) -> Label {
        Label::new(text, font).with_font_size(font_size)
    }

    // ── Builder ──────────────────────────────────────────────────────────────

    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self.selected_label = Self::make_label(
            self.options
                .get(self.selected)
                .map(|s| s.as_str())
                .unwrap_or(""),
            size,
            Arc::clone(&self.font),
        );
        self.item_labels = self
            .options
            .iter()
            .map(|t| Self::make_label(t, size, Arc::clone(&self.font)))
            .collect();
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

    /// Set the callback called when the user selects a new option.
    pub fn on_change(mut self, cb: impl FnMut(usize) + 'static) -> Self {
        self.on_change = Some(Box::new(cb));
        self
    }

    /// Override the font used for EACH dropdown entry individually — one
    /// font per option.  Intended for font-preview UI (the System window's
    /// font picker renders each name in its own face).  Each item label
    /// is rebuilt with the matching `Arc<Font>` and marked to ignore the
    /// system-wide font override (otherwise changing the global font
    /// would overwrite all the per-entry faces).
    ///
    /// Lengths must match: `fonts.len()` should equal the number of
    /// options.  Extra fonts are ignored; missing entries keep the
    /// default `self.font`.  The SELECTED label (shown when the dropdown
    /// is closed) is also rebuilt with the currently-selected font so
    /// the closed combo reflects the live face.
    pub fn with_item_fonts(mut self, fonts: Vec<Arc<Font>>) -> Self {
        self.set_item_fonts(fonts);
        self
    }

    /// Replace per-item preview fonts after construction for lazy font UIs.
    pub fn set_item_fonts(&mut self, fonts: Vec<Arc<Font>>) {
        self.item_fonts = Some(fonts.clone());
        let size = self.font_size;
        self.item_labels = self
            .options
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let f = fonts
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| Arc::clone(&self.font));
                Label::new(t, f)
                    .with_font_size(size)
                    .with_ignore_system_font(true)
            })
            .collect();
        if let Some(sel_font) = fonts.get(self.selected).cloned() {
            self.selected_label = Label::new(
                self.options
                    .get(self.selected)
                    .map(|s| s.as_str())
                    .unwrap_or(""),
                sel_font,
            )
            .with_font_size(size)
            .with_ignore_system_font(true);
        }
    }

    // ── Accessors ────────────────────────────────────────────────────────────

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn set_selected(&mut self, idx: usize) {
        if idx < self.options.len() {
            self.selected = idx;
            // If per-item fonts are set, rebuild the selected label with
            // the matching face so the closed combo shows the correct
            // preview.  Otherwise just swap the text on the existing
            // label.
            if let Some(ref fonts) = self.item_fonts {
                if let Some(f) = fonts.get(idx).cloned() {
                    self.selected_label = Label::new(self.options[idx].as_str(), f)
                        .with_font_size(self.font_size)
                        .with_ignore_system_font(true);
                    return;
                }
            }
            self.selected_label.set_text(self.options[idx].as_str());
        }
    }
}

mod geometry;

impl Widget for ComboBox {
    fn type_name(&self) -> &'static str {
        "ComboBox"
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

    fn needs_draw(&self) -> bool {
        self.scrollbar.animation_active()
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        self.in_button(local_pos) || self.pos_in_popup(local_pos)
    }

    fn hit_test_global_overlay(&self, local_pos: Point) -> bool {
        self.pos_in_popup(local_pos)
    }

    fn margin(&self) -> Insets {
        self.base.margin
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
        // Pick up external-cell writes — e.g. a sibling combo bound to
        // the same selected_cell wrote a new index since our last paint.
        // Skip while open so an in-progress dropdown interaction doesn't
        // get yanked back.
        if !self.open {
            if let Some(cell) = &self.selected_cell {
                let n = self.options.len();
                if n > 0 {
                    let v = cell.get().min(n - 1);
                    if v != self.selected {
                        // Use set_selected so the visible label (and the
                        // per-item-font preview, if any) refreshes too.
                        self.set_selected(v);
                    }
                }
            }
        }

        self.bounds = Rect::new(0.0, 0.0, available.width, CLOSED_H);
        let inner_w = (available.width - PAD_X * 2.0 - ARROW_W).max(0.0);

        // Layout selected label.
        let sl = self.selected_label.layout(Size::new(inner_w, CLOSED_H));
        let sl_y = (CLOSED_H - sl.height) * 0.5;
        self.selected_label
            .set_bounds(Rect::new(PAD_X, sl_y, sl.width, sl.height));

        // Layout item labels in the floating panel. The panel may open
        // above or below depending on available screen space.
        for i in 0..self.item_labels.len() {
            let s = self.item_labels[i].layout(Size::new(inner_w, ITEM_H));
            let ir = self.item_rect(i);
            let ly = ir.y + (ITEM_H - s.height) * 0.5;
            self.item_labels[i].set_bounds(Rect::new(PAD_X, ly, s.width, s.height));
        }

        Size::new(available.width, CLOSED_H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;

        // ── Button background ─────────────────────────────────────────────────
        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, CLOSED_H, CORNER_R);
        ctx.fill();

        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, CLOSED_H, CORNER_R);
        ctx.stroke();

        // ── Dropdown arrow (▼) ────────────────────────────────────────────────
        let arrow_x = w - ARROW_W * 0.5;
        let arrow_cy = CLOSED_H * 0.5;
        let arrow_sz = 4.0;
        ctx.set_fill_color(v.text_dim);
        ctx.begin_path();
        // Small downward triangle.
        ctx.move_to(arrow_x - arrow_sz, arrow_cy + arrow_sz * 0.5);
        ctx.line_to(arrow_x + arrow_sz, arrow_cy + arrow_sz * 0.5);
        ctx.line_to(arrow_x, arrow_cy - arrow_sz * 0.5);
        ctx.close_path();
        ctx.fill();

        // ── Selected label ────────────────────────────────────────────────────
        self.selected_label.set_color(v.text_color);
        let sl_bounds = self.selected_label.bounds();

        ctx.save();
        ctx.translate(sl_bounds.x, sl_bounds.y);
        paint_subtree(&mut self.selected_label, ctx);
        ctx.restore();
    }

    fn paint_overlay(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        if self.open {
            let mut x = 0.0;
            let mut y = 0.0;
            let t = ctx.root_transform();
            t.transform(&mut x, &mut y);
            // `root_transform` includes the outer device-scale multiplier, but
            // the popup queue is drained while that same scale is still active
            // on the ctx and `viewport_h` (and the rest of the popup geometry)
            // are in logical units.  Strip the scale here so request coords
            // stay in logical root space — otherwise on HiDPI mobile (DPR 2-3)
            // the popup paints at scale²-magnified position while hit-testing
            // (which is purely logical) stays adjacent to the closed button.
            let scale = crate::device_scale::device_scale().max(1e-6);
            let x = x / scale;
            let y = y / scale;
            let viewport_h = crate::widgets::combo_box::current_combo_viewport()
                .map(|s| s.height)
                .unwrap_or(f64::MAX / 4.0);
            self.configure_popup_geometry(y, viewport_h);
            let style = self.popup_scroll_style();
            let visibility = current_scroll_visibility();
            let viewport = self.popup_scroll_viewport();
            let geom = self.scrollbar_geometry(style);
            let scrollbar = self
                .scrollbar
                .prepare_paint(viewport, style, visibility, geom)
                .map(|bar| bar.translated(x, y));
            submit_combo_popup(ComboPopupRequest {
                x,
                y,
                width: self.bounds.width,
                popup_h: self.popup_h(),
                opens_up: self.popup_opens_up,
                first_item: self.scroll_offset,
                visible_count: self.popup_visible_count,
                selected: self.selected,
                hovered_item: self.hovered_item,
                scrollbar,
                options: self.options.clone(),
                font: Arc::clone(&self.font),
                font_size: self.font_size,
                item_fonts: self.item_fonts.clone(),
            });
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown {
                button: MouseButton::Middle,
                pos,
                ..
            } => {
                if self.pos_in_popup(*pos) {
                    self.middle_dragging = true;
                    self.middle_last_pos = *pos;
                    self.hovered_item = None;
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
                if self.in_button(*pos) {
                    self.open = !self.open;
                    self.hovered_item = None;
                    self.scrollbar.hovered_bar = false;
                    self.scrollbar.hovered_thumb = false;
                    self.scrollbar.dragging = false;
                    self.middle_dragging = false;
                    if self.open {
                        self.ensure_selected_visible();
                    }
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                if self.open {
                    if self.pos_in_scrollbar(*pos) {
                        let style = self.popup_scroll_style();
                        let viewport = self.popup_scroll_viewport();
                        let geom = self.scrollbar_geometry(style);
                        self.sync_scrollbar_from_rows();
                        if self.scrollbar.begin_drag(*pos, viewport, style, geom) {
                            // No visible effect until the cursor moves.
                        } else if self.scrollbar.page_at(*pos, viewport, style, geom) {
                            self.sync_rows_from_scrollbar();
                        }
                        self.hovered_item = None;
                        self.scrollbar.hovered_thumb = self.pos_on_scroll_thumb(*pos);
                        crate::animation::request_draw();
                        return EventResult::Consumed;
                    }
                    if let Some(i) = self.item_for_pos(*pos) {
                        // Route through `set_selected` so the closed
                        // combo's preview label is rebuilt with the
                        // newly-selected per-item font (when item_fonts
                        // is set).  Direct `self.selected = i` would
                        // change the index without swapping the face,
                        // leaving the closed combo showing the new
                        // name in the OLD typeface — the bug visible
                        // when the LCD Subpixel demo's font picker
                        // showed e.g. "Bangers" in Cascadia Code.
                        self.set_selected(i);
                        self.open = false;
                        self.hovered_item = None;
                        self.scrollbar.hovered_bar = false;
                        self.scrollbar.hovered_thumb = false;
                        self.scrollbar.dragging = false;
                        self.middle_dragging = false;
                        self.fire();
                        crate::animation::request_draw();
                        return EventResult::Consumed;
                    }
                    // Click outside the dropdown — close it.
                    self.open = false;
                    self.hovered_item = None;
                    self.scrollbar.hovered_bar = false;
                    self.scrollbar.hovered_thumb = false;
                    self.scrollbar.dragging = false;
                    self.middle_dragging = false;
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseMove { pos } => {
                if self.middle_dragging {
                    let dy = pos.y - self.middle_last_pos.y;
                    self.middle_last_pos = *pos;
                    self.sync_scrollbar_from_rows();
                    if self.scrollbar.scroll_by(dy, self.popup_scroll_viewport()) {
                        self.sync_rows_from_scrollbar();
                        self.hovered_item = None;
                        crate::animation::request_draw();
                    }
                    return EventResult::Consumed;
                }
                if self.scrollbar.dragging {
                    let style = self.popup_scroll_style();
                    let viewport = self.popup_scroll_viewport();
                    let geom = self.scrollbar_geometry(style);
                    if self.scrollbar.drag_to(*pos, viewport, style, geom) {
                        self.sync_rows_from_scrollbar();
                        self.hovered_item = None;
                        crate::animation::request_draw();
                    }
                    return EventResult::Consumed;
                }
                let hovered_item = self.item_for_pos(*pos);
                let style = self.popup_scroll_style();
                let viewport = self.popup_scroll_viewport();
                let geom = self.scrollbar_geometry(style);
                let scroll_hover_changed = self.scrollbar.update_hover(*pos, viewport, style, geom);
                if hovered_item != self.hovered_item || scroll_hover_changed {
                    self.hovered_item = hovered_item;
                    crate::animation::request_draw();
                }
                EventResult::Ignored
            }
            Event::MouseWheel { delta_y, .. } => {
                if self.open && self.options.len() > self.popup_visible_count {
                    self.sync_scrollbar_from_rows();
                    if self
                        .scrollbar
                        .scroll_by(delta_y * 40.0, self.popup_scroll_viewport())
                    {
                        self.sync_rows_from_scrollbar();
                        self.hovered_item = None;
                        crate::animation::request_draw();
                    }
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::KeyDown { key, .. } => {
                let n = self.options.len();
                match key {
                    Key::Enter | Key::Char(' ') => {
                        self.open = !self.open;
                        self.scrollbar.hovered_bar = false;
                        self.scrollbar.hovered_thumb = false;
                        self.scrollbar.dragging = false;
                        self.middle_dragging = false;
                        if self.open {
                            self.ensure_selected_visible();
                        }
                        crate::animation::request_draw();
                        EventResult::Consumed
                    }
                    Key::Escape => {
                        if self.open {
                            self.open = false;
                            self.scrollbar.hovered_bar = false;
                            self.scrollbar.hovered_thumb = false;
                            self.scrollbar.dragging = false;
                            self.middle_dragging = false;
                            crate::animation::request_draw();
                            EventResult::Consumed
                        } else {
                            EventResult::Ignored
                        }
                    }
                    Key::ArrowDown => {
                        if self.selected + 1 < n {
                            self.set_selected(self.selected + 1);
                            self.ensure_selected_visible();
                            self.fire();
                            crate::animation::request_draw();
                        }
                        EventResult::Consumed
                    }
                    Key::ArrowUp => {
                        if self.selected > 0 {
                            self.set_selected(self.selected - 1);
                            self.ensure_selected_visible();
                            self.fire();
                            crate::animation::request_draw();
                        }
                        EventResult::Consumed
                    }
                    _ => EventResult::Ignored,
                }
            }
            Event::FocusLost => {
                let was_open = self.open;
                self.open = false;
                self.hovered_item = None;
                self.scrollbar.hovered_bar = false;
                self.scrollbar.hovered_thumb = false;
                self.scrollbar.dragging = false;
                self.middle_dragging = false;
                if was_open {
                    crate::animation::request_draw();
                }
                EventResult::Ignored
            }
            Event::MouseUp { button, .. } => {
                if *button == MouseButton::Left && self.scrollbar.dragging {
                    self.scrollbar.dragging = false;
                    crate::animation::request_draw();
                    EventResult::Consumed
                } else if *button == MouseButton::Middle && self.middle_dragging {
                    self.middle_dragging = false;
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
            ("selected", self.selected.to_string()),
            ("open", self.open.to_string()),
            ("options", self.options.len().to_string()),
            ("popup_opens_up", self.popup_opens_up.to_string()),
            ("popup_visible_count", self.popup_visible_count.to_string()),
            ("scroll_offset", self.scroll_offset.to_string()),
        ]
    }
}

fn submit_combo_popup(request: ComboPopupRequest) {
    COMBO_POPUP_QUEUE.with(|q| q.borrow_mut().push(request));
}

fn current_combo_viewport() -> Option<Size> {
    CURRENT_COMBO_VIEWPORT.with(|v| v.get())
}

pub(crate) fn begin_combo_popup_frame(viewport: Size) {
    CURRENT_COMBO_VIEWPORT.with(|v| v.set(Some(viewport)));
    COMBO_POPUP_QUEUE.with(|q| q.borrow_mut().clear());
}

pub(crate) fn paint_global_combo_popups(ctx: &mut dyn DrawCtx) {
    let requests = COMBO_POPUP_QUEUE.with(|q| q.borrow_mut().drain(..).collect::<Vec<_>>());
    if requests.is_empty() {
        return;
    }

    ctx.save();
    ctx.reset_clip();
    for request in requests {
        paint_combo_popup(ctx, request);
    }
    ctx.restore();
}

fn paint_combo_popup(ctx: &mut dyn DrawCtx, request: ComboPopupRequest) {
    let v = ctx.visuals();
    let popup_y = if request.opens_up {
        request.y + CLOSED_H
    } else {
        request.y - request.popup_h
    };

    // Opaque backing first. Some widget fills are intentionally subtle; the
    // popup itself must always obscure the content underneath.
    ctx.set_fill_color(v.window_fill);
    ctx.begin_path();
    ctx.rounded_rect(request.x, popup_y, request.width, request.popup_h, CORNER_R);
    ctx.fill();

    ctx.set_fill_color(v.widget_bg);
    ctx.begin_path();
    ctx.rounded_rect(request.x, popup_y, request.width, request.popup_h, CORNER_R);
    ctx.fill();

    let has_scroll = request.options.len() > request.visible_count;
    let text_w = if has_scroll {
        (request.width - SCROLLBAR_W - 4.0).max(0.0)
    } else {
        request.width
    };

    for row in 0..request.visible_count {
        let idx = request.first_item + row;
        let Some(text) = request.options.get(idx) else {
            break;
        };
        let item_y = popup_y + request.popup_h - (row as f64 + 1.0) * ITEM_H;
        let is_selected = idx == request.selected;
        let is_hovered = request.hovered_item == Some(idx);
        if is_selected || is_hovered {
            let bg = if is_selected {
                v.accent
            } else {
                v.widget_bg_hovered
            };
            ctx.set_fill_color(bg);
            ctx.begin_path();
            ctx.rounded_rect(
                request.x + 2.0,
                item_y + 1.0,
                text_w - 4.0,
                ITEM_H - 2.0,
                3.0,
            );
            ctx.fill();
        }

        let font = request
            .item_fonts
            .as_ref()
            .and_then(|fonts| fonts.get(idx))
            .cloned()
            .unwrap_or_else(|| Arc::clone(&request.font));
        ctx.set_font(font);
        ctx.set_font_size(request.font_size);
        ctx.set_fill_color(if is_selected {
            Color::white()
        } else {
            v.text_color
        });
        let baseline = item_y + (ITEM_H - request.font_size) * 0.5;
        ctx.fill_text(text, request.x + PAD_X, baseline);
    }

    if let Some(scrollbar) = request.scrollbar {
        paint_prepared_scrollbar(ctx, scrollbar);
    }

    ctx.set_stroke_color(v.widget_stroke);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.rounded_rect(request.x, popup_y, request.width, request.popup_h, CORNER_R);
    ctx.stroke();
}
