//! "System" demo window — process-wide font / text-rendering toggles.
//!
//! Widgets read `agg_gui::font_settings::*` each frame (scrollbar-style
//! pattern), so changes here propagate live without a widget-tree rebuild.
//!
//! # Wired today
//! - **Font selector** — swaps `current_system_font` override.  Every
//!   `Label` (and widgets that compose a Label) re-measures and re-rasters
//!   on the next layout.
//! - **LCD + hinting toggles** flip their respective globals.  The render
//!   wire-up is staged for the next chunk; see module-level comments in
//!   `agg_gui::font_settings` and `agg_gui::text_lcd` (to be re-added).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    font_settings, ComboBox, FlexColumn, FlexRow, Font, Label,
    ScrollView, Separator, SizedBox, TextField, ToggleSwitch, Widget,
};

// ---------------------------------------------------------------------------
// Shared persistent cells — owned by `StateAccessor`, registered here at
// startup via `init_cells` so `system_view` can bind widgets without a new
// dispatcher signature.
// ---------------------------------------------------------------------------

/// Mirror of the system's persisted settings.  Each field is an `Rc<...>`
/// cell so the System window's widgets and the auto-save loop
/// (`StateAccessor::current_state`) share the same storage.
#[derive(Clone)]
pub struct SystemCells {
    pub font_name:       Rc<RefCell<Option<String>>>,
    pub font_size_scale: Rc<Cell<f64>>,
    pub lcd_enabled:     Rc<Cell<bool>>,
    pub hinting_enabled: Rc<Cell<bool>>,
}

thread_local! {
    static CELLS: RefCell<Option<SystemCells>> = RefCell::new(None);
}

/// Wire the System window's cells.  Call once from `build_demo_ui` before
/// the sidebar builds the first System window.
pub fn init_cells(cells: SystemCells) {
    CELLS.with(|c| *c.borrow_mut() = Some(cells));
}

/// Retrieve the registered cells.  Panics if `init_cells` wasn't called —
/// the demo shell always calls it, so this is a bug if it ever fires.
fn cells() -> SystemCells {
    CELLS.with(|c| c.borrow().clone().expect("system::init_cells not called"))
}

// ---------------------------------------------------------------------------
// Bundled fallback chain (icons + emoji) — attached to every primary font
// so code-points outside the primary's range still resolve.
// ---------------------------------------------------------------------------

const FA_ICONS:   &[u8] = include_bytes!("../../../demo/assets/fa.ttf");
const NOTO_EMOJI: &[u8] = include_bytes!("../../../demo/assets/NotoEmoji-Regular.ttf");

// ---------------------------------------------------------------------------
// Primary font table — enumerated from demo/assets.
//
// Each entry pairs a display name with its TTF bytes (baked via
// `include_bytes!` so no runtime file IO).  Keep the list alphabetical by
// display name so the dropdown has a sensible order.
// ---------------------------------------------------------------------------

struct NamedFont {
    name:  &'static str,
    bytes: &'static [u8],
}

/// Macro keeps the font table compact — one line per font instead of three.
macro_rules! font_table {
    ( $( ($disp:literal, $path:literal) ),* $(,)? ) => {
        &[
            $( NamedFont { name: $disp, bytes: include_bytes!($path) } ),*
        ]
    };
}

