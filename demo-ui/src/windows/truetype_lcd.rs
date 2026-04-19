//! "LCD Subpixel" demo window.
//!
//! Mirrors the AGG `truetype_test_02_win` (truetype_lcd.cpp) control
//! panel, built entirely out of agg-gui widgets.  Every control binds to
//! a cell owned by [`crate::windows::system::SystemCells`], so the
//! System window and this demo are two views onto the same model —
//! changes in either route through to `agg_gui::font_settings::*`
//! globals and flow into every widget that reads them on the next
//! frame.
//!
//! # Wired today
//! - Font picker (full bundled list)
//! - Font Scale slider → `font_size_scale`
//! - Faux Italic / Faux Weight / Interval / Width / Gamma / Primary Weight
//!   sliders → matching `font_settings` globals
//! - LCD + Hinting toggles → matching globals
//! - Sample paragraphs from the C++ reference rendered with the
//!   currently-selected system font
//!
//! # Pending (phase 2)
//! - Apply gamma / width / interval / faux-weight / faux-italic during
//!   glyph raster in `text.rs` + `lcd_coverage.rs`.  Controls are live;
//!   the visible text just doesn't respond to most of them yet.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    font_settings, FlexColumn, FlexRow, Font, Label,
    ScrollView, Separator, SizedBox, Slider,
    ToggleSwitch, Widget,
};

use super::system;

// ---------------------------------------------------------------------------
// C++ reference sample paragraphs
// ---------------------------------------------------------------------------

const TEXT1: &str = "A single pixel on a color LCD is made of three colored elements \
ordered (on various displays) either as blue, green, and red (BGR), \
or as red, green, and blue (RGB). These pixel components, sometimes \
called sub-pixels, appear as a single color to the human eye because \
of blurring by the optics and spatial integration by nerve cells in the eye.";

const TEXT2: &str = "The components are easily visible, however, when viewed with \
a small magnifying glass, such as a loupe. Over a certain resolution \
range the colors in the sub-pixels are not visible, but the relative \
intensity of the components shifts the apparent position or orientation \
of a line. Methods that take this interaction between the display \
technology and the human visual system into account are called \
subpixel rendering algorithms.";

const TEXT3: &str = "The resolution at which colored sub-pixels go unnoticed differs, \
however, with each user some users are distracted by the colored \
\"fringes\" resulting from sub-pixel rendering. Subpixel rendering \
is better suited to some display technologies than others. The \
technology is well-suited to LCDs, but less so for CRTs. In a CRT \
the light from the pixel components often spread across pixels, \
and the outputs of adjacent pixels are not perfectly independent.";

const TEXT4: &str = "If a designer knew precisely a great deal about the display's \
electron beams and aperture grille, subpixel rendering might \
have some advantage. But the properties of the CRT components, \
coupled with the alignment variations that are part of the \
production process, make subpixel rendering less effective for \
these displays. The technique should have good application to \
organic light emitting diodes and other display technologies.";

// ---------------------------------------------------------------------------
// Window builder
// ---------------------------------------------------------------------------

