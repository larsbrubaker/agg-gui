//! "System" demo window — process-wide font / text-rendering toggles.
//!
//! Minds the scrollbar-style pattern: widgets read
//! `agg_gui::font_settings::*` each frame, so any change made here
//! propagates live without a widget-tree rebuild.
//!
//! # Wired today
//! - **Font selector** — ComboBox.  Swaps the global
//!   [`agg_gui::font_settings::current_system_font`] override.  `Label`
//!   (and by extension widgets that compose a `Label` — `Button`,
//!   `ToggleButton`, …) pick up the new font on the next layout/paint.
//!
//! # Staged but not yet rendered
//! - **LCD subpixel** — flag flips via `set_lcd_enabled`.  Actual LCD
//!   raster path in `Label` is the next chunk; requires:
//!   - A bg-color-aware path (LCD needs to know the destination bg to
//!     blend per-channel correctly).  Plumbed via a future
//!     `DrawCtx::surface_bg()` accessor, following MatterCAD's pre-fill
//!     pattern.
//!   - Bringing back the `text_lcd` module (deleted earlier) wired into
//!     Label's backbuffer closure.
//! - **Hinting** — flag flips via `set_hinting_enabled`.  Actual hinting
//!   requires a TrueType interpreter; `ttf-parser` and the agg-rust
//!   FontEngine both store the flag but neither applies it.  Future work
//!   is either FreeType bindings or the in-progress Rust hinter.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    font_settings, ComboBox, FlexColumn, FlexRow, Font, Label, ScrollView,
    Separator, ToggleSwitch, Widget,
};

// ---------------------------------------------------------------------------
// Bundled fonts
// ---------------------------------------------------------------------------

/// Primary app font — Cascadia Code, monospace.  Bundled in demo/assets.
const CASCADIA_CODE: &[u8]   = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
/// Secondary sans-serif for the selector demo — Liberation Sans.
const LIBERATION_SANS: &[u8] = include_bytes!("../../../demo/assets/LiberationSans-Regular.ttf");
const FA_ICONS: &[u8]        = include_bytes!("../../../demo/assets/fa.ttf");
const NOTO_EMOJI: &[u8]      = include_bytes!("../../../demo/assets/NotoEmoji-Regular.ttf");

/// Named entry in the font dropdown.  `bytes` points at the primary font;
/// the fallback chain (FA icons → emoji) is attached uniformly so icon
/// glyphs keep rendering no matter which primary font is selected.
struct NamedFont {
    name:  &'static str,
    bytes: &'static [u8],
}

const FONT_OPTIONS: &[NamedFont] = &[
    NamedFont { name: "CascadiaCode (monospace)",     bytes: CASCADIA_CODE },
    NamedFont { name: "LiberationSans (sans-serif)",  bytes: LIBERATION_SANS },
];

/// Load the font at `opt` with the standard icon+emoji fallback chain,
/// returning an `Arc<Font>` suitable for
/// [`font_settings::set_system_font`].
fn load_font(opt: &NamedFont) -> Arc<Font> {
    let emoji = Font::from_slice(NOTO_EMOJI).expect("NotoEmoji");
    let fa    = Font::from_slice(FA_ICONS).expect("fa.ttf")
        .with_fallback(Arc::new(emoji));
    let font  = Font::from_slice(opt.bytes).expect("primary font")
        .with_fallback(Arc::new(fa));
    Arc::new(font)
}

// ---------------------------------------------------------------------------
// Window builder
// ---------------------------------------------------------------------------

pub fn system_view(font: Arc<Font>) -> Box<dyn Widget> {
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
        "Swaps the system-wide font override.  Labels and any widget that \
         composes a Label pick it up on the next layout.",
    ), 0.0);
    {
        let names: Vec<&'static str> = FONT_OPTIONS.iter().map(|o| o.name).collect();
        // Initial index: 0 unless caller already set an override (we have no
        // cheap way to identify which bundled font matched an existing
        // override, so default to 0).
        let initial = 0;
        let combo = ComboBox::new(names, initial, Arc::clone(&font))
            .with_font_size(13.0)
            .on_change(|idx| {
                if let Some(opt) = FONT_OPTIONS.get(idx) {
                    font_settings::set_system_font(Some(load_font(opt)));
                }
            });
        col.push(Box::new(combo), 0.0);
    }
    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── LCD subpixel ────────────────────────────────────────────────────
    col.push(heading("LCD subpixel text"), 0.0);
    col.push(body(
        "When enabled, text rendering uses LCD per-channel (R/G/B) coverage \
         wherever the widget can determine its destination background \
         colour.  Widgets whose bg isn't known fall back to grayscale AA.  \
         Flag set; Label render path pending.",
    ), 0.0);
    {
        let state = Rc::new(Cell::new(font_settings::lcd_enabled()));
        let row = FlexRow::new().with_gap(12.0)
            .add(Box::new(
                ToggleSwitch::new(state.get())
                    .with_state_cell(Rc::clone(&state))
                    .on_change(|on| font_settings::set_lcd_enabled(on))
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
        "Enables the hinting pipeline from the agg-rust TrueType LCD \
         Subpixel demo.  Flag is stored today — actual grid-fitting needs \
         a TrueType interpreter beyond ttf-parser; the agg-rust `FontEngine` \
         stores the same flag without applying it.  Wire-up is planned \
         alongside the LCD render path.",
    ), 0.0);
    {
        let state = Rc::new(Cell::new(font_settings::hinting_enabled()));
        let row = FlexRow::new().with_gap(12.0)
            .add(Box::new(
                ToggleSwitch::new(state.get())
                    .with_state_cell(Rc::clone(&state))
                    .on_change(|on| font_settings::set_hinting_enabled(on))
            ))
            .add(Box::new(
                Label::new("Enable glyph hinting",
                    Arc::clone(&font)).with_font_size(13.0),
            ));
        col.push(Box::new(row), 0.0);
    }

    Box::new(ScrollView::new(Box::new(col)))
}
