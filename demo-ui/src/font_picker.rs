//! `font_picker` — reusable font-selection widget for the demo app.
//!
//! Returns a configured `ComboBox` set up for the bundled font table:
//! - One entry per font in `windows::system::FONT_OPTIONS`
//! - The catalog lists app-owned font assets without parsing them up front.
//!   Selecting an unloaded font queues a platform-specific load request.
//! - Bound bidirectionally to the shared `font_index` cell on
//!   `windows::system::SystemCells` — picking a font in one window
//!   snaps every other picker in the app to the same selection on
//!   the next layout
//! - on_change automatically calls `apply_font_by_index`, which writes
//!   through to `font_settings::set_system_font`, the persisted
//!   `font_name` cell, and the shared `font_index` cell
//!
//! Drop `font_picker_with_size(font, size)` in anywhere a font choice is
//! exposed — the wiring (cell binding, lazy load request, on-change
//! side-effects) is handled here so call sites stay one line.

use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{ComboBox, DrawCtx, Event, EventResult, Font, Point, Rect, Size, Widget};

use crate::windows::{
    apply_font_by_index, font_cache_epoch, font_option_names, loaded_item_fonts,
    request_all_font_previews, system_cells as cells,
};

/// Build a font-picker `ComboBox` ready to drop into any layout.
///
/// `label_font` is the typeface used for the closed combo's selected-name
/// label and unloaded dropdown entries. Pass any reasonable fallback (the
/// window's body font is fine).  `font_size` sets the closed-combo's font
/// size — 13 pt matches the System window's body widgets.
///
/// The returned box is the picker itself — no wrapping.  All font-picker
/// behaviour comes from `ComboBox`'s built-in features plus the
/// cell-binding + on-change wiring set up here.
pub fn font_picker_with_size(label_font: Arc<Font>, font_size: f64) -> Box<dyn Widget> {
    let cells = cells();
    let names = font_option_names();
    let initial_idx = cells.font_index.get().min(names.len().saturating_sub(1));

    let cells_for_change = cells.clone();
    let mut combo = ComboBox::new(names, initial_idx, Arc::clone(&label_font))
        .with_font_size(font_size)
        .with_selected_cell(Rc::clone(&cells.font_index))
        .on_change(move |idx| {
            apply_font_by_index(&cells_for_change, idx);
        });
    combo.set_item_fonts(loaded_item_fonts(&label_font));

    Box::new(LazyFontPicker {
        combo,
        label_font,
        last_font_epoch: font_cache_epoch(),
        requested_previews: false,
    })
}

struct LazyFontPicker {
    combo: ComboBox,
    label_font: Arc<Font>,
    last_font_epoch: u64,
    requested_previews: bool,
}

impl LazyFontPicker {
    fn request_previews_once(&mut self) {
        if !self.requested_previews {
            request_all_font_previews(&cells());
            self.requested_previews = true;
        }
    }

    fn refresh_loaded_fonts(&mut self) {
        let epoch = font_cache_epoch();
        if epoch != self.last_font_epoch {
            self.combo
                .set_item_fonts(loaded_item_fonts(&self.label_font));
            self.last_font_epoch = epoch;
        }
    }
}

impl Widget for LazyFontPicker {
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
