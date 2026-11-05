//! A row's second line (`MenuItem::description`).
//!
//! A described row is taller by `geometry::description_band`, on the mouse
//! and the touch layout alike, and stays one row: hit-testing (and with it
//! hover and activation) and keyboard selection cover both lines.  Its label
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

use super::super::fit_width::FitMeasure;
use super::super::geometry::{
    description_band, effective_metrics, hit_test, metrics_for, MenuHit, ROW_H, TOUCH_MIN,
};
use super::super::model::{MenuEntry, MenuItem};
use super::super::paint::MenuStyle;
use super::super::state::MenuResponse;
use super::PopupMenu;

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
