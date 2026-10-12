//! Tests for the agg-sharp `PopupMenu` chrome options of
//! [`super::super::style::MenuStyle`]: the separator's geometry and colour
//! ([`MenuSeparatorStyle`], agg-sharp `PopupMenu.CreateSeparator`), the
//! filled submenu triangle ([`SubmenuArrow::FilledTriangle`],
//! `SubMenuItemButton.OnDraw`) and the window margin
//! ([`MenuStyle::window_margin`]; agg-sharp sits flush).  Each option is
//! checked on layout geometry or on pixels painted into a real framebuffer,
//! and its default against agg-gui's historical look.

use std::sync::Arc;

use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::input_profile::{profile_test_lock, set_input_profile, InputProfile};
use crate::text::Font;

use super::super::geometry::{PopupLayout, MENU_W, ROW_H, SEP_H};
use super::super::model::{MenuEntry, MenuItem};
use super::super::paint::{submenu_triangle_points, MenuStyle};
use super::super::style::{reset_menu_style, MenuSeparatorStyle, SubmenuArrow};
use super::PopupMenu;

const VIEWPORT: Size = Size {
    width: 400.0,
    height: 400.0,
};
/// Panel fill: mid grey, so both a light and a dark theme's text colour
/// stand out from it.
const BG: [u8; 3] = [0x80, 0x80, 0x80];

fn test_font() -> Arc<Font> {
    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn reset_env() {
    set_input_profile(InputProfile::Desktop);
    crate::touch_state::clear_last_touch_event_for_testing();
    crate::ux_scale::set_ux_scale(1.0);
    crate::device_scale::set_device_scale(1.0);
    reset_menu_style();
}

/// A flat, unoutlined panel so painted chrome is the only thing on it.
fn flat(style: MenuStyle) -> MenuStyle {
    MenuStyle {
        background: Some(Color::from_rgb8(BG[0], BG[1], BG[2])),
        border_width: 0.0,
        shadow: false,
        ..style
    }
}

/// agg-sharp's `CreateSeparator`: a 1 px line with `Margin(8, 1)`.
fn agg_sharp_separator(color: Option<Color>) -> MenuSeparatorStyle {
    MenuSeparatorStyle {
        line_height: 1.0,
        margin_y: 1.0,
        inset_x: 8.0,
        color,
    }
}

fn items() -> Vec<MenuEntry> {
    vec![
        MenuItem::action("Cut", "cut").into(),
        MenuEntry::Separator,
        MenuItem::submenu("More", vec![MenuItem::action("Paste", "paste").into()]).into(),
    ]
}

/// The popup opened at `anchor` in `VIEWPORT`, its root layout, and its
/// pixels painted over black.
fn paint_at(style: MenuStyle, anchor: Point) -> (Framebuffer, PopupLayout) {
    let mut menu = PopupMenu::new(items()).with_style(style);
    menu.set_measure_font(test_font(), 14.0);
    menu.open_at(anchor);
    let layout = menu.state.layouts(&menu.items, VIEWPORT)[0].clone();
    let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        menu.paint(&mut ctx, test_font(), 14.0, VIEWPORT);
    }
    (fb, layout)
}

fn paint(style: MenuStyle) -> (Framebuffer, PopupLayout) {
    paint_at(style, Point::new(20.0, 380.0))
}

fn px(fb: &Framebuffer, x: f64, y: f64) -> [u8; 3] {
    let i = ((y as u32 * fb.width() + x as u32) * 4) as usize;
    [fb.pixels()[i], fb.pixels()[i + 1], fb.pixels()[i + 2]]
}

#[test]
fn rust_only_chrome_defaults_are_the_historical_look() {
    let s = MenuStyle::default();
    assert_eq!(s.separator.row_height(), SEP_H);
    assert_eq!(s.separator.line_height, 1.0);
    assert_eq!(s.separator.inset_x, 8.0);
    assert_eq!(s.separator.color, None);
    assert_eq!(s.submenu_arrow, SubmenuArrow::Chevron);
    assert_eq!(s.window_margin, 4.0);
}

#[test]
fn rust_only_separator_style_sets_the_separator_row_height() {
    let _guard = profile_test_lock();
    reset_env();
    let style = MenuStyle {
        separator: agg_sharp_separator(None),
        ..MenuStyle::default()
    };
    let (_, layout) = paint(flat(style));
    assert_eq!(layout.rows[0].rect.height, ROW_H);
    assert_eq!(
        layout.rows[1].rect.height, 3.0,
        "1 px line + 2 x 1 px margin"
    );
    assert_eq!(layout.rows[2].rect.height, ROW_H);
    assert_eq!(layout.rect.height, ROW_H * 2.0 + 3.0);
    // The default keeps the 7 px row.
    let (_, layout) = paint(flat(MenuStyle::default()));
    assert_eq!(layout.rows[1].rect.height, SEP_H);
}

