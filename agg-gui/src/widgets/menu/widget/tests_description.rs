//! A row's second line (`MenuItem::description`).
//!
//! A described row is taller by its description's band, on the mouse and
//! the touch layout alike, and stays one row: hit-testing (and with it hover
//! and activation) and keyboard selection cover all of it.  Before a font is
//! known the band is one line (`geometry::description_band`); with the font
//! the description wraps to the panel's width, never eliding, and the band
//! holds every line.  Its label
//! keeps a one-line row's place in the line above the band, the description
//! paints under it in the theme's dimmed text, and a fitted popup is wide
//! enough for the description.  Rows without one keep their exact height
//! (every other menu test, and the plain rows here).

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, Key, Modifiers, MouseButton};
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::theme::Visuals;

use super::super::fit_width::{description_label, FitMeasure, MenuWidth, DESCRIPTION_MEASURE_EMS};
use super::super::geometry::{
    description_band, effective_metrics, hit_test, metrics_for, MenuHit, DESCRIPTION_SCALE, ROW_H,
    TOUCH_MIN,
};
use super::super::model::{MenuEntry, MenuItem};
use super::super::paint::MenuStyle;
use super::super::state::MenuResponse;
use super::PopupMenu;
use crate::widget::Widget;

const VIEWPORT: Size = Size {
    width: 400.0,
    height: 400.0,
};

const WORDS: &str = "Takes longer";

fn reset_env() {
    crate::input_profile::set_input_profile(crate::input_profile::InputProfile::Desktop);
    crate::touch_state::clear_last_touch_event_for_testing();
    crate::touch_emulation::long_press::set_last_press_was_touch_for_testing(false);
    crate::ux_scale::set_ux_scale(1.0);
    crate::device_scale::set_device_scale(1.0);
}

fn test_font() -> Arc<Font> {
    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn described_menu() -> PopupMenu {
    let mut menu = PopupMenu::new(vec![
        MenuEntry::Item(MenuItem::action("A", "a")),
        MenuEntry::Item(
            MenuItem::action("Best", "b")
                .radio(false)
                .description(WORDS),
        ),
        MenuEntry::Item(MenuItem::action("C", "c")),
    ]);
    menu.open_at(Point::new(20.0, 300.0));
    menu
}

fn key(k: Key) -> Event {
    Event::KeyDown {
        key: k,
        modifiers: Modifiers::default(),
    }
}

#[test]
fn a_described_row_grows_by_the_band_and_plain_rows_keep_their_height() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let menu = described_menu();
    let band = description_band(&metrics_for(false));
    assert!(band > 0.0);
    let layout = &menu.state.layouts(&menu.items, VIEWPORT)[0];
    let heights: Vec<f64> = layout.rows.iter().map(|r| r.rect.height).collect();
    assert_eq!(heights, vec![ROW_H, ROW_H + band, ROW_H]);
    assert_eq!(layout.rect.height, 3.0 * ROW_H + band);
    let row = &layout.rows[1];
    // The label's line is a one-line row at the top; the band is under it.
    assert_eq!(row.line_rect().height, ROW_H);
    assert_eq!(row.line_rect().y, row.rect.y + band);
    let words = row.description_rect();
    assert!(words.y >= row.rect.y && words.y + words.height <= row.line_rect().y);
    assert_eq!(layout.rows[0].description_height, 0.0);
    assert_eq!(layout.rows[0].line_rect(), layout.rows[0].rect);
}

#[test]
fn the_whole_described_row_hovers_activates_and_takes_the_keyboard() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut menu = described_menu();
    let row = menu.state.layouts(&menu.items, VIEWPORT)[0].rows[1].clone();
    let layouts = menu.state.layouts(&menu.items, VIEWPORT);
    // In the description's band, below where a one-line row would end.
    let in_band = Point::new(row.rect.x + 40.0, row.rect.y + 2.0);
    assert!(in_band.y < row.line_rect().y);
    assert!(matches!(hit_test(&layouts, in_band), Some(MenuHit::Item(p)) if p == vec![1]));
    menu.handle_event(&Event::MouseMove { pos: in_band }, VIEWPORT);
    assert_eq!(menu.state.hover_path, Some(vec![1]));
    let (_, response) = menu.handle_event(
        &Event::MouseDown {
            pos: in_band,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
        VIEWPORT,
    );
    assert_eq!(response, MenuResponse::Action("b".to_string()));

    let mut menu = described_menu();
    menu.handle_event(&key(Key::ArrowDown), VIEWPORT);
    menu.handle_event(&key(Key::ArrowDown), VIEWPORT);
    assert_eq!(menu.state.hover_path, Some(vec![1]));
    let (_, response) = menu.handle_event(&key(Key::Enter), VIEWPORT);
    assert_eq!(response, MenuResponse::Action("b".to_string()));
}

