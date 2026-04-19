//! `ComboBox` — a single-selection dropdown widget.
//!
//! When closed the widget occupies `CLOSED_H` pixels vertically.  When open
//! it expands downward in the layout (returning `CLOSED_H + n_items × ITEM_H`
//! from `layout()`), so sibling widgets are pushed down.  This works naturally
//! inside a `ScrollView` (the scroll area absorbs the extra height).
//!
//! Text for the selected value and dropdown items is rendered through
//! backbuffered [`Label`] children maintained in `selected_label` and
//! `item_labels`.  Colors are updated from `ctx.visuals()` in `paint()` so the
//! widget responds correctly to dark / light mode switches.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, EventResult, Key, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::{Widget, paint_subtree};
use crate::widgets::label::Label;

const CLOSED_H: f64 = 28.0;
const ITEM_H:   f64 = 24.0;
const PAD_X:    f64 = 8.0;
const ARROW_W:  f64 = 20.0;
const CORNER_R: f64 = 4.0;

/// A single-selection dropdown.
///
/// # Example
/// ```ignore
/// ComboBox::new(vec!["Option A", "Option B", "Option C"], 0, font)
///     .on_change(|idx| println!("selected {idx}"))
/// ```
pub struct ComboBox {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>, // always empty — labels stored separately
    base:     WidgetBase,

    options:  Vec<String>,
    selected: usize,
    open:     bool,
    /// Index of the item the cursor is currently over (only meaningful when open).
    hovered_item: Option<usize>,

    font:      Arc<Font>,
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
    item_labels:    Vec<Label>,
    /// Optional per-item font overrides, set via [`with_item_fonts`].
    /// `None` means every entry (and the selected label) uses `self.font`
    /// — the default.  `Some(vec)` means each entry uses `vec[i]` and
    /// the selected label uses `vec[selected]`, ignoring the system
    /// font override so font-preview UI stays stable.
    item_fonts:     Option<Vec<Arc<Font>>>,
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
        let item_labels = opts.iter().map(|t| {
            Self::make_label(t, font_size, Arc::clone(&font))
        }).collect();

        Self {
            bounds:   Rect::default(),
            children: Vec::new(),
            base:     WidgetBase::new(),
            options:  opts,
            selected: sel,
            open:     false,
            hovered_item: None,
            font,
            font_size,
            on_change: None,
            selected_cell: None,
            selected_label,
            item_labels,
            item_fonts: None,
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
        Label::new(text, font)
            .with_font_size(font_size)
    }