#[test]
fn rust_only_separator_paints_its_line_with_its_margins_and_insets() {
    let _guard = profile_test_lock();
    reset_env();
    let red = Color::from_rgb8(0xff, 0, 0);
    let style = MenuStyle {
        separator: agg_sharp_separator(Some(red)),
        ..MenuStyle::default()
    };
    let (fb, layout) = paint(flat(style));
    let sep = layout.rows[1].rect;
    let mid_x = sep.x + sep.width * 0.5;
    // Y-up: the row's bottom margin, the line, the top margin.
    assert_eq!(px(&fb, mid_x, sep.y + 0.5), BG, "bottom margin");
    assert_eq!(px(&fb, mid_x, sep.y + 1.5), [0xff, 0, 0], "the line");
    assert_eq!(px(&fb, mid_x, sep.y + 2.5), BG, "top margin");
    // Inset 8 px from both sides.
    assert_eq!(px(&fb, sep.x + 7.5, sep.y + 1.5), BG, "left inset");
    assert_eq!(px(&fb, sep.x + 8.5, sep.y + 1.5), [0xff, 0, 0]);
    let right = sep.x + sep.width;
    assert_eq!(px(&fb, right - 7.5, sep.y + 1.5), BG, "right inset");
    assert_eq!(px(&fb, right - 8.5, sep.y + 1.5), [0xff, 0, 0]);
}

#[test]
fn rust_only_separator_colour_replaces_the_translucent_stroke() {
    let _guard = profile_test_lock();
    reset_env();
    let (fb, layout) = paint(flat(MenuStyle::default()));
    let sep = layout.rows[1].rect;
    // The default line sits mid-row (3 px margins) in a translucent stroke.
    let default_line = px(&fb, sep.x + sep.width * 0.5, sep.y + 3.5);
    assert_ne!(default_line, BG, "the default separator paints");
    let green = Color::from_rgb8(0, 0xff, 0);
    let style = MenuStyle {
        separator: MenuSeparatorStyle {
            color: Some(green),
            ..MenuSeparatorStyle::default()
        },
        ..MenuStyle::default()
    };
    let (fb, layout) = paint(flat(style));
    let sep = layout.rows[1].rect;
    assert_eq!(px(&fb, sep.x + sep.width * 0.5, sep.y + 3.5), [0, 0xff, 0]);
}

#[test]
fn rust_only_submenu_triangle_matches_agg_sharp_geometry() {
    // `SubMenuItemButton.OnDraw`: x = Right - Height / 2,
    // y = Height / 2 + 2, apex 3 right of x, base 3 left spanning +-5.
    let row = Rect::new(10.0, 100.0, 200.0, 28.0);
    let [apex, top, bottom] = submenu_triangle_points(row);
    assert_eq!(apex, (199.0, 116.0));
    assert_eq!(top, (193.0, 121.0));
    assert_eq!(bottom, (193.0, 111.0));
}

#[test]
fn rust_only_filled_triangle_option_fills_the_submenu_arrow() {
    let _guard = profile_test_lock();
    reset_env();
    let triangle = MenuStyle {
        submenu_arrow: SubmenuArrow::FilledTriangle,
        ..MenuStyle::default()
    };
    let (fb, layout) = paint(flat(triangle));
    let row = layout.rows[2].rect;
    let [apex, top, _] = submenu_triangle_points(row);
    // The pixel just inside the triangle's base, 3-4 px below its tip's
    // height: wholly covered by the triangle (whose lower edge is 4.17 px
    // below the tip there), while the stroked chevron's lower arm ends 2 px
    // further down and leaves it bare.
    let (x, y) = (top.0 + 0.5, apex.1 - 3.5);
    assert!(x > top.0, "probe lies inside the triangle");
    let filled = px(&fb, x, y);
    assert!(
        filled.iter().zip(BG).any(|(a, b)| a.abs_diff(b) > 60),
        "the triangle is filled in the row's text colour, got {filled:?}"
    );
    let (fb, _) = paint(flat(MenuStyle::default()));
    assert_eq!(px(&fb, x, y), BG, "the chevron does not fill it");
}

#[test]
fn rust_only_window_margin_zero_lets_the_panel_sit_flush() {
    let _guard = profile_test_lock();
    reset_env();
    // Opened past the bottom-right corner, the panel clamps into the window.
    let corner = Point::new(VIEWPORT.width - 10.0, 10.0);
    let (_, default) = paint_at(flat(MenuStyle::default()), corner);
    assert_eq!(default.rect.x, VIEWPORT.width - MENU_W - 4.0);
    assert_eq!(default.rect.y, 4.0);
    let flush = MenuStyle {
        window_margin: 0.0,
        ..MenuStyle::default()
    };
    let (fb, layout) = paint_at(flat(flush), corner);
    assert_eq!(layout.rect.x, VIEWPORT.width - MENU_W);
    assert_eq!(layout.rect.y, 0.0);
    // The panel's fill reaches the window's last column and bottom row.
    let mid_y = layout.rect.y + layout.rect.height * 0.5;
    assert_eq!(px(&fb, VIEWPORT.width - 0.5, mid_y), BG);
    assert_eq!(px(&fb, VIEWPORT.width - MENU_W * 0.5, 0.5), BG);
}
