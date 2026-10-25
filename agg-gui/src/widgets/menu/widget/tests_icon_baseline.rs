//! A popup row's icon glyph (`MenuItem::icon`) stands on the same device
//! pixel row as its label's text under every line box, the snapped one
//! (`LineBox::EmSnapped`) included, and on the desktop stays within half a
//! pixel of the 7 px baseline it had before it followed the label.
//!
//! The row's label is a `Label`, whose baseline `label/line_box.rs` places
//! (snapped to device pixels under `EmSnapped`); the icon is drawn directly
//! by `popup_paint.rs`.  The tests use the letter "H" as both the icon and
//! the label, so the two inks are the same glyph and must cover the same
//! pixel rows exactly when their baselines agree.

use std::sync::Arc;

use crate::color::Color;
use crate::font_settings::LineBox;
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::input_profile::{profile_test_lock, set_input_profile, InputProfile};
use crate::text::Font;

use super::super::model::MenuItem;
use super::super::paint::MenuStyle;
use super::PopupMenu;

const FONT_SIZE: f64 = 13.0;
const VIEWPORT: Size = Size {
    width: 300.0,
    height: 200.0,
};

fn test_font() -> Arc<Font> {
    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// First and last device pixel rows (counted up from the bottom) holding
/// ink between columns `x0..x1`, or `None` when there is none.
fn ink_rows(fb: &Framebuffer, x0: u32, x1: u32) -> Option<(u32, u32)> {
    let inked = |y: u32| {
        (x0..x1).any(|x| {
            let i = ((y * fb.width() + x) * 4) as usize;
            fb.pixels()[i..i + 3].iter().any(|&c| c > 40)
        })
    };
    let rows: Vec<u32> = (0..fb.height()).filter(|&y| inked(y)).collect();
    Some((*rows.first()?, *rows.last()?))
}

/// The ink rows of the icon "H" and the label "H" in one desktop row
/// painted under `line_box` at `dpr`, plus the row.
fn icon_and_label_rows(line_box: LineBox, dpr: f64) -> ((u32, u32), (u32, u32), Rect) {
    set_input_profile(InputProfile::Desktop);
    crate::touch_state::clear_last_touch_event_for_testing();
    crate::ux_scale::set_ux_scale(1.0);
    crate::device_scale::set_device_scale(dpr);
    crate::font_settings::set_line_box(line_box);
    crate::widget::set_current_viewport(VIEWPORT);

    let mut menu = PopupMenu::new(vec![MenuItem::action("H", "h").icon('H').into()]);
    menu.open_at(Point::new(20.3, 150.7));
    let row = menu.state.layouts(&menu.items, VIEWPORT)[0].rows[0].rect;
    let mut fb = Framebuffer::new(
        (VIEWPORT.width * dpr).ceil() as u32,
        (VIEWPORT.height * dpr).ceil() as u32,
    );
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        ctx.scale(dpr, dpr);
        // White text on a black panel, so ink is any bright pixel.
        menu.set_style(MenuStyle {
            background: Some(Color::black()),
            border_width: 0.0,
            shadow: false,
            ..MenuStyle::default()
        });
        let mut v = crate::theme::current_visuals();
        v.text_color = Color::white();
        crate::theme::set_visuals(v.clone());
        menu.paint(&mut ctx, test_font(), FONT_SIZE, VIEWPORT);
    }
    let style = MenuStyle::default();
    let dev = |v: f64| (v * dpr).round() as u32;
    let icon = ink_rows(
        &fb,
        dev(row.x + style.icon_x - 2.0),
        dev(row.x + style.label_x - 4.0),
    )
    .expect("icon ink");
    let label = ink_rows(
        &fb,
        dev(row.x + style.label_x),
        dev(row.x + style.label_x + 14.0),
    )
    .expect("label ink");
    crate::font_settings::set_line_box(LineBox::Standard);
    crate::device_scale::set_device_scale(1.0);
    (icon, label, row)
}

#[test]
fn a_row_icon_shares_its_labels_pixel_rows_under_every_line_box() {
    let _guard = profile_test_lock();
    let saved = crate::theme::current_visuals();
    for line_box in [LineBox::EmSnapped, LineBox::Em, LineBox::Standard] {
        for dpr in [1.0, 1.25, 2.0] {
            let (icon, label, row) = icon_and_label_rows(line_box, dpr);
            assert_eq!(
                icon, label,
                "{line_box:?} at DPR {dpr}: the icon's ink rows (bottom, top) {icon:?} \
                 must be the label's {label:?} in row {row:?}"
            );
        }
    }
    crate::theme::set_visuals(saved);
}

#[test]
fn a_desktop_row_icon_stays_within_half_a_pixel_of_its_old_baseline() {
    let _guard = profile_test_lock();
    let saved = crate::theme::current_visuals();
    // The old icon baseline was 7 logical px above the row's bottom.  Under
    // the unsnapped line boxes the icon (on the label's baseline) stays
    // within half a device pixel of it; the snapped box is pinned by the
    // pixel-row test above.
    for line_box in [LineBox::Em, LineBox::Standard] {
        for dpr in [1.0, 1.25, 2.0] {
            let (icon, _, row) = icon_and_label_rows(line_box, dpr);
            let old_bottom = ((row.y + 7.0) * dpr).floor() as i64;
            assert!(
                (i64::from(icon.0) - old_bottom).abs() <= 1,
                "{line_box:?} at DPR {dpr}: icon ink bottom row {} vs the old baseline's {old_bottom}",
                icon.0
            );
        }
    }
    crate::theme::set_visuals(saved);
}