    // ── Builder ──────────────────────────────────────────────────────────────

    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self.selected_label = Self::make_label(
            self.options.get(self.selected).map(|s| s.as_str()).unwrap_or(""),
            size,
            Arc::clone(&self.font),
        );
        self.item_labels = self.options.iter().map(|t| {
            Self::make_label(t, size, Arc::clone(&self.font))
        }).collect();
        self
    }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }

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
        self.item_fonts = Some(fonts.clone());
        let size = self.font_size;
        self.item_labels = self.options.iter().enumerate().map(|(i, t)| {
            let f = fonts.get(i).cloned()
                .unwrap_or_else(|| Arc::clone(&self.font));
            Label::new(t, f)
                .with_font_size(size)
                .with_ignore_system_font(true)
        }).collect();
        // Rebuild the selected label with its matching font too.
        if let Some(sel_font) = fonts.get(self.selected).cloned() {
            self.selected_label = Label::new(
                self.options.get(self.selected).map(|s| s.as_str()).unwrap_or(""),
                sel_font,
            )
                .with_font_size(size)
                .with_ignore_system_font(true);
        }
        self
    }

    // ── Accessors ────────────────────────────────────────────────────────────

    pub fn selected(&self) -> usize { self.selected }

    pub fn set_selected(&mut self, idx: usize) {
        if idx < self.options.len() {
            self.selected = idx;
            // If per-item fonts are set, rebuild the selected label with
            // the matching face so the closed combo shows the correct
            // preview.  Otherwise just swap the text on the existing
            // label.
            if let Some(ref fonts) = self.item_fonts {
                if let Some(f) = fonts.get(idx).cloned() {
                    self.selected_label = Label::new(
                        self.options[idx].as_str(), f,
                    )
                        .with_font_size(self.font_size)
                        .with_ignore_system_font(true);
                    return;
                }
            }
            self.selected_label.set_text(self.options[idx].as_str());
        }
    }

    // ── Internal helpers ─────────────────────────────────────────────────────

    fn fire(&mut self) {
        let idx = self.selected;
        if let Some(cell) = &self.selected_cell { cell.set(idx); }
        if let Some(cb) = self.on_change.as_mut() { cb(idx); }
    }

    /// Height returned by `layout()` — varies with open/closed state.
    fn total_h(&self) -> f64 {
        if self.open {
            CLOSED_H + self.options.len() as f64 * ITEM_H
        } else {
            CLOSED_H
        }
    }

    /// Local Y coordinate of the TOP of item `i` (Y-up: larger = higher on screen).
    ///
    /// Items are drawn below the closed button area (y < 0 from the button
    /// bottom), but since layout expands downward in Y-up coordinates, item 0
    /// starts just below the button, which is at `total_h - CLOSED_H`.
    fn item_top_y(&self, i: usize) -> f64 {
        // In local Y-up space the button occupies [total_h-CLOSED_H .. total_h].
        // Items occupy [0 .. total_h-CLOSED_H], item 0 highest.
        let dropdown_h = self.total_h() - CLOSED_H;
        dropdown_h - (i as f64 * ITEM_H)
    }

    fn item_rect(&self, i: usize) -> Rect {
        let w  = self.bounds.width;
        let ty = self.item_top_y(i);
        Rect::new(0.0, ty - ITEM_H, w, ITEM_H)
    }

    /// Which dropdown item (if any) contains local point `p`.
    fn item_for_pos(&self, p: Point) -> Option<usize> {
        if !self.open { return None; }
        for i in 0..self.options.len() {
            let r = self.item_rect(i);
            if p.x >= r.x && p.x <= r.x + r.width
                && p.y >= r.y && p.y <= r.y + r.height
            {
                return Some(i);
            }
        }
        None
    }

    /// Whether `p` is inside the closed button area (top 28px of the widget).
    fn in_button(&self, p: Point) -> bool {
        let button_y = self.total_h() - CLOSED_H;
        p.x >= 0.0 && p.x <= self.bounds.width
            && p.y >= button_y && p.y <= self.total_h()
    }
}

impl Widget for ComboBox {
    fn type_name(&self) -> &'static str { "ComboBox" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn is_focusable(&self) -> bool { true }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

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

        let h = self.total_h();
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        let inner_w = (available.width - PAD_X * 2.0 - ARROW_W).max(0.0);

        // Layout selected label.
        let sl = self.selected_label.layout(Size::new(inner_w, CLOSED_H));
        let sl_y = (self.total_h() - CLOSED_H) + (CLOSED_H - sl.height) * 0.5;
        self.selected_label.set_bounds(Rect::new(PAD_X, sl_y, sl.width, sl.height));

        // Layout item labels — compute ty before borrowing item_labels.
        let dropdown_h = self.total_h() - CLOSED_H;
        for i in 0..self.item_labels.len() {
            let s = self.item_labels[i].layout(Size::new(inner_w, ITEM_H));
            let ty = dropdown_h - (i as f64 * ITEM_H);
            let ly = ty - ITEM_H + (ITEM_H - s.height) * 0.5;
            self.item_labels[i].set_bounds(Rect::new(PAD_X, ly, s.width, s.height));
        }

        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v   = ctx.visuals();
        let w   = self.bounds.width;
        let h   = self.total_h();
        // Button area (top section, Y-up).
        let btn_y = h - CLOSED_H;

