//! Right sidebar for the agg-gui demo shell.
//!
//! Layout (top → bottom):
//!   1. "agg-gui Demo" heading
//!   2. Separator
//!   3. About checkbox
//!   4. Separator
//!   5. Search field (filters the groups below by substring)
//!   6. Scrollable list of `CollapsingHeader` groups:
//!        Widgets · Layout · Graphics · Interaction · Tests · Tools
//!      Each entry is a [`FilterableItem`] that hides itself when the search
//!      text does not match its label.
//!   7. Separator
//!   8. "Organize windows" button
//!
//! Every entry's open/closed state is a shared `Rc<Cell<bool>>`, so the
//! sidebar checkbox and the window's own close button stay in sync.
//!
//! Filtering uses a simple case-insensitive substring match on the raw label
//! (the FA4 icon prefix is included in the string but never matches alphabetic
//! searches because it lives in the Private Use Area).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, Color, CollapsingHeader, DrawCtx, Event, EventResult,
    FlexColumn, Font, Insets, Label, MouseButton, Point, Rect, ScrollView,
    Separator, Size, SizedBox, TextField, Widget,
};
use agg_gui::widget::paint_subtree;

/// One entry in the sidebar list (demo window, test window, or tool).
pub struct SidebarEntry {
    pub label: &'static str,
    /// Shared open-state cell: checkbox and window both read/write this.
    pub open:  Rc<Cell<bool>>,
}

impl SidebarEntry {
    pub fn new(label: &'static str, initially_open: bool) -> Self {
        Self { label, open: Rc::new(Cell::new(initially_open)) }
    }
    pub fn from_cell(label: &'static str, cell: Rc<Cell<bool>>) -> Self {
        Self { label, open: cell }
    }
}

/// A group of sidebar entries shown together under one [`CollapsingHeader`].
pub struct SidebarGroup<'a> {
    pub name:    &'static str,
    pub entries: Vec<&'a SidebarEntry>,
}

// ── ToggleButton ──────────────────────────────────────────────────────────────
//
// egui-style "toggle value" row: the entire row is clickable; the whole row
// gets an accent-colored background when the shared state cell is `true`.
// Hovering tints the row with a subtle highlight.

const TB_HEIGHT:    f64 = 22.0;
const TB_INDENT:    f64 = 22.0;  // left indent so items nest under group triangle
const TB_R:         f64 = 4.0;
const TB_FONT_SIZE: f64 = 13.0;
/// Horizontal inset of the row background so the accent fill doesn't reach
/// all the way to the sidebar's right separator.
const TB_BG_INSET_L: f64 = 4.0;
const TB_BG_INSET_R: f64 = 8.0;

struct ToggleButton {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    label:    Label,
    state:    Rc<Cell<bool>>,
    hovered:  bool,
    pressed:  bool,
}

impl ToggleButton {
    fn new(text: &str, font: Arc<Font>, state: Rc<Cell<bool>>) -> Self {
        Self {
            bounds:   Rect::default(),
            children: Vec::new(),
            label:    Label::new(text, font).with_font_size(TB_FONT_SIZE),
            state,
            hovered:  false,
            pressed:  false,
        }
    }
}

impl Widget for ToggleButton {
    fn type_name(&self) -> &'static str { "ToggleButton" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let w = available.width;
        let inner_w = (w - TB_INDENT - 6.0).max(1.0);
        let ls = self.label.layout(Size::new(inner_w, TB_HEIGHT));
        // Vertically centre the label inside the row; left-align after the
        // indent used to mark group membership.
        let ly = ((TB_HEIGHT - ls.height) * 0.5).max(0.0);
        self.label.set_bounds(Rect::new(TB_INDENT, ly, ls.width.min(inner_w), ls.height));
        self.bounds = Rect::new(0.0, 0.0, w, TB_HEIGHT);
        Size::new(w, TB_HEIGHT)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;
        let selected = self.state.get();

        // Row background — hover darkens; pressed goes further; selected is accent-filled.
        let bg = if selected {
            v.accent
        } else if self.pressed {
            Color::rgba(v.text_color.r, v.text_color.g, v.text_color.b, 0.16)
        } else if self.hovered {
            Color::rgba(v.text_color.r, v.text_color.g, v.text_color.b, 0.10)
        } else {
            Color::rgba(0.0, 0.0, 0.0, 0.0)
        };
        if bg.a > 0.001 {
            let bx = TB_BG_INSET_L;
            let bw = (w - TB_BG_INSET_L - TB_BG_INSET_R).max(1.0);
            ctx.set_fill_color(bg);
            ctx.begin_path();
            ctx.rounded_rect(bx, 0.0, bw, h, TB_R);
            ctx.fill();
        }

