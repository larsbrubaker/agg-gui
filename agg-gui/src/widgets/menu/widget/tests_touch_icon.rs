//! A popup row's Font Awesome icon glyph (`MenuItem::icon`) sits vertically
//! centred with the row's label at every row height, not only the 24 px
//! desktop row.
//!
//! Reported from the web demo on an Android phone (Chrome, ~412 CSS px wide,
//! DPR ~2.6): in the open "Demos" menu each row's icon hung about half a line
//! below its label.  The rows there are finger-sized (floored at the 44 px
//! touch minimum by `geometry::metrics_for`), while `popup_paint.rs` placed
//! the glyph's baseline a fixed 7 px above the row's bottom, which only
//! centres it in a 24 px row.  The label, a `Label` centred on the row, rose
//! with the row; the icon stayed at the bottom.
//!
//! These tests paint a popup through the production `PopupMenu::paint` at the
//! phone's input profile, UX scale and device scale and compare the vertical
//! centre of the icon's ink with the label's.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::input_profile::InputProfile;
use crate::text::Font;

use super::super::model::{MenuEntry, MenuItem};
use super::super::paint::MenuStyle;
use super::PopupMenu;

/// The demo's popup font size (`demo-ui/src/top_bar.rs` `with_font_size`).
const FONT_SIZE: f64 = 13.0;
/// Font Awesome `fa-th-large`, the demo's Demos-group icon.
const ICON: char = '\u{F009}';

fn icon_font() -> Arc<Font> {
    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(crate::fonts::with_standard_fallbacks(
        Font::from_slice(FONT_BYTES).expect("font"),
    ))
}

fn demos_menu() -> Vec<MenuEntry> {
    ["Widgets", "Tests", "Tools"]
        .iter()
        .map(|name| {
            MenuItem::submenu(*name, vec![MenuItem::action("Demo", "demo").into()])
                .icon(ICON)
                .into()
        })
        .collect()
}

/// Vertical centre (device px, Y-up like the row) of the ink between
/// `x0..x1` (device px) over the row's span, or `None` with no ink there.
fn ink_centre(fb: &Framebuffer, bg: [u8; 3], x0: u32, x1: u32, y0: u32, y1: u32) -> Option<f64> {
    let mut lo = None;
    let mut hi = None;
    for y in y0..y1 {
        let inked = (x0..x1).any(|x| {
            let i = ((y * fb.width() + x) * 4) as usize;
            let p = &fb.pixels()[i..i + 3];
            (0..3)
                .map(|c| (i32::from(p[c]) - i32::from(bg[c])).abs())
                .sum::<i32>()
                > 120
        });
        if inked {
            lo.get_or_insert(y);
            hi = Some(y);
        }
    }
    Some((f64::from(lo?) + f64::from(hi?) + 1.0) * 0.5)
}

/// Paint the Demos popup under `profile` at `ux` UX scale and `dpr` device
/// scale and return how far (logical px) the icon's ink centre sits from the
/// label's in row 1, plus that row's height.  Row 1 is "Tests", a label with
/// no descender, so its ink spans exactly baseline to cap height.
fn icon_offset_from_label(profile: InputProfile, ux: f64, dpr: f64) -> (f64, f64) {
    crate::input_profile::set_input_profile(profile);
    crate::touch_state::clear_last_touch_event_for_testing();
    crate::ux_scale::set_ux_scale(ux);
    crate::device_scale::set_device_scale(dpr);
    let scale = ux * dpr;
    // Lars's phone: 412 CSS px wide, in logical px.
    let viewport = Size::new((412.0 / ux).floor(), (760.0 / ux).floor());
    crate::widget::set_current_viewport(viewport);

    let mut menu = PopupMenu::new(demos_menu());
    menu.open_at(Point::new(4.0, viewport.height - 4.0));
    let row: Rect = menu.state.layouts(&menu.items, viewport)[0].rows[1].rect;

    let mut fb = Framebuffer::new(
        (viewport.width * scale).ceil() as u32,
        (viewport.height * scale).ceil() as u32,
    );
    let bg;
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        ctx.scale(scale, scale);
        bg = ctx.visuals().panel_fill;
        menu.paint(&mut ctx, icon_font(), FONT_SIZE, viewport);
    }
    let bg = [
        (f64::from(bg.r) * 255.0).round() as u8,
        (f64::from(bg.g) * 255.0).round() as u8,
        (f64::from(bg.b) * 255.0).round() as u8,
    ];
    let style = MenuStyle::default();
    let dev = |v: f64| (v * scale).round() as u32;
    let (y0, y1) = (dev(row.y), dev(row.y + row.height));
    let icon = ink_centre(
        &fb,
        bg,
        dev(row.x + style.icon_x - 2.0),
        dev(row.x + style.label_x - 2.0),
        y0,
        y1,
    )
    .expect("the icon glyph paints in the icon column");
    let label = ink_centre(
        &fb,
        bg,
        dev(row.x + style.label_x),
        dev(row.x + style.label_x + 40.0),
        y0,
        y1,
    )
    .expect("the label paints in the label column");
    crate::device_scale::set_device_scale(1.0);
    crate::ux_scale::set_ux_scale(1.0);
    crate::input_profile::set_input_profile(InputProfile::Desktop);
    ((icon - label) / scale, row.height)
}

#[test]
fn a_row_icon_is_centred_with_its_label_on_a_phone() {
    let _guard = crate::input_profile::profile_test_lock();
    let ux = InputProfile::MobileAndroid.recommended_ux_scale();
    let (offset, row_h) = icon_offset_from_label(InputProfile::MobileAndroid, ux, 2.625);
    assert!(
        row_h > 30.0,
        "the fixture has finger-sized rows (row {row_h} logical px)"
    );
    assert!(
        offset.abs() <= 1.5,
        "the icon's ink centre sits {offset:.2} logical px from the label's \
         in a {row_h} px touch row"
    );
}

#[test]
fn a_row_icon_is_centred_with_its_label_on_the_desktop() {
    let _guard = crate::input_profile::profile_test_lock();
    let (offset, row_h) = icon_offset_from_label(InputProfile::Desktop, 1.0, 1.0);
    assert_eq!(row_h, super::super::geometry::ROW_H);
    assert!(
        offset.abs() <= 1.5,
        "the icon's ink centre sits {offset:.2} logical px from the label's \
         in a {row_h} px desktop row"
    );
}
