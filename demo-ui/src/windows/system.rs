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
    font_settings, FlexColumn, FlexRow, Font, Label, ScrollView, Separator, SizedBox, Slider,
    TabView, TextField, ToggleSwitch, Widget,
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
    pub font_name: Rc<RefCell<Option<String>>>,
    /// Index into `FONT_OPTIONS` matching `font_name`.  Shared between
    /// every font-picker `ComboBox` (System window + LCD Subpixel demo)
    /// via `with_selected_cell`, so picking a font in either window
    /// updates the other live.  Kept in lock-step with `font_name` by
    /// `apply_font_by_index` and the System window's combo callback.
    pub font_index: Rc<Cell<usize>>,
    pub font_size_scale: Rc<Cell<f64>>,
    pub lcd_enabled: Rc<Cell<bool>>,
    pub hinting_enabled: Rc<Cell<bool>>,
    // Typography-style mirrors — shared between this window's controls,
    // the TrueType LCD Subpixel demo's controls, and the font_settings
    // globals that (phase 2) the text render path will read.
    pub gamma: Rc<Cell<f64>>,
    pub width_scale: Rc<Cell<f64>>,
    pub interval: Rc<Cell<f64>>,
    pub faux_weight: Rc<Cell<f64>>,
    pub faux_italic: Rc<Cell<f64>>,
    pub primary_weight: Rc<Cell<f64>>,
    /// GL surface MSAA sample count (0/2/4/8/16).  Persisted via
    /// `StateAccessor::msaa_samples`; surfaced in the Render tab with an
    /// "applies on next launch" caveat.
    pub msaa_samples: Rc<Cell<u8>>,
    /// Active tab index inside the System window.  Bound to the TabView
    /// so clicks round-trip back into the persistence layer.
    pub system_tab: Rc<Cell<usize>>,
    /// Host-shell classification + relaunch/refresh hook.  Lives on
    /// `SystemCells` so `system_view` can render platform-appropriate
    /// controls (native 0/2/4/8/16 vs web on/off, "Relaunch" vs "Refresh"
    /// button label) without either platform crate carrying UI code.
    pub platform: crate::PlatformHooks,
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
///
/// Exposed to sibling windows (e.g. the TrueType LCD Subpixel demo) that
/// want to bind their own widgets to the same live cells — the whole
/// point of this module's init-once pattern.
pub fn cells() -> SystemCells {
    CELLS.with(|c| c.borrow().clone().expect("system::init_cells not called"))
}

pub fn try_cells() -> Option<SystemCells> {
    CELLS.with(|c| c.borrow().clone())
}

// ---------------------------------------------------------------------------
// Window builder
// ---------------------------------------------------------------------------

pub fn system_view(font: Arc<Font>) -> Box<dyn Widget> {
    // Split into two tabs so typography and OS-render settings don't
    // share a single wall-of-sliders.  Content body builders live below
    // as `build_font_tab` / `build_render_tab` so the TabView builder
    // can stay readable.
    let font_tab = build_font_tab(Arc::clone(&font));
    let render_tab = build_render_tab(Arc::clone(&font));
    let cells = cells();

    Box::new(
        TabView::new(Arc::clone(&font))
            .with_font_size(13.0)
            .add_tab("Font", font_tab)
            .add_tab("Render", render_tab)
            .with_active_tab_cell(Rc::clone(&cells.system_tab)),
    )
}

// ── Font tab ─────────────────────────────────────────────────────────────────