        // Label colour inverts on selected so it stays readable on the accent bg.
        let text_color = if selected {
            let lum = 0.299 * v.accent.r + 0.587 * v.accent.g + 0.114 * v.accent.b;
            if lum < 0.5 { Color::white() } else { Color::rgb(0.08, 0.08, 0.10) }
        } else {
            v.text_color
        };
        self.label.set_color(text_color);

        // Draw the label at the position computed in `layout()` — left-aligned
        // after the group indent, vertically centred in the row.  Label's
        // LCD path samples the actual painted pixel beneath it, so no
        // bg declaration is needed here (the accent / hover / pressed
        // pill we just painted above IS the destination Label will read).
        let lb = self.label.bounds();
        ctx.save();
        ctx.translate(lb.x, lb.y);
        paint_subtree(&mut self.label, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let inside = pos.x >= 0.0 && pos.x <= self.bounds.width
                    && pos.y >= 0.0 && pos.y <= self.bounds.height;
                let was = self.hovered;
                self.hovered = inside;
                if !inside { self.pressed = false; }
                if was != self.hovered { EventResult::Consumed } else { EventResult::Ignored }
            }
            Event::MouseDown { button: MouseButton::Left, pos, .. } => {
                let inside = pos.x >= 0.0 && pos.x <= self.bounds.width
                    && pos.y >= 0.0 && pos.y <= self.bounds.height;
                if inside { self.pressed = true; EventResult::Consumed }
                else { EventResult::Ignored }
            }
            Event::MouseUp { button: MouseButton::Left, pos, .. } => {
                let inside = pos.x >= 0.0 && pos.x <= self.bounds.width
                    && pos.y >= 0.0 && pos.y <= self.bounds.height;
                let was_pressed = self.pressed;
                self.pressed = false;
                if was_pressed && inside {
                    self.state.set(!self.state.get());
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        local_pos.x >= 0.0 && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0 && local_pos.y <= self.bounds.height
    }
}

// ── FilterableItem ────────────────────────────────────────────────────────────
//
// Wraps a single `Checkbox` and reads a shared search cell on every layout.
// When the search string is non-empty and does not match `label` (case-
// insensitive substring), the wrapper collapses to zero height so the parent
// `FlexColumn` skips over it.

struct FilterableItem {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    label:    String,
    search:   Rc<RefCell<String>>,
}

impl Widget for FilterableItem {
    fn type_name(&self) -> &'static str { "FilterableItem" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let matches = {
            let q = self.search.borrow();
            q.is_empty() || self.label.to_lowercase().contains(&q.to_lowercase())
        };
        if !matches {
            self.bounds = Rect::new(0.0, 0.0, available.width, 0.0);
            if let Some(c) = self.children.first_mut() {
                c.set_bounds(Rect::new(0.0, 0.0, available.width, 0.0));
            }
            return Size::new(available.width, 0.0);
        }
        if let Some(c) = self.children.first_mut() {
            let sz = c.layout(available);
            c.set_bounds(Rect::new(0.0, 0.0, sz.width, sz.height));
            self.bounds = Rect::new(0.0, 0.0, sz.width, sz.height);
            Size::new(sz.width, sz.height)
        } else {
            self.bounds = Rect::new(0.0, 0.0, available.width, 0.0);
            Size::new(available.width, 0.0)
        }
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── Group wrapper ────────────────────────────────────────────────────────────
//
// When the user types a search string, any group whose entries all match must
// stay visible; empty groups collapse to zero height to avoid dead headers.

struct FilterableGroup {
    bounds:    Rect,
    children:  Vec<Box<dyn Widget>>, // [0] = CollapsingHeader
    labels:    Vec<String>,          // entry labels (for visibility check)
    search:    Rc<RefCell<String>>,
}

impl Widget for FilterableGroup {
    fn type_name(&self) -> &'static str { "FilterableGroup" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        // Hide the whole group if a non-empty search matches none of its labels.
        let group_visible = {
            let q = self.search.borrow();
            if q.is_empty() {
                true
            } else {
                let ql = q.to_lowercase();
                self.labels.iter().any(|l| l.to_lowercase().contains(&ql))
            }
        };
        if !group_visible {
            self.bounds = Rect::new(0.0, 0.0, available.width, 0.0);
            if let Some(c) = self.children.first_mut() {
                c.set_bounds(Rect::new(0.0, 0.0, available.width, 0.0));
            }
            return Size::new(available.width, 0.0);
        }
        if let Some(c) = self.children.first_mut() {
            let sz = c.layout(available);
            c.set_bounds(Rect::new(0.0, 0.0, sz.width, sz.height));
            self.bounds = Rect::new(0.0, 0.0, sz.width, sz.height);
            Size::new(sz.width, sz.height)
        } else {
            Size::new(available.width, 0.0)
        }
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── Builders ──────────────────────────────────────────────────────────────────

/// Build one group: `CollapsingHeader("Widgets")` containing a `FlexColumn`
/// of filterable checkboxes.  Returns a boxed widget ready for insertion.
fn build_group(
    font:    Arc<Font>,
    name:    &'static str,
    entries: &[&SidebarEntry],
    search:  Rc<RefCell<String>>,
) -> Box<dyn Widget> {
    let mut inner = FlexColumn::new()
        .with_gap(0.0)
        .with_padding(0.0);

    for entry in entries {
        // egui-style clickable row: whole row is the button, accent-filled
        // when selected.  Indented via TB_INDENT inside the widget's paint.
        let btn = ToggleButton::new(entry.label, Arc::clone(&font), Rc::clone(&entry.open));
        let item = FilterableItem {
            bounds:   Rect::default(),
            children: vec![Box::new(btn)],
            label:    entry.label.to_string(),
            search:   Rc::clone(&search),
        };
        inner.push(Box::new(item), 0.0);
    }

    let header = CollapsingHeader::new(name, Arc::clone(&font))
        .default_open(true)
        .with_content(Box::new(inner));

    let labels: Vec<String> = entries.iter().map(|e| e.label.to_string()).collect();
    Box::new(FilterableGroup {
        bounds:   Rect::default(),
        children: vec![Box::new(header)],
        labels,
        search,
    })
}

/// Build the complete sidebar.
pub fn build_sidebar(
    font:        Arc<Font>,
    about_open:  Rc<Cell<bool>>,
    groups:      &[SidebarGroup],
    on_organize: impl FnMut() + 'static,
) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(0.0)
        .with_padding(0.0)
        .with_panel_bg();