#[test]
fn a_described_row_opened_by_a_finger_is_touch_sized_with_its_band() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    crate::touch_emulation::long_press::set_last_press_was_touch_for_testing(true);
    let menu = described_menu();
    let band = description_band(&metrics_for(true));
    let heights: Vec<f64> = menu.state.layouts(&menu.items, VIEWPORT)[0]
        .rows
        .iter()
        .map(|r| r.rect.height)
        .collect();
    assert_eq!(heights, vec![TOUCH_MIN, TOUCH_MIN + band, TOUCH_MIN]);
    assert!(
        band >= description_band(&metrics_for(false)),
        "the band grows with the finger-sized text"
    );
    reset_env();
}

/// The brightest channel of the pixels inside `rect` (Y-up, the
/// framebuffer's bottom row first).
fn brightest(fb: &Framebuffer, rect: Rect) -> u8 {
    let mut most = 0;
    for y in rect.y.ceil() as u32..(rect.y + rect.height).floor() as u32 {
        for x in rect.x.ceil() as u32..(rect.x + rect.width).floor() as u32 {
            let i = ((y * fb.width() + x) * 4) as usize;
            most = most.max(fb.pixels()[i..i + 3].iter().copied().max().unwrap_or(0));
        }
    }
    most
}

#[test]
fn the_description_paints_under_the_label_in_the_dimmed_text_colour() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    crate::theme::set_visuals(Visuals::dark());
    let mut menu = described_menu();
    let row = menu.state.layouts(&menu.items, VIEWPORT)[0].rows[1].clone();
    let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        menu.paint(&mut ctx, test_font(), 14.0, VIEWPORT);
    }
    let label = brightest(&fb, row.line_rect());
    let words = brightest(&fb, row.description_rect());
    // Dark theme: text at 90% grey, dimmed text the same at half alpha, on
    // the popup's dark fill.
    assert!(
        label > 170,
        "the label is drawn in the text colour ({label})"
    );
    assert!(
        words > 50 && words < 160,
        "the description is drawn, dimmed ({words})"
    );
    let shown = menu
        .labels
        .row_description(0, 1)
        .expect("the description label");
    assert_eq!(shown.text_str(), WORDS);
    assert!(menu.labels.row_description(0, 0).is_none());
}

#[test]
fn a_fitted_popup_is_wide_enough_for_a_description() {
    let style = MenuStyle::default();
    let measure = FitMeasure::new(test_font(), 14.0, &style);
    let short = MenuItem::action("Best", "b");
    let described = short
        .clone()
        .description("Takes longer and uses your allowance faster.");
    assert!(
        measure.row_natural_width(&described) > measure.row_natural_width(&short) + 100.0,
        "the description widens its row"
    );
    let natural = measure.row_natural_width(&described);
    let width = measure.popup_width_with_min(&[described.into()], &effective_metrics(), 0.0);
    assert_eq!(width, natural.ceil(), "the popup fits the described row");
}

const LONG: &str = "Takes longer and uses your allowance faster, which is worth it for complex \
                    designs with many parts and careful fillets.";

/// A content-sized menu with a short row and a long described one, open in a
/// window `width` wide, measured with the font it paints with.
fn wrapping_menu(width: f64) -> (PopupMenu, Size) {
    let viewport = Size::new(width, 400.0);
    let mut menu = PopupMenu::new(vec![
        MenuEntry::Item(MenuItem::action("Standard", "s")),
        MenuEntry::Item(MenuItem::action("Best", "b").radio(false).description(LONG)),
    ]);
    menu.set_width(MenuWidth::FitContent);
    menu.set_measure_font(test_font(), 14.0);
    menu.open_at(Point::new(4.0, 300.0));
    (menu, viewport)
}