        // ── Button background ─────────────────────────────────────────────────
        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, btn_y, w, CLOSED_H, CORNER_R);
        ctx.fill();

        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.0, btn_y, w, CLOSED_H, CORNER_R);
        ctx.stroke();

        // ── Dropdown arrow (▼) ────────────────────────────────────────────────
        let arrow_x = w - ARROW_W * 0.5;
        let arrow_cy = btn_y + CLOSED_H * 0.5;
        let arrow_sz = 4.0;
        ctx.set_fill_color(v.text_dim);
        ctx.begin_path();
        // Small downward triangle.
        ctx.move_to(arrow_x - arrow_sz, arrow_cy + arrow_sz * 0.5);
        ctx.line_to(arrow_x + arrow_sz, arrow_cy + arrow_sz * 0.5);
        ctx.line_to(arrow_x,            arrow_cy - arrow_sz * 0.5);
        ctx.close_path();
        ctx.fill();

        // ── Selected label ────────────────────────────────────────────────────
        self.selected_label.set_color(v.text_color);
        let sl_bounds = self.selected_label.bounds();

        ctx.save();
        ctx.translate(sl_bounds.x, sl_bounds.y);
        paint_subtree(&mut self.selected_label, ctx);
        ctx.restore();

        // ── Open dropdown ─────────────────────────────────────────────────────
        if self.open {
            let dropdown_h = h - CLOSED_H;

            // Dropdown panel background.
            ctx.set_fill_color(v.widget_bg);
            ctx.begin_path();
            ctx.rounded_rect(0.0, 0.0, w, dropdown_h, CORNER_R);
            ctx.fill();

            ctx.set_stroke_color(v.widget_stroke);
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.rounded_rect(0.0, 0.0, w, dropdown_h, CORNER_R);
            ctx.stroke();

            // Items.
            for i in 0..self.options.len() {
                let ir = self.item_rect(i);

                // Hover / selected highlight.
                let is_hovered  = self.hovered_item == Some(i);
                let is_selected = i == self.selected;
                if is_selected || is_hovered {
                    let bg = if is_selected { v.accent } else { v.widget_bg_hovered };
                    ctx.set_fill_color(bg);
                    ctx.begin_path();
                    ctx.rounded_rect(2.0, ir.y + 1.0, w - 4.0, ir.height - 2.0, 3.0);
                    ctx.fill();
                }

                // Label.
                let text_color = if is_selected { Color::white() } else { v.text_color };
                self.item_labels[i].set_color(text_color);
                let lb = self.item_labels[i].bounds();

                ctx.save();
                ctx.translate(lb.x, lb.y);
                paint_subtree(&mut self.item_labels[i], ctx);
                ctx.restore();
            }
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown { button: MouseButton::Left, pos, .. } => {
                if self.in_button(*pos) {
                    self.open = !self.open;
                    self.hovered_item = None;
                    return EventResult::Consumed;
                }
                if self.open {
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
                        self.fire();
                        return EventResult::Consumed;
                    }
                    // Click outside the dropdown — close it.
                    self.open = false;
                    self.hovered_item = None;
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseMove { pos } => {
                self.hovered_item = self.item_for_pos(*pos);
                EventResult::Ignored
            }
            Event::KeyDown { key, .. } => {
                let n = self.options.len();
                match key {
                    Key::Enter | Key::Char(' ') => {
                        self.open = !self.open;
                        EventResult::Consumed
                    }
                    Key::Escape => {
                        if self.open {
                            self.open = false;
                            EventResult::Consumed
                        } else {
                            EventResult::Ignored
                        }
                    }
                    Key::ArrowDown => {
                        if self.selected + 1 < n {
                            self.selected += 1;
                            self.selected_label.set_text(self.options[self.selected].as_str());
                            self.fire();
                        }
                        EventResult::Consumed
                    }
                    Key::ArrowUp => {
                        if self.selected > 0 {
                            self.selected -= 1;
                            self.selected_label.set_text(self.options[self.selected].as_str());
                            self.fire();
                        }
                        EventResult::Consumed
                    }
                    _ => EventResult::Ignored,
                }
            }
            Event::FocusLost => {
                self.open = false;
                self.hovered_item = None;
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("selected", self.selected.to_string()),
            ("open",     self.open.to_string()),
            ("options",  self.options.len().to_string()),
        ]
    }
}