    // ── Heading ──
    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    col.push(Box::new(
        Label::new("agg-gui Demo", Arc::clone(&font))
            .with_font_size(15.0)
            .with_margin(Insets::from_sides(12.0, 12.0, 4.0, 4.0))
    ), 0.0);

    col.push(Box::new(SizedBox::new().with_height(6.0)), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(SizedBox::new().with_height(4.0)), 0.0);

    // ── About toggle row ── (FA4 "info-circle" icon prefix).  Uses the same
    // ToggleButton widget as every other sidebar entry so it visually matches.
    col.push(Box::new(
        ToggleButton::new("\u{F05A} About", Arc::clone(&font), Rc::clone(&about_open))
    ), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(SizedBox::new().with_height(6.0)), 0.0);

    // ── Search field ──
    let search_cell: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    let search_for_cb = Rc::clone(&search_cell);
    col.push(Box::new(
        SizedBox::new()
            .with_height(26.0)
            .with_margin(Insets::from_sides(10.0, 10.0, 2.0, 4.0))
            .with_child(Box::new(
                TextField::new(Arc::clone(&font))
                    .with_font_size(12.0)
                    .with_placeholder("\u{F002}  Search…")
                    .on_change(move |t| {
                        *search_for_cb.borrow_mut() = t.to_string();
                    })
            ))
    ), 0.0);
    col.push(Box::new(SizedBox::new().with_height(6.0)), 0.0);

    // ── Scrollable group list + Organize button ──
    let mut list = FlexColumn::new()
        .with_gap(2.0)
        .with_padding(0.0)
        .with_panel_bg();

    for g in groups {
        list.push(
            build_group(Arc::clone(&font), g.name, &g.entries, Rc::clone(&search_cell)),
            0.0,
        );
    }

    list.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    list.push(Box::new(Separator::horizontal()), 0.0);
    list.push(Box::new(SizedBox::new().with_height(6.0)), 0.0);
    list.push(Box::new(
        SizedBox::new()
            .with_height(28.0)
            .with_margin(Insets::from_sides(10.0, 10.0, 4.0, 4.0))
            .with_child(Box::new(
                Button::new("Organize windows", Arc::clone(&font))
                    .with_font_size(12.0)
                    .on_click(on_organize)
            ))
    ), 0.0);
    list.push(Box::new(SizedBox::new().with_height(12.0)), 0.0);

    col.push(Box::new(ScrollView::new(Box::new(list))), 1.0);

    Box::new(col)
}
