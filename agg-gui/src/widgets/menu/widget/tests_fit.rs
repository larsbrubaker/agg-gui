//! Tests for popup width policy ([`super::super::fit_width`]) and for the
//! menu bar painting its laid-out height.
//!
//! Every test here depends on [`super::super::effective_metrics`], which reads
//! the process-global input profile, so each holds
//! `input_profile::profile_test_lock()` and pins `InputProfile::Desktop`.

use std::sync::Arc;

use crate::event::{Event, EventResult, Modifiers, MouseButton};
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::input_profile::{profile_test_lock, set_input_profile, InputProfile};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::label::Label;
use crate::Color;

use super::super::fit_width::{MenuWidth, FIT_MIN_W, SHORTCUT_GAP};
use super::super::geometry::{BAR_H, MENU_W, TOUCH_MIN};
use super::super::model::{MenuEntry, MenuItem};
use super::super::paint::MenuStyle;
use super::{MenuBar, PopupMenu, TopMenu};

fn test_font() -> Arc<Font> {
    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn reset_env() {
    set_input_profile(InputProfile::Desktop);
    crate::touch_state::clear_last_touch_event_for_testing();
    crate::ux_scale::set_ux_scale(1.0);
    crate::device_scale::set_device_scale(1.0);
}

/// Width of `text` as a popup row's `Label` lays it out at paint time.
fn label_width(font: &Arc<Font>, text: &str, size: f64) -> f64 {
    let mut label = Label::new(text, Arc::clone(font)).with_font_size(size);
    label.layout(Size::new(10_000.0, 100.0)).width
}

/// Natural width of a row as painted: label from `label_x`, then the
/// shortcut `SHORTCUT_GAP` later, then `shortcut_right` of padding.
fn painted_row_width(font: &Arc<Font>, item: &MenuItem, size: f64) -> f64 {
    let style = MenuStyle::default();
    let shortcut = item
        .shortcut_text_for_font(crate::platform::current_platform(), font)
        .map_or(0.0, |s| SHORTCUT_GAP + label_width(font, &s, size));
    style.label_x + label_width(font, &item.label, size) + shortcut + style.shortcut_right
}

fn long_items() -> Vec<MenuEntry> {
    vec![
        MenuItem::action("Open", "open").shortcut("Ctrl+O").into(),
        MenuEntry::Separator,
        MenuItem::action("Export Selection As Wavefront OBJ", "export")
            .shortcut("Ctrl+Shift+E")
            .into(),
        MenuItem::action("A Rather Long Label Without Any Shortcut", "long").into(),
    ]
}

fn fitted_root_width(items: Vec<MenuEntry>, font: &Arc<Font>) -> f64 {
    let mut popup = PopupMenu::new(items).with_width(MenuWidth::FitContent);
    popup.set_measure_font(Arc::clone(font), 14.0);
    popup.open_at(Point::new(20.0, 500.0));
    popup.state.layouts(&popup.items, Size::new(1200.0, 600.0))[0]
        .rect
        .width
}

fn assert_fits_widest_row(device_scale: f64) {
    let _guard = profile_test_lock();
    reset_env();
    crate::device_scale::set_device_scale(device_scale);
    let font = test_font();
    let items = long_items();
    let widest = items
        .iter()
        .filter_map(|e| match e {
            MenuEntry::Item(item) => Some(painted_row_width(&font, item, 14.0)),
            MenuEntry::Separator => None,
        })
        .fold(0.0_f64, f64::max);
    let width = fitted_root_width(items, &font);
    reset_env();
    assert!(widest > FIT_MIN_W, "fixture rows must exceed the floor");
    assert!(
        (width - widest).abs() <= 1.0,
        "fitted popup at {device_scale}x is {width} px, widest row is {widest} px"
    );
}

#[test]
fn fitted_popup_width_matches_widest_row_at_1x() {
    assert_fits_widest_row(1.0);
}

#[test]
fn fitted_popup_width_matches_widest_row_at_2x() {
    assert_fits_widest_row(2.0);
}

#[test]
fn fixed_width_stays_the_default() {
    let _guard = profile_test_lock();
    reset_env();
    let mut popup = PopupMenu::new(long_items());
    popup.set_measure_font(test_font(), 14.0);
    popup.open_at(Point::new(20.0, 500.0));
    let layouts = popup.state.layouts(&popup.items, Size::new(1200.0, 600.0));
    assert_eq!(layouts[0].rect.width, MENU_W);
}

#[test]
fn fitted_popup_of_short_rows_keeps_the_minimum_width() {
    let _guard = profile_test_lock();
    reset_env();
    let items = vec![MenuItem::action("Cut", "cut").into()];
    assert_eq!(fitted_root_width(items, &test_font()), FIT_MIN_W);
}

fn short_items() -> Vec<MenuEntry> {
    vec![
        MenuItem::action("Cut", "cut").into(),
        MenuEntry::Separator,
        MenuItem::action("Copy", "copy").into(),
        MenuItem::submenu("Snap", vec![MenuItem::action("1", "one").into()]).into(),
    ]
}

/// With the floor dropped, a popup of short rows is exactly its widest row,
/// and the same floor applies to its submenu.
fn assert_zero_min_fits_short_rows(device_scale: f64) {
    let _guard = profile_test_lock();
    reset_env();
    crate::device_scale::set_device_scale(device_scale);
    let font = test_font();
    let items = short_items();
    let widest = items
        .iter()
        .filter_map(|e| match e {
            MenuEntry::Item(item) => Some(painted_row_width(&font, item, 14.0)),
            MenuEntry::Separator => None,
        })
        .fold(0.0_f64, f64::max);
    let leaf = painted_row_width(&font, &MenuItem::action("1", "one"), 14.0);
    let mut popup = PopupMenu::new(items)
        .with_width(MenuWidth::FitContent)
        .with_min_width(0.0);
    popup.set_measure_font(Arc::clone(&font), 14.0);
    popup.open_at(Point::new(20.0, 500.0));
    popup.state.open_path = vec![3];
    let layouts = popup.state.layouts(&popup.items, Size::new(1200.0, 600.0));
    reset_env();
    assert!(widest < FIT_MIN_W, "fixture rows must be under the floor");
    let root = layouts[0].rect.width;
    assert!(
        (root - widest).abs() <= 1.0,
        "unfloored popup at {device_scale}x is {root} px, widest row is {widest} px"
    );
    let sub = layouts[1].rect.width;
    assert!(
        (sub - leaf).abs() <= 1.0,
        "unfloored submenu at {device_scale}x is {sub} px, its row is {leaf} px"
    );
}

#[test]
fn zero_min_width_popup_matches_widest_short_row_at_1x() {
    assert_zero_min_fits_short_rows(1.0);
}

#[test]
fn zero_min_width_popup_matches_widest_short_row_at_2x() {
    assert_zero_min_fits_short_rows(2.0);
}

#[test]
fn min_width_defaults_to_the_fit_floor() {
    let _guard = profile_test_lock();
    reset_env();
    assert_eq!(PopupMenu::new(Vec::new()).state.min_width(), FIT_MIN_W);
    assert_eq!(fitted_root_width(short_items(), &test_font()), FIT_MIN_W);
}

#[test]
fn custom_min_width_floors_the_popup_and_bar_forwards_it() {
    let _guard = profile_test_lock();
    reset_env();
    let mut popup = PopupMenu::new(short_items())
        .with_width(MenuWidth::FitContent)
        .with_min_width(120.0);
    popup.set_measure_font(test_font(), 14.0);
    popup.open_at(Point::new(20.0, 500.0));
    let width = popup.state.layouts(&popup.items, Size::new(1200.0, 600.0))[0]
        .rect
        .width;
    assert_eq!(width, 120.0);
    let bar = MenuBar::new(test_font(), vec![], |_| {}).with_menu_min_width(0.0);
    assert_eq!(bar.popup.state.min_width(), 0.0);
}

#[test]
fn fitted_submenu_sizes_to_its_own_rows() {
    let _guard = profile_test_lock();
    reset_env();
    let font = test_font();
    let leaf = MenuItem::action("An Even Longer Submenu Leaf Row Label Here", "leaf");
    let expected = painted_row_width(&font, &leaf, 14.0);
    let items = vec![
        MenuItem::action("Short", "short").into(),
        MenuItem::submenu("More", vec![leaf.into()]).into(),
    ];
    let mut popup = PopupMenu::new(items).with_width(MenuWidth::FitContent);
    popup.set_measure_font(Arc::clone(&font), 14.0);
    popup.open_at(Point::new(20.0, 500.0));
    popup.state.open_path = vec![1];
    let layouts = popup.state.layouts(&popup.items, Size::new(1200.0, 600.0));
    assert_eq!(
        layouts[0].rect.width, FIT_MIN_W,
        "root holds only short rows"
    );
    assert!((layouts[1].rect.width - expected).abs() <= 1.0);
    // The cascade starts at the fitted root's right edge.
    assert_eq!(layouts[1].rect.x, layouts[0].rect.x + FIT_MIN_W - 2.0);
}

/// A menu bar's fitted popup hit-tests at its fitted width: a press past the
/// fitted right edge (but inside the old fixed width) is outside the menu.
#[test]
fn menu_bar_fitted_popup_hit_tests_at_fitted_width() {
    let _guard = profile_test_lock();
    reset_env();
    crate::widget::set_current_viewport(Size::new(800.0, 600.0));
    let mut bar = MenuBar::new(
        test_font(),
        vec![TopMenu::new(
            "File",
            vec![MenuItem::action("New", "file.new").into()],
        )],
        |_| {},
    )
    .with_menu_width(MenuWidth::FitContent);
    bar.layout(Size::new(300.0, BAR_H));
    bar.on_event(&Event::MouseDown {
        pos: Point::new(8.0, 8.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    bar.on_event(&Event::MouseUp {
        pos: Point::new(8.0, 8.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(bar.popup.is_open());
    let viewport = Size::new(800.0, 600.0);
    let rect = bar.popup.state.layouts(&bar.popup.items, viewport)[0].rect;
    assert_eq!(rect.width, FIT_MIN_W);
    let past_edge = Point::new(rect.x + rect.width + 20.0, rect.y + rect.height * 0.5);
    assert!(past_edge.x < rect.x + MENU_W, "inside the fixed width");
    assert!(!bar.popup.body_contains(past_edge, viewport));
    let r = bar.on_event(&Event::MouseDown {
        pos: past_edge,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert_eq!(r, EventResult::Consumed);
    assert!(
        !bar.popup.is_open(),
        "a press outside the fitted panel closes"
    );
}

/// Count the pixels in column `x` that differ from white.
fn column_ink(fb: &Framebuffer, x: u32) -> u32 {
    let w = fb.width();
    let px = fb.pixels();
    (0..fb.height())
        .filter(|&y| {
            let i = ((y * w + x) * 4) as usize;
            px[i] < 250 || px[i + 1] < 250 || px[i + 2] < 250
        })
        .count() as u32
}

/// The bar fills the height it was laid out at even when the input profile
/// flips to touch (growing `effective_metrics().bar_h` to 44) between layout
/// and paint.
#[test]
fn menu_bar_paints_layout_height_when_profile_flips_before_paint() {
    let _guard = profile_test_lock();
    reset_env();
    crate::font_settings::set_lcd_enabled(false);
    let mut bar = MenuBar::new(test_font(), vec![TopMenu::new("File", vec![])], |_| {});
    let used = bar.layout(Size::new(300.0, 60.0));
    assert_eq!(used.height, BAR_H);
    bar.set_bounds(Rect::new(0.0, 0.0, 300.0, used.height));

    set_input_profile(InputProfile::MobileIOS);
    assert_eq!(super::super::geometry::menu_bar_height(), TOUCH_MIN);
    let mut fb = Framebuffer::new(300, 60);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        bar.paint(&mut ctx);
    }
    reset_env();
    crate::font_settings::clear_lcd_enabled_override();
    assert_eq!(column_ink(&fb, 280), BAR_H as u32);
}