#[test]
fn a_long_description_in_a_narrow_window_wraps_and_its_row_grows_by_its_lines() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    crate::theme::set_visuals(Visuals::dark());
    let (mut menu, viewport) = wrapping_menu(220.0);
    let layout = menu.state.layouts(&menu.items, viewport)[0].clone();
    assert!(
        layout.rect.width <= 220.0,
        "the panel keeps inside the window: {:?}",
        layout.rect
    );
    let row = layout.rows[1].clone();
    let style = MenuStyle::default();
    let item = MenuItem::action("Best", "b").radio(false).description(LONG);
    let room = row.rect.width
        - super::super::style::row_label_x(style.label_x, style.selection_column, &item)
        - super::super::style::row_label_right(style.shortcut_right, &item);
    let lines =
        crate::widgets::label::wrap_text(&test_font(), LONG, 14.0 * DESCRIPTION_SCALE, room);
    assert!(lines.len() >= 3, "the words wrap: {lines:?}");
    // The band holds every line: N one-line heights, not one.
    let one_line = description_label("Takes", &test_font(), 14.0).measure_min_height(1000.0);
    let text = row.description_rect().height;
    assert!(
        text >= lines.len() as f64 * one_line - 0.5,
        "{} lines of {one_line} fit in {text}",
        lines.len()
    );
    assert_eq!(
        row.rect.height,
        row.line_rect().height + row.description_height,
        "the row is its label's line and the band"
    );
    // The whole taller row hit-tests: its bottom edge, under the last line.
    let layouts = menu.state.layouts(&menu.items, viewport);
    let bottom = Point::new(row.rect.x + 30.0, row.rect.y + 1.0);
    assert!(matches!(hit_test(&layouts, bottom), Some(MenuHit::Item(p)) if p == vec![1]));

    // Painted: every line band of the description has ink (it wrapped onto
    // them rather than ending in an ellipsis on the first).
    let mut fb = Framebuffer::new(viewport.width as u32, viewport.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        menu.paint(&mut ctx, test_font(), 14.0, viewport);
    }
    let words = row.description_rect();
    let n = lines.len();
    let line_h = words.height / n as f64;
    for i in 0..n {
        let stripe = Rect::new(
            words.x,
            words.y + words.height - (i as f64 + 1.0) * line_h,
            words.width,
            line_h,
        );
        assert!(
            brightest(&fb, stripe) > 50,
            "line {} of {n} is drawn",
            i + 1
        );
    }
    assert_eq!(
        menu.labels.row_description(0, 1).map(|l| l.text_str()),
        Some(LONG),
        "the whole description, never cut"
    );
}

#[test]
fn a_wide_window_caps_a_long_description_at_the_measure_and_touch_still_floors_the_label_line() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let (menu, viewport) = wrapping_menu(1200.0);
    let layout = menu.state.layouts(&menu.items, viewport)[0].clone();
    let wide = layout.rows[1].clone();
    let one_line = description_label(LONG, &test_font(), 14.0).measure_min_height(1.0e6);
    // The long words would make a ~1000 px row: the panel stops at the
    // reading measure and they wrap inside it, the window's room unused.
    let room = description_room(&menu, &wide, LONG);
    assert!(
        room <= DESCRIPTION_MEASURE_EMS * 14.0 + 1.0,
        "the description's room ({room}) is capped at the measure"
    );
    assert!(
        wide.description_rect().height >= 2.0 * one_line,
        "and the words wrap inside it: {:?}",
        wide.description_rect()
    );
    // A short description still sits on one line, widening the panel past
    // its short labels.
    let mut short = PopupMenu::new(vec![MenuEntry::Item(
        MenuItem::action("A", "a").description("Fast, great for most designs."),
    )]);
    short.set_width(MenuWidth::FitContent);
    short.set_measure_font(test_font(), 14.0);
    short.open_at(Point::new(4.0, 300.0));
    let row = short.state.layouts(&short.items, viewport)[0].rows[0].clone();
    assert!(row.description_rect().height < 2.0 * one_line, "{row:?}");

    crate::touch_emulation::long_press::set_last_press_was_touch_for_testing(true);
    let (menu, viewport) = wrapping_menu(220.0);
    let rows = menu.state.layouts(&menu.items, viewport)[0].rows.clone();
    assert_eq!(rows[0].rect.height, TOUCH_MIN);
    assert_eq!(rows[1].line_rect().height, TOUCH_MIN);
    assert!(
        rows[1].description_height > 2.0 * one_line,
        "it wraps on touch too"
    );
    reset_env();
}

