//! `font_picker` — reusable font-preview dropdown for the demo app.
//!
//! Two layers live here:
//!
//! - [`font_preview_combo`] — the reusable builder. It wraps a [`ComboBox`]
//!   over `windows::system::FONT_OPTIONS` that renders EACH family name in its
//!   own face (Bangers in Bangers, Times in Times, …), loading those preview
//!   faces on demand the first time the user opens the picker and refreshing
//!   them as faces arrive (watched through the font-cache epoch). Selection is
//!   routed to a caller-supplied `on_change`; this builder deliberately never
//!   mutates the global system-font cells, so callers that must not disturb the
//!   System window's font (e.g. the RichText toolbar) can reuse the same
//!   preview UI safely.
//!
//! - [`font_picker_with_size`] — the System-window picker, built on top of
//!   [`font_preview_combo`]. It binds the shared `font_index` cell on
//!   `windows::system::SystemCells` (so every picker in the app snaps to the
//!   same selection) and routes `on_change` to `apply_font_by_index`, which
//!   writes through to `font_settings::set_system_font`, the persisted
//!   `font_name` cell, and the shared `font_index` cell.
//!
//! Drop `font_picker_with_size(font, size)` in anywhere a system-font choice is
//! exposed; reach for `font_preview_combo(...)` when you need the same
//! per-family preview UX but want to route the selection somewhere else.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{ComboBox, DrawCtx, Event, EventResult, Font, Point, Rect, Size, Widget};

use crate::windows::{
    apply_font_by_index, font_cache_epoch, font_option_names, loaded_item_fonts,
    request_all_font_previews, system_cells as cells, try_system_cells,
};

/// Build a reusable font-preview dropdown.
///
/// `label_font` is the fallback face used for the closed-combo label and for
/// any option whose preview face has not loaded yet. `font_size` sets the
/// closed-combo font size. `initial_idx` is the starting selection (clamped).
///
/// `selected_cell` optionally binds the combo's index to a shared
/// `Rc<Cell<usize>>` — the System window passes its `font_index` so every
/// picker stays in lock-step; pass `None` for an independent dropdown whose
/// selection is owned entirely by the caller through `on_change`.
///
/// The returned [`FontPreviewCombo`] implements [`Widget`]; box it to drop it
/// into a layout, or chain [`FontPreviewCombo::with_max_size`] /
/// [`FontPreviewCombo::with_reflect`] first.
pub fn font_preview_combo(
    label_font: Arc<Font>,
    font_size: f64,
    initial_idx: usize,
    selected_cell: Option<Rc<Cell<usize>>>,
    on_change: impl FnMut(usize) + 'static,
) -> FontPreviewCombo {
    let names: Vec<String> = font_option_names().iter().map(|s| s.to_string()).collect();
    let initial = initial_idx.min(names.len().saturating_sub(1));

    let mut combo = ComboBox::new(names, initial, Arc::clone(&label_font))
        .with_font_size(font_size)
        .on_change(on_change);
    if let Some(cell) = selected_cell {
        combo = combo.with_selected_cell(cell);
    }
    combo.set_item_fonts(loaded_item_fonts(&label_font));

    FontPreviewCombo {
        combo,
        label_font,
        last_font_epoch: font_cache_epoch(),
        requested_previews: false,
        reflect: None,
    }
}

/// Build the System-window font picker: a [`font_preview_combo`] bound to the
/// shared `font_index` cell and wired to `apply_font_by_index` on change.
///
/// `label_font` is the fallback typeface (the window's body font is fine);
/// `font_size` sets the closed-combo font size (13 pt matches the System
/// window's body widgets).
pub fn font_picker_with_size(label_font: Arc<Font>, font_size: f64) -> Box<dyn Widget> {
    let cells = cells();
    let initial_idx = cells.font_index.get();
    let cells_for_change = cells.clone();
    Box::new(font_preview_combo(
        label_font,
        font_size,
        initial_idx,
        Some(Rc::clone(&cells.font_index)),
        move |idx| apply_font_by_index(&cells_for_change, idx),
    ))
}

/// A [`ComboBox`] wrapper that lazily loads per-family preview faces and keeps
/// them in sync with the font-cache epoch. See [`font_preview_combo`].
pub struct FontPreviewCombo {
    combo: ComboBox,
    label_font: Arc<Font>,
    last_font_epoch: u64,
    requested_previews: bool,
    /// Optional per-frame reflection hook. Returning `Some(idx)` highlights
    /// that option; returning `None` (e.g. a mixed selection) leaves the
    /// current highlight untouched — the ComboBox has no unselected state.
    reflect: Option<Box<dyn Fn() -> Option<usize>>>,
}

impl FontPreviewCombo {
    /// Cap the closed-combo size (the RichText toolbar uses this to keep the
    /// family dropdown compact).
    pub fn with_max_size(mut self, size: Size) -> Self {
        if let Some(base) = self.combo.widget_base_mut() {
            base.max_size = size;
        }
        self
    }

