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
    Button, CollapsingHeader, DrawCtx, Event, EventResult, FlexColumn, Font, HAnchor, Insets,
    Label, LabelAlign, Rect, ScrollView, Separator, Size, SizedBox, TextField, Widget,
};

/// One entry in the sidebar list (demo window, test window, or tool).
pub struct SidebarEntry {
    pub label: &'static str,
    /// Shared open-state cell: checkbox and window both read/write this.
    pub open: Rc<Cell<bool>>,
}

impl SidebarEntry {
    pub fn new(label: &'static str, initially_open: bool) -> Self {
        Self {
            label,
            open: Rc::new(Cell::new(initially_open)),
        }
    }
    pub fn from_cell(label: &'static str, cell: Rc<Cell<bool>>) -> Self {
        Self { label, open: cell }
    }
}

/// A group of sidebar entries shown together under one [`CollapsingHeader`].
pub struct SidebarGroup<'a> {
    pub name: &'static str,
    pub entries: Vec<&'a SidebarEntry>,
}

// ── ToggleButton ──────────────────────────────────────────────────────────────
//
// egui-style "toggle value" row: the entire row is clickable; the whole row
// gets an accent-colored background when the shared state cell is `true`.
// Hovering tints the row with a subtle highlight.

const TB_HEIGHT: f64 = 22.0;
const TB_INDENT: f64 = 22.0; // left indent so items nest under group triangle
const TB_FONT_SIZE: f64 = 13.0;
/// Vertical inset of the row background — leaves a 1 px sliver of the panel
/// colour above and below so consecutive selected rows don't fuse into one
/// solid block.
const TB_BG_INSET_V: f64 = 1.0;
/// Padding inside the fill, between the bg-left edge and the start of the
/// label (icon + text).  Matches the vertical breathing room so the fill
/// forms a consistent pill around the label.
const TB_BG_PAD_L: f64 = 5.0;
/// Left edge of the row background relative to the row's left edge.  Sits
/// just before the label so the pill hugs the text rather than extending
/// out into empty space on the left.
const TB_BG_INSET_L: f64 = TB_INDENT - TB_BG_PAD_L;
/// Right inset of the row background.  Matches the 1 px outer vertical
/// margin so the pill's right end has the same breathing room as top/bottom.
const TB_BG_INSET_R: f64 = 5.0;

/// Sidebar list row — full-width clickable pill that toggles a shared
/// `Rc<Cell<bool>>` (a demo window's open state).  Composed from a single
/// `Button` child styled with `with_subtle()` + `with_active_fn()`,
/// stretched horizontally and inset on the leading edge by `TB_INDENT`
/// so the label nests under its `CollapsingHeader` group triangle.
struct ToggleButton {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl ToggleButton {
    fn new(text: &str, font: Arc<Font>, state: Rc<Cell<bool>>) -> Self {
        let state_active = Rc::clone(&state);
        let state_click = state;
        let btn = Button::new(text, font)
            .with_font_size(TB_FONT_SIZE)
            .with_subtle()
            .with_h_anchor(HAnchor::STRETCH)
            .with_label_align(LabelAlign::Left)
            .with_label_pad_h(TB_INDENT)
            .with_active_fn(move || state_active.get())
            .on_click(move || {
                state_click.set(!state_click.get());
                agg_gui::animation::request_draw();
            });
        Self {
            bounds: Rect::default(),
            children: vec![Box::new(btn)],
        }
    }
}

impl Widget for ToggleButton {
    fn type_name(&self) -> &'static str {
        "ToggleButton"
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