static FONT_OPTIONS: &[NamedFont] = font_table![
    ("Alfa Slab",              "../../../demo/assets/Alfa_Slab.ttf"),
    ("Arial",                  "../../../demo/assets/Arial-Regular.ttf"),
    ("Arial Italic",           "../../../demo/assets/Arial-Italic.ttf"),
    ("Audiowide",              "../../../demo/assets/Audiowide.ttf"),
    ("Bangers",                "../../../demo/assets/Bangers.ttf"),
    ("Cascadia Code",          "../../../demo/assets/CascadiaCode.ttf"),
    ("Courgette",              "../../../demo/assets/Courgette.ttf"),
    ("Damion",                 "../../../demo/assets/Damion.ttf"),
    ("Fredoka",                "../../../demo/assets/Fredoka.ttf"),
    ("Georgia",                "../../../demo/assets/Georgia-Regular.ttf"),
    ("Georgia Italic",         "../../../demo/assets/Georgia-Italic.ttf"),
    ("Great Vibes",            "../../../demo/assets/Great_Vibes.ttf"),
    ("Liberation Sans",        "../../../demo/assets/LiberationSans-Regular.ttf"),
    ("Liberation Sans Italic", "../../../demo/assets/LiberationSans-Italic.ttf"),
    ("Liberation Serif",       "../../../demo/assets/LiberationSerif-Regular.ttf"),
    ("Liberation Serif Italic","../../../demo/assets/LiberationSerif-Italic.ttf"),
    ("Lobster",                "../../../demo/assets/Lobster.ttf"),
    ("Nunito",                 "../../../demo/assets/Nunito_Regular.ttf"),
    ("Nunito Italic",          "../../../demo/assets/Nunito_Italic.ttf"),
    ("Nunito SemiBold",        "../../../demo/assets/Nunito_SemiBold.ttf"),
    ("Nunito Bold",            "../../../demo/assets/Nunito_Bold.ttf"),
    ("Nunito Bold Italic",     "../../../demo/assets/Nunito_Bold_Italic.ttf"),
    ("Pacifico",               "../../../demo/assets/Pacifico.ttf"),
    ("Poppins",                "../../../demo/assets/Poppins.ttf"),
    ("Questrial",              "../../../demo/assets/Questrial.ttf"),
    ("Righteous",              "../../../demo/assets/Righteous.ttf"),
    ("Russo",                  "../../../demo/assets/Russo.ttf"),
    ("Tahoma",                 "../../../demo/assets/Tahoma-Regular.ttf"),
    ("Times New Roman",        "../../../demo/assets/TimesNewRoman-Regular.ttf"),
    ("Times New Roman Italic", "../../../demo/assets/TimesNewRoman-Italic.ttf"),
    ("Titan",                  "../../../demo/assets/Titan.ttf"),
    ("Verdana",                "../../../demo/assets/Verdana-Regular.ttf"),
    ("Verdana Italic",         "../../../demo/assets/Verdana-Italic.ttf"),
];

/// Public lookup: find a font in `FONT_OPTIONS` by display name and return
/// a fully-loaded `Arc<Font>` (chained with icons + emoji fallback).  Used
/// on startup to rehydrate the persisted font choice.
pub fn load_font_by_name(name: &str) -> Option<Arc<Font>> {
    FONT_OPTIONS.iter().find(|o| o.name == name).map(load_font)
}

/// Load `opt` as the primary font, chained to the standard icons + emoji
/// fallback so code-points outside the primary's range still render.
fn load_font(opt: &NamedFont) -> Arc<Font> {
    let emoji = Font::from_slice(NOTO_EMOJI).expect("NotoEmoji");
    let fa    = Font::from_slice(FA_ICONS).expect("fa.ttf")
        .with_fallback(Arc::new(emoji));
    let font  = Font::from_slice(opt.bytes).expect("primary font")
        .with_fallback(Arc::new(fa));
    Arc::new(font)
}

