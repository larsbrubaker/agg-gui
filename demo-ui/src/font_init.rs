//! Font / system-cell initialisation extracted from
//! [`crate::app_builder::build_demo_ui`].
//!
//! Owns the construction of the persistent typography + render-tab cells,
//! pushes their values into `agg_gui::font_settings`, registers them with
//! the windows module's thread-local `SystemCells`, and triggers the first
//! font load.  Returning a single struct lets the caller use one binding
//! per cell instead of inlining the same `Rc::new(Cell::new(initial_state
//! .as_ref().map(...)))` pattern eleven times.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::api::PlatformHooks;
use crate::state::SavedState;
use crate::windows;

/// All persistent typography cells the demo shell needs to share between
/// the System window, the LCD-Subpixel demo, the backend panel, and the
/// state-persistence layer.
pub struct FontInitCells {
    pub font_name: Rc<RefCell<Option<String>>>,
    pub font_index: Rc<Cell<usize>>,
    pub font_size_scale: Rc<Cell<f64>>,
    pub lcd_enabled: Rc<Cell<bool>>,
    pub hinting_enabled: Rc<Cell<bool>>,
    pub gamma: Rc<Cell<f64>>,
    pub width_scale: Rc<Cell<f64>>,
    pub interval: Rc<Cell<f64>>,
    pub faux_weight: Rc<Cell<f64>>,
    pub faux_italic: Rc<Cell<f64>>,
    pub primary_weight: Rc<Cell<f64>>,
    pub msaa_samples: Rc<Cell<u8>>,
    pub system_tab: Rc<Cell<usize>>,
}

/// Build every typography cell from `initial_state`, push values into
/// `font_settings::set_*`, register the cells with the windows module's
/// thread-local registry, and trigger the first font load by index.
pub fn init(initial_state: Option<&SavedState>, platform: PlatformHooks) -> FontInitCells {
    let font_name = Rc::new(RefCell::new(
        initial_state.and_then(|s| s.font_name.clone()),
    ));
    let font_size_scale = Rc::new(Cell::new(
        initial_state.map(|s| s.font_size_scale).unwrap_or(1.0),
    ));
    let standard_dpi = agg_gui::device_scale() <= 1.25;
    let lcd_enabled = Rc::new(Cell::new(
        initial_state.map(|s| s.lcd_enabled).unwrap_or(standard_dpi),
    ));
    let hinting_enabled = Rc::new(Cell::new(
        initial_state
            .map(|s| s.hinting_enabled)
            .unwrap_or(standard_dpi),
    ));
    let gamma = Rc::new(Cell::new(initial_state.map(|s| s.gamma).unwrap_or(1.0)));
    let width_scale = Rc::new(Cell::new(
        initial_state.map(|s| s.width_scale).unwrap_or(1.0),
    ));
    let interval = Rc::new(Cell::new(initial_state.map(|s| s.interval).unwrap_or(0.0)));
    let faux_weight = Rc::new(Cell::new(
        initial_state.map(|s| s.faux_weight).unwrap_or(0.0),
    ));
    let faux_italic = Rc::new(Cell::new(
        initial_state.map(|s| s.faux_italic).unwrap_or(0.0),
    ));
    let primary_weight = Rc::new(Cell::new(
        initial_state.map(|s| s.primary_weight).unwrap_or(1.0 / 3.0),
    ));
    let msaa_samples = Rc::new(Cell::new(
        initial_state.map(|s| s.msaa_samples).unwrap_or(0),
    ));
    let system_tab = Rc::new(Cell::new(initial_state.map(|s| s.system_tab).unwrap_or(0)));
    let font_index = Rc::new(Cell::new({
        let name_lock = font_name.borrow();
        name_lock
            .as_deref()
            .and_then(windows::font_option_index)
            .unwrap_or_else(windows::default_font_index)
    }));

    agg_gui::font_settings::set_font_size_scale(font_size_scale.get());
    agg_gui::font_settings::set_lcd_enabled(lcd_enabled.get());
    agg_gui::font_settings::set_hinting_enabled(hinting_enabled.get());
    agg_gui::font_settings::set_gamma(gamma.get());
    agg_gui::font_settings::set_width(width_scale.get());
    agg_gui::font_settings::set_interval(interval.get());
    agg_gui::font_settings::set_faux_weight(faux_weight.get());
    agg_gui::font_settings::set_faux_italic(faux_italic.get());
    agg_gui::font_settings::set_primary_weight(primary_weight.get());

    let resolved_font_idx = font_name
        .borrow()
        .as_deref()
        .and_then(windows::font_option_index)
        .unwrap_or_else(|| font_index.get());

    windows::init_system_cells(windows::SystemCells {
        font_name: Rc::clone(&font_name),
        font_index: Rc::clone(&font_index),
        font_size_scale: Rc::clone(&font_size_scale),
        lcd_enabled: Rc::clone(&lcd_enabled),
        hinting_enabled: Rc::clone(&hinting_enabled),
        gamma: Rc::clone(&gamma),
        width_scale: Rc::clone(&width_scale),
        interval: Rc::clone(&interval),
        faux_weight: Rc::clone(&faux_weight),
        faux_italic: Rc::clone(&faux_italic),
        primary_weight: Rc::clone(&primary_weight),
        system_tab: Rc::clone(&system_tab),
        platform,
    });
    {
        let cells = windows::system_cells();
        windows::request_font_by_index(&cells, resolved_font_idx);
    }

    FontInitCells {
        font_name,
        font_index,
        font_size_scale,
        lcd_enabled,
        hinting_enabled,
        gamma,
        width_scale,
        interval,
        faux_weight,
        faux_italic,
        primary_weight,
        msaa_samples,
        system_tab,
    }
}
