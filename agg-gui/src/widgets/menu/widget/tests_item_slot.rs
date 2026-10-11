//! Per-row height and icon slot (`MenuItem::row_height`, `MenuIconSlot`).
//!
//! A row with its own height stacks at that height in the popup layout, so
//! hit-testing (and with it hover, activation and the row tooltip, which
//! hit-tests the same layout) covers the whole tall row; keyboard navigation
//! still selects and activates it.  A row with its own icon slot paints its
//! image at the slot's size centred in the slot's gutter, snapped to whole
//! device pixels, and starts its label at the slot's `label_x`, which a
//! fitted popup measures.  Rows that set neither keep the style's metrics
//! (every other menu test).

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, Key, Modifiers, MouseButton};
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::icon_image::IconImage;
use crate::text::Font;

use super::super::fit_width::FitMeasure;
use super::super::geometry::{hit_test, MenuHit, TOUCH_MIN};
use super::super::model::{MenuEntry, MenuIconSlot, MenuItem};
use super::super::paint::MenuStyle;
use super::super::state::MenuResponse;
use super::PopupMenu;

const VIEWPORT: Size = Size {
    width: 400.0,
    height: 400.0,
};

/// A 45-unit thumbnail in a 52-unit gutter past a 3-unit inset, its label 3
/// units past the gutter: the shape of a recent-files row.
const SLOT: MenuIconSlot = MenuIconSlot {
    gutter_x: 3.0,
    gutter_width: 52.0,
    image_size: Size {
        width: 45.0,
        height: 45.0,
    },
    label_x: 58.0,
    label_right: 15.0,
};

fn reset_env() {
    crate::input_profile::set_input_profile(crate::input_profile::InputProfile::Desktop);
    crate::touch_state::clear_last_touch_event_for_testing();
    crate::ux_scale::set_ux_scale(1.0);
    crate::device_scale::set_device_scale(1.0);
}

fn test_font() -> Arc<Font> {
    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn green_icon() -> IconImage {
    IconImage::from_svg_data(
        br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12">
            <rect width="12" height="12" fill="#00ff00"/></svg>"##,
        Size::new(12.0, 12.0),
    )
    .expect("icon svg")
}

fn tall_menu() -> PopupMenu {
    let mut menu = PopupMenu::new(vec![
        MenuEntry::Item(MenuItem::action("A", "a")),
        MenuEntry::Item(MenuItem::action("B", "b").row_height(52.0).tooltip("tip")),
        MenuEntry::Item(MenuItem::action("C", "c")),
    ]);
    menu.open_at(Point::new(20.0, 300.0));
    menu
}

fn row_rect(menu: &PopupMenu, row: usize) -> Rect {
    menu.state.layouts(&menu.items, VIEWPORT)[0].rows[row].rect
}

fn key(k: Key) -> Event {
    Event::KeyDown {
        key: k,
        modifiers: Modifiers::default(),
    }
}

#[test]
fn own_row_height_sizes_its_row_and_the_panel() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let menu = tall_menu();
    let layout = &menu.state.layouts(&menu.items, VIEWPORT)[0];
    let heights: Vec<f64> = layout.rows.iter().map(|r| r.rect.height).collect();
    assert_eq!(heights, vec![24.0, 52.0, 24.0]);
    assert_eq!(layout.rect.height, 100.0);
    assert_eq!(layout.rows[1].rect.y + 52.0, layout.rows[0].rect.y);
    assert_eq!(layout.rows[2].rect.y + 24.0, layout.rows[1].rect.y);
}

#[test]
fn the_whole_tall_row_hovers_activates_and_hit_tests_for_its_tooltip() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut menu = tall_menu();
    let b = row_rect(&menu, 1);
    // 27 units below the first row: inside the 52-unit row, where a
    // 24-unit row would already have given way to the next one.
    let near_top = Point::new(b.x + 40.0, row_rect(&menu, 0).y - 27.0);
    let layouts = menu.state.layouts(&menu.items, VIEWPORT);
    assert!(matches!(hit_test(&layouts, near_top), Some(MenuHit::Item(p)) if p == vec![1]));

    menu.handle_event(&Event::MouseMove { pos: near_top }, VIEWPORT);
    assert_eq!(menu.state.hover_path, Some(vec![1]));

    let near_bottom = Point::new(b.x + 40.0, b.y + 2.0);
    let (_, response) = menu.handle_event(
        &Event::MouseDown {
            pos: near_bottom,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
        VIEWPORT,
    );
    assert_eq!(response, MenuResponse::Action("b".to_string()));
}

