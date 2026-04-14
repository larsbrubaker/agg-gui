//! Right sidebar for the egui-style demo shell.
//!
//! Builds a fixed-width dark panel matching egui's right demo panel:
//! - "agg-gui Demo" heading
//! - Scrollable checklist grouped by "Demos" and "Tests" sections
//! - "Organize windows" button at the BOTTOM of the scroll list
//!
//! Each checklist item uses `Checkbox::with_state_cell` so opening/closing a
//! window from either the sidebar or the window's own close button stays in sync.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, Checkbox, Color, FlexColumn, Font, Insets, Label, ScrollView,
    Separator, SizedBox, Widget,
};
use agg_gui::widgets::button::ButtonTheme;

/// One entry in the sidebar checklist.
pub struct SidebarEntry {
    pub label: &'static str,
    /// Shared open/close state: checkbox and window both read/write this cell.
    pub open:  Rc<Cell<bool>>,
}

impl SidebarEntry {
    pub fn new(label: &'static str, initially_open: bool) -> Self {
        Self { label, open: Rc::new(Cell::new(initially_open)) }
    }
}

/// Build the sidebar widget from entry slices plus an "Organize windows" callback.
///
/// The returned widget should be wrapped in a `SizedBox::with_width(220.0)` by
/// the caller.  Layout mirrors egui's right demo panel exactly:
/// heading → About checkbox → scrollable (Demos section, Tests section, separator, Organize button).
pub fn build_sidebar(
    font:        Arc<Font>,
    about_open:  Rc<Cell<bool>>,
    demos:       &[SidebarEntry],
    tests:       &[SidebarEntry],
    on_organize: impl FnMut() + 'static,
) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(0.0)
        .with_padding(0.0)
        .with_panel_bg();

    // ── Heading ──────────────────────────────────────────────────────────────
    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    col.push(Box::new(
        Label::new("agg-gui Demo", Arc::clone(&font))
            .with_font_size(15.0)
            .with_margin(Insets::from_sides(12.0, 12.0, 4.0, 4.0))
    ), 0.0);

    col.push(Box::new(SizedBox::new().with_height(6.0)), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(SizedBox::new().with_height(4.0)), 0.0);

    // ── "About" checkbox at the top, matching egui ────────────────────────────
    col.push(Box::new(
        Checkbox::new("About", Arc::clone(&font), about_open.get())
            .with_font_size(13.0)
            .with_state_cell(Rc::clone(&about_open))
            .with_margin(Insets::from_sides(10.0, 0.0, 2.0, 2.0))
    ), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(SizedBox::new().with_height(4.0)), 0.0);

    // ── Scrollable checklist ──────────────────────────────────────────────────
    // "Organize windows" lives at the BOTTOM of the scroll area, matching egui.
    let mut list = FlexColumn::new()
        .with_gap(0.0)
        .with_padding(0.0)
        .with_panel_bg();

    append_section(&mut list, &font, "Demos", demos);
    list.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    append_section(&mut list, &font, "Tests", tests);

    // ── Separator + Organize button at scroll-list bottom ─────────────────────
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
                    .with_theme(ButtonTheme {
                        background:         Color::rgba(1.0, 1.0, 1.0, 0.08),
                        background_hovered: Color::rgba(1.0, 1.0, 1.0, 0.14),
                        background_pressed: Color::rgba(1.0, 1.0, 1.0, 0.20),
                        label_color:        Color::rgba(1.0, 1.0, 1.0, 0.85),
                        border_radius:      5.0,
                        focus_ring_color:   Color::rgba(1.0, 1.0, 1.0, 0.30),
                        focus_ring_width:   2.0,
                    })
                    .on_click(on_organize)
            ))
    ), 0.0);
    list.push(Box::new(SizedBox::new().with_height(12.0)), 0.0);

    col.push(Box::new(ScrollView::new(Box::new(list))), 1.0);

    Box::new(col)
}

fn append_section(
    col:     &mut FlexColumn,
    font:    &Arc<Font>,
    title:   &'static str,
    entries: &[SidebarEntry],
) {
    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    col.push(Box::new(
        Label::new(title, Arc::clone(font))
            .with_font_size(11.0)
            .with_margin(Insets::from_sides(14.0, 0.0, 2.0, 2.0))
    ), 0.0);

    for entry in entries {
        // `with_state_cell` makes the checkbox fully reactive: paint reads from
        // `entry.open`, and toggle writes to it.  No separate `on_change` needed.
        col.push(Box::new(
            Checkbox::new(entry.label, Arc::clone(font), entry.open.get())
                .with_font_size(13.0)
                .with_state_cell(Rc::clone(&entry.open))
                .with_margin(Insets::from_sides(10.0, 0.0, 1.0, 1.0))
        ), 0.0);
    }
}
