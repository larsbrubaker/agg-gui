//! agg-sharp `PopupMenuChromeTests` (Tests/Agg.Tests/Agg.UI/PopupMenuChromeTests.cs):
//! the rounded menu panel and the inset, rounded row highlight, painted into a
//! real framebuffer and read back.
//!
//! C# reads `BackgroundRadius` off the panel and row widgets; agg-gui's popup
//! paints its panel and highlight itself (`menu/paint.rs`), so these tests
//! check the painted pixels: a panel corner stays unpainted, and the hover
//! highlight leaves panel colour between itself and the panel's edges and
//! rounds its own corners.
//!
//! `MenuRowsAreTheThemeRowHeight` duplicates
//! `tests_style.rs::row_height_option_sets_item_rows_and_panel_height` (every
//! item row is exactly the style's `row_h`). `APopupHostingAMenuWearsTheMenuCorners`
//! and `AScrolledPopupCarriesTheRoundedFillItsBorderTraces` exercise C#'s
//! `PopupWidget` scroll host, which agg-gui's popups do not have yet (menu
//! overflow scrolling, `PopupMenuOverflowScrollTests`).

use std::sync::Arc;

use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;

use super::super::model::{MenuEntry, MenuItem};
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

/// A three row menu (First, Middle, Last) with a flat grey panel, painted
/// over black with row `hovered` highlighted. Returns the framebuffer, the
/// panel rect and the row rects.
fn paint(hovered: Option<usize>) -> (Framebuffer, Rect, Vec<Rect>) {
    let _guard = crate::input_profile::profile_test_lock();
    crate::input_profile::set_input_profile(crate::input_profile::InputProfile::Desktop);
    crate::touch_state::clear_last_touch_event_for_testing();
    crate::ux_scale::set_ux_scale(1.0);
    crate::device_scale::set_device_scale(1.0);
    let style = MenuStyle {
        row_h: 28.0,
        background: Some(Color::from_rgb8(0xf1, 0xf1, 0xf1)),
        border_width: 0.0,
        shadow: false,
        ..MenuStyle::default()
    };
    let items: Vec<MenuEntry> = ["First", "Middle", "Last"]
        .iter()
        .map(|l| MenuItem::action(*l, *l).into())
        .collect();
    let mut menu = PopupMenu::new(items).with_style(style);
    menu.open_at(Point::new(20.0, 380.0));
    menu.state.hover_path = hovered.map(|i| vec![i]);
    let layout = menu.state.layouts(&menu.items, VIEWPORT)[0].clone();
    let rows = layout.rows.iter().map(|r| r.rect).collect();
    let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        menu.paint(&mut ctx, test_font(), 14.0, VIEWPORT);
    }
    (fb, layout.rect, rows)
}

fn px(fb: &Framebuffer, x: f64, y: f64) -> [u8; 3] {
    let i = ((y as u32 * fb.width() + x as u32) * 4) as usize;
    [fb.pixels()[i], fb.pixels()[i + 1], fb.pixels()[i + 2]]
}

#[test]
fn menu_panel_is_rounded() {
    let (fb, panel, _) = paint(None);
    let fill = px(&fb, panel.x + panel.width * 0.5, panel.y + 2.5);
    assert_eq!(fill, [0xf1, 0xf1, 0xf1]);
    let top = panel.y + panel.height;
    let right = panel.x + panel.width;
    // a menu panel with square corners is the look this styling replaced:
    // the outermost pixel of every corner stays unpainted
    for (x, y) in [
        (panel.x + 0.5, top - 0.5),
        (right - 0.5, top - 0.5),
        (panel.x + 0.5, panel.y + 0.5),
        (right - 0.5, panel.y + 0.5),
    ] {
        assert_eq!(px(&fb, x, y), [0, 0, 0], "corner pixel at ({x}, {y})");
    }
}

#[test]
fn row_highlight_is_inset_from_the_panel_and_rounded_itself() {
    let bg = [0xf1, 0xf1, 0xf1];
    // The first and last rows are the ones that meet the panel's rounded
    // corners, so they are the ones that have to be held clear of them.
    let (fb, panel, rows) = paint(Some(0));
    assert_eq!(rows.len(), 3);
    let first = rows[0];
    let mid_y = first.y + first.height * 0.5;
    let highlight = px(&fb, first.x + first.width * 0.5, mid_y);
    assert_ne!(highlight, bg, "the hovered row is highlighted");
    assert_eq!(first.y + first.height, panel.y + panel.height);
    assert_eq!(
        px(
            &fb,
            first.x + first.width * 0.5,
            first.y + first.height - 0.5
        ),
        bg,
        "the top row's highlight must not reach the top edge of a rounded panel"
    );
    assert_eq!(px(&fb, first.x + 0.5, mid_y), bg, "inset from the left");
    assert_eq!(
        px(&fb, first.x + first.width - 0.5, mid_y),
        bg,
        "inset from the right"
    );
    // a square highlight in a rounded panel is what the inset look avoids:
    // the highlight's own corner pixel is not fully covered
    let corner = px(&fb, first.x + 3.5, first.y + first.height - 2.5);
    assert_ne!(corner, highlight, "the highlight rounds its own corners");

    let (fb, panel, rows) = paint(Some(2));
    let last = rows[2];
    assert_eq!(last.y, panel.y);
    assert_ne!(
        px(&fb, last.x + last.width * 0.5, last.y + last.height * 0.5),
        bg
    );
    assert_eq!(
        px(&fb, last.x + last.width * 0.5, last.y + 0.5),
        bg,
        "the bottom row's highlight must not reach the bottom edge of a rounded panel"
    );
}