pub fn truetype_lcd_view(font: Arc<Font>) -> Box<dyn Widget> {
    let cells = system::cells();

    let heading = {
        let font = Arc::clone(&font);
        move |text: &str| -> Box<dyn Widget> {
            Box::new(Label::new(text, Arc::clone(&font)).with_font_size(16.0))
        }
    };

    let mut col = FlexColumn::new().with_gap(8.0).with_padding(14.0);

    // ── Font picker ─────────────────────────────────────────────────────
    col.push(heading("Font:"), 0.0);
    // Shared font picker — same widget used in the System window.
    // Picking a font here updates the System window's picker too on
    // the next layout, and vice-versa, via the shared `font_index`
    // cell on `SystemCells`.
    col.push(crate::font_picker::font_picker(Arc::clone(&font)), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── Typography-style sliders ────────────────────────────────────────
    //
    // Same bidirectional-cell wiring as the System window, so the two
    // windows share one model.  Each slider's label sits in a fixed-
    // width SizedBox so the sliders all line up regardless of label
    // length.
    col.push(heading("Style parameters"), 0.0);

    let style_row = {
        let font = Arc::clone(&font);
        move |label_text: &'static str,
              min: f64, max: f64, step: f64, decimals: usize,
              cell: Rc<Cell<f64>>,
              apply: Box<dyn Fn(f64)>|
              -> Box<dyn Widget> {
            // Label column — matches slider row height so vertical
            // centring lines up.
            let label_w = Box::new(
                SizedBox::new().with_width(120.0).with_height(22.0)
                    .with_child(Box::new(
                        Label::new(label_text, Arc::clone(&font))
                            .with_font_size(13.0)
                    ))
            );
            let slider = Slider::new(cell.get(), min, max, Arc::clone(&font))
                .with_step(step)
                .with_decimals(decimals)
                .with_value_cell(Rc::clone(&cell))
                .on_change(move |v| apply(v));
            // Slider goes in as a FLEX child so the FlexRow shrinks it
            // to the space left after the fixed-width label column and
            // gap.  Without the flex factor, Slider's `layout` reports
            // the full available width and the row overflows.
            let row = FlexRow::new().with_gap(10.0)
                .add(label_w)
                .add_flex(Box::new(slider), 1.0);
            Box::new(row) as Box<dyn Widget>
        }
    };

    col.push(style_row("Font Scale", 0.5, 2.0, 0.01, 2,
        Rc::clone(&cells.font_size_scale),
        Box::new(font_settings::set_font_size_scale)), 0.0);
    col.push(style_row("Faux Italic", -1.0, 1.0, 0.01, 2,
        Rc::clone(&cells.faux_italic),
        Box::new(font_settings::set_faux_italic)), 0.0);
    col.push(style_row("Faux Weight", -1.0, 1.0, 0.01, 2,
        Rc::clone(&cells.faux_weight),
        Box::new(font_settings::set_faux_weight)), 0.0);
    col.push(style_row("Interval", -0.2, 0.2, 0.001, 3,
        Rc::clone(&cells.interval),
        Box::new(font_settings::set_interval)), 0.0);
    col.push(style_row("Width", 0.75, 1.25, 0.01, 2,
        Rc::clone(&cells.width_scale),
        Box::new(font_settings::set_width)), 0.0);
    col.push(style_row("Gamma", 0.5, 2.5, 0.01, 2,
        Rc::clone(&cells.gamma),
        Box::new(font_settings::set_gamma)), 0.0);
    col.push(style_row("Primary Weight", 0.0, 1.0, 0.01, 2,
        Rc::clone(&cells.primary_weight),
        Box::new(font_settings::set_primary_weight)), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── LCD / Hinting toggles ───────────────────────────────────────────
    col.push(heading("Raster mode"), 0.0);

    {
        let font2 = Arc::clone(&font);
        let cell  = Rc::clone(&cells.lcd_enabled);
        let cell2 = Rc::clone(&cell);
        let row = FlexRow::new().with_gap(12.0)
            .add(Box::new(
                ToggleSwitch::new(cell.get())
                    .with_state_cell(Rc::clone(&cell))
                    .on_change(move |on| {
                        font_settings::set_lcd_enabled(on);
                        cell2.set(on);
                    })
            ))
            .add(Box::new(
                Label::new("LCD subpixel rendering",
                    Arc::clone(&font2)).with_font_size(13.0),
            ));
        col.push(Box::new(row), 0.0);
    }
    {
        let font2 = Arc::clone(&font);
        let cell  = Rc::clone(&cells.hinting_enabled);
        let cell2 = Rc::clone(&cell);
        let row = FlexRow::new().with_gap(12.0)
            .add(Box::new(
                ToggleSwitch::new(cell.get())
                    .with_state_cell(Rc::clone(&cell))
                    .on_change(move |on| {
                        font_settings::set_hinting_enabled(on);
                        cell2.set(on);
                    })
            ))
            .add(Box::new(
                Label::new("Hinting (Y-axis baseline snap)",
                    Arc::clone(&font2)).with_font_size(13.0),
            ));
        col.push(Box::new(row), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── Sample paragraphs ───────────────────────────────────────────────
    col.push(heading("Sample text"), 0.0);

    for text in [TEXT1, TEXT2, TEXT3, TEXT4] {
        col.push(Box::new(
            Label::new(text, Arc::clone(&font))
                .with_font_size(14.0)
                .with_wrap(true),
        ), 0.0);
    }

    Box::new(ScrollView::new(Box::new(col)))
}