/// Index of "Cascadia Code" in `FONT_OPTIONS` — the default the app ships
/// with.  Used as the combo's initial selection.
fn default_font_index() -> usize {
    FONT_OPTIONS.iter().position(|o| o.name == "Cascadia Code").unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Window builder
// ---------------------------------------------------------------------------

pub fn system_view(font: Arc<Font>) -> Box<dyn Widget> {
    let cells = cells();
    let mut col = FlexColumn::new().with_gap(10.0).with_padding(14.0);

    let heading = |text: &str| -> Box<dyn Widget> {
        Box::new(Label::new(text, Arc::clone(&font)).with_font_size(16.0))
    };
    let body = |text: &str| -> Box<dyn Widget> {
        Box::new(
            Label::new(text, Arc::clone(&font))
                .with_font_size(13.0)
                .with_wrap(true),
        )
    };

    col.push(heading("System"), 0.0);
    col.push(body(
        "Process-wide rendering toggles.  Change a setting here and every \
         widget that doesn't override it updates live on the next frame — \
         the same pattern scrollbars use with `current_scroll_style`.",
    ), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── Font selector ───────────────────────────────────────────────────
    col.push(heading("Font"), 0.0);
    col.push(body(
        "Enumerated from `demo/assets/` at build time.  All bundled TTFs \
         appear here; picking one swaps the system-wide override and every \
         `Label` re-measures on the next layout.",
    ), 0.0);
    {
        let names:    Vec<&'static str> = FONT_OPTIONS.iter().map(|o| o.name).collect();
        // Load ALL bundled fonts up front so each dropdown entry renders
        // its own display name in its own face.  ~10 MB total at 33
        // fonts; cost paid once when the System window is first opened.
        let per_item: Vec<Arc<Font>> = FONT_OPTIONS.iter().map(load_font).collect();

        // Seed initial selection from the persisted cell if the saved
        // name matches a known entry; otherwise default to Cascadia Code.
        let initial_idx = cells.font_name.borrow().as_deref()
            .and_then(|n| FONT_OPTIONS.iter().position(|o| o.name == n))
            .unwrap_or_else(default_font_index);

        let cells_for_combo = cells.clone();
        let combo = ComboBox::new(names, initial_idx, Arc::clone(&font))
            .with_font_size(14.0)
            .with_item_fonts(per_item)
            .on_change(move |idx| {
                if let Some(opt) = FONT_OPTIONS.get(idx) {
                    font_settings::set_system_font(Some(load_font(opt)));
                    *cells_for_combo.font_name.borrow_mut() = Some(opt.name.to_string());
                }
            });
        col.push(Box::new(combo), 0.0);
    }
    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── Font size scale ─────────────────────────────────────────────────
    col.push(heading("Text size"), 0.0);
    col.push(body(
        "System-wide font-size multiplier.  Scales every Label's size \
         proportionally — headings stay bigger than body, but the whole \
         UI grows or shrinks with the slider.  Range 0.5×–3.0×.",
    ), 0.0);
    {
        // Typable numeric input — a `TextField` that parses on edit-complete
        // (Enter or blur).  Out-of-range or non-numeric entries are ignored
        // (the cell / global stay at the last valid value), and the
        // clamp in `font_settings::set_font_size_scale` guards the range.
        let cells_for_size = cells.clone();
        let initial = format!("{:.2}", cells.font_size_scale.get());
        let field = TextField::new(Arc::clone(&font))
            .with_font_size(13.0)
            .with_text(initial)
            .with_select_all_on_focus(true)
            .on_edit_complete(move |s| {
                if let Ok(v) = s.trim().parse::<f64>() {
                    font_settings::set_font_size_scale(v);
                    // `set_font_size_scale` clamps; mirror the clamped
                    // value into the cell so disk save stays in range.
                    cells_for_size.font_size_scale
                        .set(font_settings::current_font_size_scale());
                }
            });
        // Wrap in a fixed-width `SizedBox` so the field looks like a
        // compact numeric input rather than stretching full-width.
        col.push(Box::new(
            SizedBox::new().with_width(100.0).with_height(28.0).with_child(Box::new(field)),
        ), 0.0);
    }
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── LCD subpixel ────────────────────────────────────────────────────
    col.push(heading("LCD subpixel text"), 0.0);
    col.push(body(
        "When enabled, Label backbuffers (solid-bg text: buttons, panels, \
         sidebar) pre-fill with their parent's bg colour and raster through \
         `PixfmtRgba32Lcd`, producing opaque RGBA where each glyph edge \
         carries per-channel coverage — the LCD look.  Text rendered \
         direct-to-screen (wrapped paragraphs, text fields) stays grayscale \
         AA because LCD only makes sense against a known opaque bg.",
    ), 0.0);
    {
        // Reuse the persisted cell directly so toggling writes through
        // to disk via the auto-save loop.
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
                Label::new("Enable LCD subpixel rendering",
                    Arc::clone(&font)).with_font_size(13.0),
            ));
        col.push(Box::new(row), 0.0);
    }
    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── Hinting ─────────────────────────────────────────────────────────
    col.push(heading("Hinting"), 0.0);
    col.push(body(
        "Grid-fits glyph outlines to whole pixels before rasterisation — \
         sharper small text on low-DPI monitors.  Flag is stored today; \
         actual hinting needs a TrueType interpreter beyond what `ttf-parser` \
         or the agg-rust `FontEngine` currently implement (both store the \
         flag without applying it).",
    ), 0.0);
    {
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
                Label::new("Enable glyph hinting (pending engine support)",
                    Arc::clone(&font)).with_font_size(13.0),
            ));
        col.push(Box::new(row), 0.0);
    }

    Box::new(ScrollView::new(Box::new(col)))
}