fn build_font_tab(font: Arc<Font>) -> Box<dyn Widget> {
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

    col.push(
        body("Process-wide text rendering settings.  Changes apply on the next frame."),
        0.0,
    );
    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── Font selector ───────────────────────────────────────────────────
    col.push(heading("Font"), 0.0);
    col.push(
        body("Sets the system font for every widget that doesn't override it."),
        0.0,
    );
    // Shared font picker — same widget used in the LCD Subpixel demo.
    // Owns its cell binding, per-item font loading, and on-change
    // apply-font wiring; picking a font here updates every other
    // FontPicker in the app on the next layout.
    col.push(
        crate::font_picker::font_picker_with_size(Arc::clone(&font), 14.0),
        0.0,
    );
    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── Point size ──────────────────────────────────────────────────────
    // Displayed as actual point size (base 14pt × scale).  Internally the
    // system still stores a scale multiplier for `font_settings`, so every
    // Label's declared size gets multiplied consistently; the user just
    // sees the resulting body-text point size and types in those units.
    const BASE_POINT_SIZE: f64 = 14.0;
    col.push(heading("Point size"), 0.0);
    col.push(
        body("Body-text size in points.  Scales every label proportionally.  Range 7–42 pt."),
        0.0,
    );
    {
        // Typable numeric input — a `TextField` that parses on edit-complete
        // (Enter or blur).  Out-of-range or non-numeric entries are ignored
        // (the cell / global stay at the last valid value), and the
        // clamp in `font_settings::set_font_size_scale` guards the range.
        let cells_for_size = cells.clone();
        let initial = format!("{:.1}", cells.font_size_scale.get() * BASE_POINT_SIZE);
        let field = TextField::new(Arc::clone(&font))
            .with_font_size(13.0)
            .with_text(initial)
            .with_select_all_on_focus(true)
            .on_edit_complete(move |s| {
                if let Ok(pt) = s.trim().parse::<f64>() {
                    font_settings::set_font_size_scale(pt / BASE_POINT_SIZE);
                    // `set_font_size_scale` clamps; mirror the clamped
                    // value into the cell so disk save stays in range.
                    cells_for_size
                        .font_size_scale
                        .set(font_settings::current_font_size_scale());
                }
            });
        // Wrap in a fixed-width `SizedBox` so the field looks like a
        // compact numeric input rather than stretching full-width.
        col.push(
            Box::new(
                SizedBox::new()
                    .with_width(100.0)
                    .with_height(28.0)
                    .with_child(Box::new(field)),
            ),
            0.0,
        );
    }
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── LCD subpixel ────────────────────────────────────────────────────
    col.push(heading("LCD subpixel text"), 0.0);
    col.push(
        body("Renders text using per-channel R/G/B coverage for sharper edges on LCD displays."),
        0.0,
    );
    {
        // Reuse the persisted cell directly so toggling writes through
        // to disk via the auto-save loop.
        let cell = Rc::clone(&cells.lcd_enabled);
        let cell2 = Rc::clone(&cell);
        let row = FlexRow::new()
            .with_gap(12.0)
            .add(Box::new(
                ToggleSwitch::new(cell.get())
                    .with_state_cell(Rc::clone(&cell))
                    .on_change(move |on| {
                        font_settings::set_lcd_enabled(on);
                        cell2.set(on);
                    }),
            ))
            .add(Box::new(
                Label::new("Enable LCD subpixel rendering", Arc::clone(&font)).with_font_size(13.0),
            ));
        col.push(Box::new(row), 0.0);
    }
    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── Hinting ─────────────────────────────────────────────────────────
    col.push(heading("Hinting"), 0.0);
    col.push(
        body(
            "Snaps glyph baselines to whole pixels for crisper text at small sizes.  \
         Required if you want LCD and grayscale renderers to land on the same \
         vertical position.",
        ),
        0.0,
    );
    {
        let cell = Rc::clone(&cells.hinting_enabled);
        let cell2 = Rc::clone(&cell);
        let row = FlexRow::new()
            .with_gap(12.0)
            .add(Box::new(
                ToggleSwitch::new(cell.get())
                    .with_state_cell(Rc::clone(&cell))
                    .on_change(move |on| {
                        font_settings::set_hinting_enabled(on);
                        cell2.set(on);
                    }),
            ))
            .add(Box::new(
                Label::new("Snap baselines to whole pixels", Arc::clone(&font))
                    .with_font_size(13.0),
            ));
        col.push(Box::new(row), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── Typography style parameters ─────────────────────────────────────
    //
    // Six `Slider` widgets, each bound to one of the `SystemCells` f64
    // cells via `with_value_cell`.  The on_change side mirrors the cell
    // write through to `agg_gui::font_settings::*` — the global that
    // (phase 2) the text render path will consume.  Cells + globals
    // stay in lock-step so the TrueType LCD Subpixel demo's widgets,
    // which bind to the same cells, move whenever anything here does
    // and vice-versa.
    col.push(heading("Typography style"), 0.0);
    col.push(
        body(
            "Process-wide style overrides applied to every glyph at paint time.  \
         Defaults are pass-through.",
        ),
        0.0,
    );

    // A closure captures `font` by reference so every slider row shares
    // one Arc<Font> clone.  Applies the value both to the persistent
    // cell (already handled by `with_value_cell`) and — via on_change
    // — to the matching `font_settings` global.
    let style_row = |label_text: &'static str,
                     min: f64,
                     max: f64,
                     step: f64,
                     cell: Rc<Cell<f64>>,
                     apply: Box<dyn Fn(f64)>|
     -> Box<dyn Widget> {
        let label_w = Box::new(
            SizedBox::new()
                .with_width(140.0)
                .with_height(22.0)
                .with_child(Box::new(
                    Label::new(label_text, Arc::clone(&font)).with_font_size(13.0),
                )),
        );
        let slider = Slider::new(cell.get(), min, max, Arc::clone(&font))
            .with_step(step)
            .with_value_cell(Rc::clone(&cell))
            .on_change(move |v| apply(v));
        // Slider is a flex child so the FlexRow shrinks it to the
        // space left after the fixed-width label column.
        let row = FlexRow::new()
            .with_gap(10.0)
            .add(label_w)
            .add_flex(Box::new(slider), 1.0);
        Box::new(row)
    };

    col.push(
        style_row(
            "Gamma",
            0.5,
            2.5,
            0.01,
            Rc::clone(&cells.gamma),
            Box::new(font_settings::set_gamma),
        ),
        0.0,
    );
    col.push(
        style_row(
            "Width",
            0.75,
            1.25,
            0.01,
            Rc::clone(&cells.width_scale),
            Box::new(font_settings::set_width),
        ),
        0.0,
    );
    col.push(
        style_row(
            "Interval",
            -0.2,
            0.2,
            0.001,
            Rc::clone(&cells.interval),
            Box::new(font_settings::set_interval),
        ),
        0.0,
    );
    col.push(
        style_row(
            "Faux Weight",
            -1.0,
            1.0,
            0.01,
            Rc::clone(&cells.faux_weight),
            Box::new(font_settings::set_faux_weight),
        ),
        0.0,
    );
    col.push(
        style_row(
            "Faux Italic",
            -1.0,
            1.0,
            0.01,
            Rc::clone(&cells.faux_italic),
            Box::new(font_settings::set_faux_italic),
        ),
        0.0,
    );
    col.push(
        style_row(
            "Primary Weight",
            0.0,
            1.0,
            0.01,
            Rc::clone(&cells.primary_weight),
            Box::new(font_settings::set_primary_weight),
        ),
        0.0,
    );

    Box::new(ScrollView::new(Box::new(col)))
}

// ── Render tab ───────────────────────────────────────────────────────────────

fn build_render_tab(font: Arc<Font>) -> Box<dyn Widget> {
    use crate::PlatformKind;
    use agg_gui::widgets::button::Button;

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

    // Platform-appropriate preamble.
    let intro = match cells.platform.kind {
        PlatformKind::Native => {
            "OS-level rendering settings.  The GL surface is created once at \
             startup, so changes here take effect on the next relaunch — use \
             the Relaunch button below after editing."
        }
        PlatformKind::Web => {
            "OS-level rendering settings.  The WebGL surface is created once \
             by the browser at canvas creation, so changes here take effect \
             after a page refresh — use the Refresh button below after editing."
        }
    };
    col.push(body(intro), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── MSAA ─────────────────────────────────────────────────────────────
    col.push(heading("MSAA"), 0.0);
    let msaa_body = match cells.platform.kind {
        PlatformKind::Native => {
            "Hardware multi-sample anti-aliasing for direct-GL content (e.g. \
             the 3D Animation cube grid).  Widget / text rendering uses \
             analytic halo-AA instead and is unaffected."
        }
        PlatformKind::Web => {
            "Hardware multi-sample anti-aliasing on the WebGL2 canvas.  The \
             browser WebGL spec only exposes a single boolean `antialias` \
             flag — the browser picks the sample count (typically 4×).  \
             Widget / text rendering uses analytic halo-AA instead and is \
             unaffected."
        }
    };
    col.push(body(msaa_body), 0.0);
    col.push(
        Box::new(crate::backend_panel::MsaaRow::new(
            Arc::clone(&font),
            Rc::clone(&cells.msaa_samples),
            // Native shells expose the full 5-way MSAA picker; the WASM
            // shell only gets `Off` / `On` because browser WebGL exposes
            // a boolean `antialias` flag (the browser picks the actual
            // sample count).  Same widget, different segments — both
            // write to `cells.msaa_samples`.
            match cells.platform.kind {
                PlatformKind::Native => crate::backend_panel::MsaaRow::NATIVE_SEGMENTS,
                PlatformKind::Web => crate::backend_panel::MsaaRow::WEB_SEGMENTS,
            },
        )) as Box<dyn Widget>,
        0.0,
    );

    // ── Relaunch / Refresh button ────────────────────────────────────────
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        body(
            "Apply the setting above by restarting the app.  Any unsaved UI \
         state — open windows, positions, z-order — is written to disk \
         before the restart, so your layout will come back exactly as \
         you left it.",
        ),
        0.0,
    );
    let btn_label = match cells.platform.kind {
        PlatformKind::Native => "Relaunch",
        PlatformKind::Web => "Refresh",
    };
    let reload = Rc::clone(&cells.platform.on_reload);
    let msaa_cell = Rc::clone(&cells.msaa_samples);
    let running_msaa = cells.platform.running_msaa;
    let kind = cells.platform.kind;
    // Button only enables when the persisted MSAA choice differs from
    // whatever's actually running right now — restart is pointless when
    // there's nothing to change.  Web host only gets a boolean MSAA, so
    // compare on `> 0` there instead of exact sample count.
    let reload_btn = Button::new(btn_label, Arc::clone(&font))
        .with_font_size(13.0)
        .with_enabled_fn(move || match kind {
            PlatformKind::Native => msaa_cell.get() != running_msaa,
            PlatformKind::Web => (msaa_cell.get() > 0) != (running_msaa > 0),
        })
        .on_click(move || (reload)());
    col.push(
        Box::new(
            SizedBox::new()
                .with_width(140.0)
                .with_height(30.0)
                .with_child(Box::new(reload_btn)),
        ),
        0.0,
    );

    Box::new(ScrollView::new(Box::new(col)))
}

// ── On/Off MSAA row (web — WebGL2 only exposes a boolean) ────────────────

