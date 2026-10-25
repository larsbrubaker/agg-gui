//! A [`MenuWidth::FitContent`] popup is sized to hold every row whole, so
//! none of its labels may end in an ellipsis, whatever the line box and
//! device scale.
//!
//! Popup row labels elide when their row is too narrow for them
//! (`labels.rs` `PopupLabels::paint_row`, added for phone-width windows).
//! Under `LineBox::EmSnapped` (MatterCAD's mode) a `Label` reports its width
//! rounded up to whole device pixels, while `fit_width.rs` sizes rows from
//! the unsnapped advance.  Subtracting the shortcut's snapped width left the
//! label a fraction of a pixel short: at DPR 1 in Cascadia 13 px "Export
//! Ctrl+E" painted "Ex…" and "Print Ctrl+P" painted "P…".

use std::sync::Arc;

use crate::font_settings::LineBox;
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Size};
use crate::gfx_ctx::GfxCtx;
use crate::input_profile::{profile_test_lock, set_input_profile, InputProfile};
use crate::text::Font;

use super::super::fit_width::MenuWidth;
use super::super::model::MenuItem;
use super::PopupMenu;

const FONT_SIZE: f64 = 13.0;
const VIEWPORT: Size = Size {
    width: 800.0,
    height: 600.0,
};

/// Desktop menu rows: label and shortcut.
const ROWS: [(&str, &str); 22] = [
    ("Export Selection As Wavefront OBJ", "Ctrl+Shift+E"),
    ("Open Recent Project From Disk", "Ctrl+Shift+O"),
    ("Open", "Ctrl+O"),
    ("Save", "Ctrl+S"),
    ("Save As", "Ctrl+Shift+S"),
    ("Export", "Ctrl+E"),
    ("Undo", "Ctrl+Z"),
    ("Redo", "Ctrl+Y"),
    ("Copy", "Ctrl+C"),
    ("Paste", "Ctrl+V"),
    ("Select All", "Ctrl+A"),
    ("Find", "Ctrl+F"),
    ("Replace", "Ctrl+H"),
    ("Close Tab", "Ctrl+W"),
    ("Print", "Ctrl+P"),
    ("Delete", "Del"),
    ("Duplicate", "Ctrl+D"),
    ("Group", "Ctrl+G"),
    ("Ungroup", "Ctrl+Shift+G"),
    ("Rotate", "R"),
    ("Zoom In", "Ctrl+="),
    ("Zoom Out", "Ctrl+-"),
];

fn test_font() -> Arc<Font> {
    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// Paint each row alone in a fitted popup under `line_box` at `dpr` and
/// return the rows whose label was elided, as `label -> shown text`.
fn elided_rows(line_box: LineBox, dpr: f64) -> Vec<String> {
    set_input_profile(InputProfile::Desktop);
    crate::touch_state::clear_last_touch_event_for_testing();
    crate::ux_scale::set_ux_scale(1.0);
    crate::device_scale::set_device_scale(dpr);
    crate::font_settings::set_line_box(line_box);
    crate::widget::set_current_viewport(VIEWPORT);
    let font = test_font();
    let mut elided = Vec::new();
    for (label, shortcut) in ROWS {
        let items = vec![MenuItem::action(label, "a").shortcut(shortcut).into()];
        let mut popup = PopupMenu::new(items).with_width(MenuWidth::FitContent);
        popup.set_measure_font(Arc::clone(&font), FONT_SIZE);
        popup.open_at(Point::new(20.0, 500.0));
        let mut fb = Framebuffer::new(
            (VIEWPORT.width * dpr) as u32,
            (VIEWPORT.height * dpr) as u32,
        );
        {
            let mut ctx = GfxCtx::new(&mut fb);
            ctx.scale(dpr, dpr);
            popup.paint(&mut ctx, Arc::clone(&font), FONT_SIZE, VIEWPORT);
        }
        let shown = popup.labels.row_label(0, 0).expect("row label");
        if shown.ellipsis_active() {
            elided.push(format!("{label} -> {}", shown.shown_text()));
        }
    }
    crate::font_settings::set_line_box(LineBox::Standard);
    crate::device_scale::set_device_scale(1.0);
    elided
}

#[test]
fn a_fitted_desktop_popup_never_elides_a_shortcut_rows_label() {
    let _guard = profile_test_lock();
    for line_box in [LineBox::EmSnapped, LineBox::Em, LineBox::Standard] {
        for dpr in [1.0, 1.25, 2.0] {
            let elided = elided_rows(line_box, dpr);
            assert!(
                elided.is_empty(),
                "{line_box:?} at DPR {dpr}: a fitted row's label was elided: {elided:?}"
            );
        }
    }
}