#[test]
fn keyboard_selects_and_activates_a_tall_row() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut menu = tall_menu();
    menu.handle_event(&key(Key::ArrowDown), VIEWPORT);
    menu.handle_event(&key(Key::ArrowDown), VIEWPORT);
    assert_eq!(menu.state.hover_path, Some(vec![1]));
    let (_, response) = menu.handle_event(&key(Key::Enter), VIEWPORT);
    assert_eq!(response, MenuResponse::Action("b".to_string()));
}

#[test]
fn own_row_height_is_still_touch_floored() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    crate::touch_state::note_touch_event();
    let mut menu = PopupMenu::new(vec![
        MenuEntry::Item(MenuItem::action("Short", "s").row_height(30.0)),
        MenuEntry::Item(MenuItem::action("Tall", "t").row_height(60.0)),
    ]);
    menu.open_at(Point::new(20.0, 300.0));
    let heights: Vec<f64> = menu.state.layouts(&menu.items, VIEWPORT)[0]
        .rows
        .iter()
        .map(|r| r.rect.height)
        .collect();
    assert_eq!(heights, vec![TOUCH_MIN, 60.0]);
    reset_env();
}

/// Paints a one-row menu whose row has the recent-files slot and returns
/// the row rect and the framebuffer.
fn paint_slot_row() -> (Rect, Framebuffer) {
    let item = MenuItem::action("Design", "d")
        .image_icon(green_icon())
        .row_height(52.0)
        .icon_slot(SLOT);
    let mut menu = PopupMenu::new(vec![item.into()]);
    menu.open_at(Point::new(20.0, 300.0));
    let row = row_rect(&menu, 0);
    let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        menu.paint(&mut ctx, test_font(), 14.0, VIEWPORT);
    }
    (row, fb)
}

fn is_green(fb: &Framebuffer, x: f64, y: f64) -> bool {
    let i = ((y as u32 * fb.width() + x as u32) * 4) as usize;
    let p = &fb.pixels()[i..i + 3];
    p[0] < 40 && p[1] > 215 && p[2] < 40
}

#[test]
fn icon_slot_centres_the_image_at_its_size_in_the_gutter() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let (row, fb) = paint_slot_row();
    // x: 3 + (52 - 45) / 2 = 6.5, snapped down to 6; y: (52 - 45) / 2 = 3.5 -> 3.
    let (left, bottom) = (row.x + 6.0, row.y + 3.0);
    let mid_y = bottom + 22.5;
    let mid_x = left + 22.5;
    assert!(is_green(&fb, left + 0.5, mid_y), "first image column");
    assert!(is_green(&fb, left + 44.5, mid_y), "45th image column");
    assert!(!is_green(&fb, left - 0.5, mid_y), "left of the image");
    assert!(!is_green(&fb, left + 45.5, mid_y), "right of the image");
    assert!(is_green(&fb, mid_x, bottom + 0.5), "bottom image row");
    assert!(is_green(&fb, mid_x, bottom + 44.5), "top image row");
    assert!(!is_green(&fb, mid_x, bottom - 0.5), "below the image");
    assert!(!is_green(&fb, mid_x, bottom + 45.5), "above the image");
}

#[test]
fn icon_slot_moves_the_label_and_widens_a_fitted_row() {
    let style = MenuStyle::default();
    let measure = FitMeasure::new(test_font(), 14.0, &style);
    let plain = MenuItem::action("Design", "d");
    let slotted = plain.clone().icon_slot(SLOT);
    let grown = measure.row_natural_width(&slotted) - measure.row_natural_width(&plain);
    let expected = (SLOT.label_x - style.label_x) + (SLOT.label_right - style.shortcut_right);
    assert!(
        (grown - expected).abs() < 1e-9,
        "grew {grown}, expected {expected}"
    );
    assert_eq!(
        super::super::style::row_label_x(style.label_x, None, &slotted),
        SLOT.label_x
    );
}