/// The room a row `row` gives its description under the installed style.
fn description_room(menu: &PopupMenu, row: &super::super::geometry::RowLayout, text: &str) -> f64 {
    let item = MenuItem::action("Best", "b").description(text);
    let style = &menu.style;
    row.rect.width
        - super::super::style::row_label_x(style.label_x, style.selection_column, &item)
        - super::super::style::row_label_right(style.shortcut_right, &item)
}

#[test]
fn a_half_pixel_narrower_popup_rewraps_its_description_as_its_row_was_sized() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    const TWO_WORDS: &str = "alpha beta";
    let font = test_font();
    let size = 14.0 * DESCRIPTION_SCALE;
    let words_w = crate::text::measure_advance(&font, TWO_WORDS, size);
    let mut menu = PopupMenu::new(vec![MenuEntry::Item(
        MenuItem::action("Best", "b").description(TWO_WORDS),
    )]);
    menu.set_measure_font(Arc::clone(&font), 14.0);
    menu.open_at(Point::new(0.0, 300.0));
    // A window narrow enough that the panel is the window less its margins,
    // then moved so the room is a quarter pixel either side of the words.
    let probe = Size::new(120.0, 400.0);
    let row = menu.state.layouts(&menu.items, probe)[0].rows[0].clone();
    let room = description_room(&menu, &row, TWO_WORDS);
    let fits = Size::new(probe.width + (words_w + 0.25 - room), 400.0);
    let breaks = Size::new(fits.width - 0.5, 400.0);

    let paint = |menu: &mut PopupMenu, viewport: Size| {
        let mut fb = Framebuffer::new(viewport.width.ceil() as u32, viewport.height as u32);
        let mut ctx = GfxCtx::new(&mut fb);
        menu.paint(&mut ctx, Arc::clone(&font), 14.0, viewport);
    };
    paint(&mut menu, fits);
    let one = menu
        .labels
        .row_description(0, 0)
        .expect("label")
        .bounds()
        .height;
    let band_one = menu.state.layouts(&menu.items, fits)[0].rows[0].description_rect();
    paint(&mut menu, breaks);
    let two = menu
        .labels
        .row_description(0, 0)
        .expect("label")
        .bounds()
        .height;
    let band_two = menu.state.layouts(&menu.items, breaks)[0].rows[0].description_rect();
    assert!(
        band_two.height > band_one.height,
        "the row grew for the second line"
    );
    assert!(
        two > 1.5 * one,
        "the painted words wrap to two lines as the row was sized ({one} -> {two})"
    );
    assert!(
        two <= band_two.height + 0.5,
        "and fit the band ({two} in {band_two:?})"
    );
}

#[test]
fn a_plain_row_fits_to_exactly_its_label_shortcut_and_padding() {
    let style = MenuStyle::default();
    let font = test_font();
    let measure = FitMeasure::new(Arc::clone(&font), 14.0, &style);
    let plain = MenuItem::action("Select All", "s").shortcut("Ctrl+A");
    let platform = crate::platform::current_platform();
    let shortcut = plain
        .shortcut_text_formatted(platform, &font, style.shortcut_format)
        .expect("a shortcut");
    let label_w = crate::text::measure_advance(&font, "Select All", 14.0);
    let shortcut_w = super::super::fit_width::SHORTCUT_GAP
        + crate::text::measure_advance(&font, &shortcut, 14.0);
    let expected = super::super::style::row_label_x(style.label_x, None, &plain)
        + label_w
        + shortcut_w
        + super::super::style::row_label_right(style.shortcut_right, &plain);
    assert_eq!(
        measure.row_natural_width(&plain).to_bits(),
        expected.to_bits()
    );
}