    /// Drive the highlighted option from an external source each frame — used
    /// by the toolbar to reflect the editor selection's current family.
    /// `f` returns `Some(idx)` to highlight, or `None` to leave it as-is.
    pub fn with_reflect(mut self, f: impl Fn() -> Option<usize> + 'static) -> Self {
        self.reflect = Some(Box::new(f));
        self
    }

    fn request_previews_once(&mut self) {
        if self.requested_previews {
            return;
        }
        self.requested_previews = true;
        // Preview loading only asks the platform to fetch bytes into the shared
        // font cache; it never sets the system font. Skip gracefully when the
        // System window's cells are absent (tests, headless contexts).
        if let Some(cells) = try_system_cells() {
            request_all_font_previews(&cells);
        }
    }

    fn refresh_loaded_fonts(&mut self) {
        let epoch = font_cache_epoch();
        if epoch != self.last_font_epoch {
            // A preview face arrived; rebuild per-item labels with the now
            // loaded typefaces. The epoch bump already requested a redraw
            // (see `system_fonts::install_font_bytes`).
            self.combo
                .set_item_fonts(loaded_item_fonts(&self.label_font));
            self.last_font_epoch = epoch;
        }
    }

    fn apply_reflection(&mut self) {
        if self.combo.is_open() {
            return;
        }
        if let Some(reflect) = &self.reflect {
            if let Some(idx) = reflect() {
                if idx != self.combo.selected() {
                    self.combo.set_selected(idx);
                }
            }
        }
    }
}

impl Widget for FontPreviewCombo {
    fn bounds(&self) -> Rect {
        self.combo.bounds()
    }

    fn set_bounds(&mut self, bounds: Rect) {
        self.combo.set_bounds(bounds);
    }

    fn children(&self) -> &[Box<dyn Widget>] {
        self.combo.children()
    }

    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        self.combo.children_mut()
    }

    fn layout(&mut self, available: Size) -> Size {
        self.apply_reflection();
        self.refresh_loaded_fonts();
        self.combo.layout(available)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        self.refresh_loaded_fonts();
        self.combo.paint(ctx);
    }

    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        self.combo.paint_overlay(ctx);
    }

    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        self.combo.paint_global_overlay(ctx);
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        self.combo.hit_test(local_pos)
    }

    fn hit_test_global_overlay(&self, local_pos: Point) -> bool {
        self.combo.hit_test_global_overlay(local_pos)
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if matches!(event, Event::MouseDown { .. } | Event::KeyDown { .. }) {
            self.request_previews_once();
        }
        self.refresh_loaded_fonts();
        self.combo.on_event(event)
    }

    fn is_focusable(&self) -> bool {
        self.combo.is_focusable()
    }

    fn type_name(&self) -> &'static str {
        "FontPicker"
    }

    fn properties(&self) -> Vec<(&'static str, String)> {
        self.combo.properties()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agg_gui::Key;

    fn test_font() -> Arc<Font> {
        const BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");
        Arc::new(Font::from_bytes(BYTES.to_vec()).expect("load test font"))
    }

    /// The extracted builder routes selection to the caller's `on_change`
    /// without a bound cell and without initialising the System window's
    /// cells — so it never mutates the global system font. Arrow-key
    /// selection is enough to exercise the routing path.
    #[test]
    fn preview_combo_routes_on_change_without_system_cells() {
        let count = Rc::new(Cell::new(0usize));
        let last = Rc::new(Cell::new(usize::MAX));
        let count_cb = Rc::clone(&count);
        let last_cb = Rc::clone(&last);

        let mut combo = font_preview_combo(test_font(), 13.0, 0, None, move |idx| {
            count_cb.set(count_cb.get() + 1);
            last_cb.set(idx);
        });
        combo.layout(Size::new(150.0, 24.0));

        // ArrowDown moves to index 1 and fires on_change. Cells are not
        // initialised in this test; request_previews_once must no-op rather
        // than panic, proving the builder does not require the system cells.
        let res = combo.on_event(&Event::KeyDown {
            key: Key::ArrowDown,
            modifiers: Default::default(),
        });

        assert_eq!(res, EventResult::Consumed);
        assert_eq!(count.get(), 1);
        assert_eq!(last.get(), 1);
    }

    /// `with_reflect` drives the highlighted option each frame; `None` leaves
    /// the current highlight untouched (mixed-selection case).
    #[test]
    fn reflect_updates_selection_and_keeps_last_on_none() {
        let target = Rc::new(Cell::new(Some(3usize)));
        let target_hook = Rc::clone(&target);

        let mut combo = font_preview_combo(test_font(), 13.0, 0, None, |_| {})
            .with_reflect(move || target_hook.get());
        combo.layout(Size::new(150.0, 24.0));
        assert_eq!(combo.combo.selected(), 3);

        // Mixed selection: reflection returns None → keep the last family.
        target.set(None);
        combo.layout(Size::new(150.0, 24.0));
        assert_eq!(combo.combo.selected(), 3);
    }
}
