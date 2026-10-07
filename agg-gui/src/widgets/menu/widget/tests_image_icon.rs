//! `MenuItem::image_icon`: an image in a popup row's icon slot paints at the
//! glyph column, centred on the row, and counts as the row's leading marker.

use std::sync::Arc;

use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Size};
use crate::gfx_ctx::GfxCtx;
use crate::icon_image::IconImage;
use crate::text::Font;

use super::super::model::MenuItem;
use super::super::paint::MenuStyle;
use super::PopupMenu;

const VIEWPORT: Size = Size {
    width: 400.0,
    height: 400.0,
};

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

#[test]
fn image_icon_paints_in_the_icon_column() {
    let _guard = crate::input_profile::profile_test_lock();
    crate::input_profile::set_input_profile(crate::input_profile::InputProfile::Desktop);
    crate::ux_scale::set_ux_scale(1.0);

    let item = MenuItem::action("Make Component", "make").image_icon(green_icon());
    assert!(item.has_leading_icon());
    assert_eq!(item.icon, None, "the glyph field is untouched");

    let mut menu = PopupMenu::new(vec![item.into()]);
    menu.open_at(Point::new(20.0, 300.0));
    let row = menu.state.layouts(&menu.items, VIEWPORT)[0].rows[0].rect;
    let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        menu.paint(&mut ctx, test_font(), 14.0, VIEWPORT);
    }
    let px = |x: f64, y: f64| {
        let i = ((y as u32 * fb.width() + x as u32) * 4) as usize;
        [fb.pixels()[i], fb.pixels()[i + 1], fb.pixels()[i + 2]]
    };
    let style = MenuStyle::default();
    let cx = row.x + style.icon_x + 6.0;
    let cy = row.y + row.height * 0.5;
    assert_eq!(px(cx, cy), [0, 255, 0], "image centre in the icon slot");
    assert_ne!(
        px(row.x + style.icon_x + 14.0, cy),
        [0, 255, 0],
        "image is drawn at its 12 px logical width"
    );
}