    fn layout(&mut self, available: Size) -> Size {
        let w = available.width;
        self.bounds = Rect::new(0.0, 0.0, w, TB_HEIGHT);
        let pill_w = (w - TB_BG_INSET_L - TB_BG_INSET_R).max(1.0);
        let child = &mut self.children[0];
        child.layout(Size::new(pill_w, TB_HEIGHT - TB_BG_INSET_V * 2.0));
        child.set_bounds(Rect::new(
            TB_BG_INSET_L,
            TB_BG_INSET_V,
            pill_w,
            TB_HEIGHT - TB_BG_INSET_V * 2.0,
        ));
        Size::new(w, TB_HEIGHT)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {
        // Button child paints itself via the framework's tree walk.
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── FilterableItem ────────────────────────────────────────────────────────────
//
// Wraps a single `Checkbox` and reads a shared search cell on every layout.
// When the search string is non-empty and does not match `label` (case-
// insensitive substring), the wrapper collapses to zero height so the parent
// `FlexColumn` skips over it.

struct FilterableItem {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    label: String,
    search: Rc<RefCell<String>>,
}

impl Widget for FilterableItem {
    fn type_name(&self) -> &'static str {
        "FilterableItem"
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
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── Group wrapper ────────────────────────────────────────────────────────────
//
// When the user types a search string, any group whose entries all match must
// stay visible; empty groups collapse to zero height to avoid dead headers.

struct FilterableGroup {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // [0] = CollapsingHeader
    labels: Vec<String>,            // entry labels (for visibility check)
    search: Rc<RefCell<String>>,
}

impl Widget for FilterableGroup {
    fn type_name(&self) -> &'static str {
        "FilterableGroup"
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
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── Builders ──────────────────────────────────────────────────────────────────

/// Build one group: `CollapsingHeader("Widgets")` containing a `FlexColumn`
/// of filterable checkboxes.  Returns a boxed widget ready for insertion.
fn build_group(
    font: Arc<Font>,
    name: &'static str,
    entries: &[&SidebarEntry],
    search: Rc<RefCell<String>>,
) -> Box<dyn Widget> {
    let mut inner = FlexColumn::new().with_gap(0.0).with_padding(0.0);

    for entry in entries {
        // egui-style clickable row: whole row is the button, accent-filled
        // when selected.  Indented via TB_INDENT inside the widget's paint.
        let btn = ToggleButton::new(entry.label, Arc::clone(&font), Rc::clone(&entry.open));
        let item = FilterableItem {
            bounds: Rect::default(),
            children: vec![Box::new(btn)],
            label: entry.label.to_string(),
            search: Rc::clone(&search),
        };
        inner.push(Box::new(item), 0.0);
    }

    let header = CollapsingHeader::new(name, Arc::clone(&font))
        .default_open(true)
        .with_content(Box::new(inner));

    let labels: Vec<String> = entries.iter().map(|e| e.label.to_string()).collect();
    Box::new(FilterableGroup {
        bounds: Rect::default(),
        children: vec![Box::new(header)],
        labels,
        search,
    })
}

/// Build the complete sidebar.
pub fn build_sidebar(
    font: Arc<Font>,
    about_open: Rc<Cell<bool>>,
    groups: &[SidebarGroup],
    on_organize: impl FnMut() + 'static,
) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(0.0)
        .with_padding(0.0)
        .with_panel_bg();

    // ── Heading ──
    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    col.push(
        Box::new(
            Label::new("agg-gui Demo", Arc::clone(&font))
                .with_font_size(15.0)
                .with_margin(Insets::from_sides(12.0, 12.0, 4.0, 4.0)),
        ),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(6.0)), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(SizedBox::new().with_height(4.0)), 0.0);

    // ── About toggle row ── (FA4 "info-circle" icon prefix).  Uses the same
    // ToggleButton widget as every other sidebar entry so it visually matches.
    col.push(
        Box::new(ToggleButton::new(
            "\u{F05A} About",
            Arc::clone(&font),
            Rc::clone(&about_open),
        )),
        0.0,
    );
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(SizedBox::new().with_height(6.0)), 0.0);

    // ── Search field ──
    let search_cell: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    let search_for_cb = Rc::clone(&search_cell);
    col.push(
        Box::new(
            SizedBox::new()
                .with_height(26.0)
                .with_margin(Insets::from_sides(10.0, 10.0, 2.0, 4.0))
                .with_child(Box::new(
                    TextField::new(Arc::clone(&font))
                        .with_font_size(12.0)
                        .with_placeholder("\u{F002}  Search…")
                        .on_change(move |t| {
                            *search_for_cb.borrow_mut() = t.to_string();
                        }),
                )),
        ),
        0.0,
    );
    col.push(Box::new(SizedBox::new().with_height(6.0)), 0.0);

    // ── Scrollable group list + Organize button ──
    let mut list = FlexColumn::new()
        .with_gap(2.0)
        .with_padding(0.0)
        .with_panel_bg();

    for g in groups {
        list.push(
            build_group(
                Arc::clone(&font),
                g.name,
                &g.entries,
                Rc::clone(&search_cell),
            ),
            0.0,
        );
    }

    list.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    list.push(Box::new(Separator::horizontal()), 0.0);
    list.push(Box::new(SizedBox::new().with_height(6.0)), 0.0);
    list.push(
        Box::new(
            SizedBox::new()
                .with_height(28.0)
                .with_margin(Insets::from_sides(10.0, 10.0, 4.0, 4.0))
                .with_child(Box::new(
                    Button::new("Organize windows", Arc::clone(&font))
                        .with_font_size(12.0)
                        .on_click(on_organize),
                )),
        ),
        0.0,
    );
    list.push(Box::new(SizedBox::new().with_height(12.0)), 0.0);

    col.push(Box::new(ScrollView::new(Box::new(list))), 1.0);

    Box::new(col)
}
