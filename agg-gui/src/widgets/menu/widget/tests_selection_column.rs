//! `MenuStyle::selection_column`: the opt-in agg-sharp check / radio layout,
//! with the mark in its own leading column and the row's icon beside it
//! (agg-sharp `RadioMenuItem` / `CheckboxMenuItem`: the mark is the row's
//! gutter image, the icon starts the row's content, then the label).
//!
//! Painted into a real framebuffer and read back: "ink" is any pixel that
//! differs from the row's background.

use std::sync::Arc;

use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::icon_image::IconImage;
use crate::text::Font;

use super::super::fit_width::FitMeasure;
use super::super::model::{MenuEntry, MenuItem};
use super::super::paint::MenuStyle;
use super::super::style::SelectionColumn;
use super::PopupMenu;

const VIEWPORT: Size = Size {
    width: 400.0,
    height: 400.0,
};

const COLUMN: SelectionColumn = SelectionColumn {
    mark_x: 13.0,
    icon_x: 26.0,
    icon_label_x: 50.0,
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

fn column_style() -> MenuStyle {
    MenuStyle {
        row_h: 28.0,
        icon_x: 5.0,
        label_x: 48.0,
        selection_column: Some(COLUMN),
        ..MenuStyle::default()
    }
}

/// Paint `items` under `style`; returns the framebuffer and the row rects.
fn paint(items: Vec<MenuEntry>, style: MenuStyle) -> (Framebuffer, Vec<Rect>) {
    let _guard = crate::input_profile::profile_test_lock();
    crate::input_profile::set_input_profile(crate::input_profile::InputProfile::Desktop);
    crate::ux_scale::set_ux_scale(1.0);
    crate::device_scale::set_device_scale(1.0);
    let mut menu = PopupMenu::new(items).with_style(style);
    menu.open_at(Point::new(20.0, 380.0));
    let rows = menu.state.layouts(&menu.items, VIEWPORT)[0]
        .rows
        .iter()
        .map(|r| r.rect)
        .collect();
    let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        menu.paint(&mut ctx, test_font(), 14.0, VIEWPORT);
    }
    (fb, rows)
}

fn px(fb: &Framebuffer, x: f64, y: f64) -> [u8; 3] {
    let i = ((y as u32 * fb.width() + x as u32) * 4) as usize;
    [fb.pixels()[i], fb.pixels()[i + 1], fb.pixels()[i + 2]]
}

/// Pixels in the `size` square centred at `(cx, cy)` that differ from
/// `row`'s background (sampled near its right end, where nothing paints).
fn ink(fb: &Framebuffer, row: Rect, cx: f64, cy: f64, size: f64) -> usize {
    let bg = px(fb, row.x + row.width - 4.0, row.y + 2.0);
    let mut n = 0;
    let half = size * 0.5;
    let mut y = (cy - half).floor();
    while y < cy + half {
        let mut x = (cx - half).floor();
        while x < cx + half {
            if px(fb, x, y) != bg {
                n += 1;
            }
            x += 1.0;
        }
        y += 1.0;
    }
    n
}

fn mark_ink(fb: &Framebuffer, row: Rect) -> usize {
    ink(
        fb,
        row,
        row.x + COLUMN.mark_x,
        row.y + row.height * 0.5,
        12.0,
    )
}

#[test]
fn radio_rows_show_a_circle_in_the_column_and_keep_their_icon() {
    let (fb, rows) = paint(
        vec![
            MenuItem::action("Shaded", "shaded")
                .radio(true)
                .image_icon(green_icon())
                .into(),
            MenuItem::action("Outlines", "outlines")
                .radio(false)
                .image_icon(green_icon())
                .into(),
        ],
        column_style(),
    );
    let (on, off) = (rows[0], rows[1]);
    let cy = |r: Rect| r.y + r.height * 0.5;
    // Both rows show a circle; only the selected one has a centre dot.
    assert!(mark_ink(&fb, on) > 0, "selected radio shows its circle");
    assert!(
        mark_ink(&fb, off) > 0,
        "unselected radio shows a hollow circle"
    );
    assert!(
        ink(&fb, on, on.x + COLUMN.mark_x, cy(on), 2.0) > 0,
        "selected radio has a centre dot"
    );
    assert_eq!(
        ink(&fb, off, off.x + COLUMN.mark_x, cy(off), 2.0),
        0,
        "unselected radio is hollow"
    );
    // The icon sits in its own column beside the mark.
    for r in [on, off] {
        assert_eq!(px(&fb, r.x + COLUMN.icon_x + 6.0, cy(r)), [0, 255, 0]);
    }
    // Nothing moves to the row's right end.
    assert_eq!(ink(&fb, on, on.x + on.width - 12.0, cy(on), 10.0), 0);
}

#[test]
fn check_rows_show_the_mark_only_when_checked() {
    let (fb, rows) = paint(
        vec![
            MenuItem::action("Grid", "grid").checked(true).into(),
            MenuItem::action("Axes", "axes").checked(false).into(),
        ],
        column_style(),
    );
    assert!(mark_ink(&fb, rows[0]) > 0, "checked row shows its mark");
    assert_eq!(mark_ink(&fb, rows[1]), 0, "unchecked row shows nothing");
}

#[test]
fn rows_without_a_selection_keep_the_icon_column() {
    let (fb, rows) = paint(
        vec![MenuItem::action("Export", "export")
            .image_icon(green_icon())
            .into()],
        column_style(),
    );
    let r = rows[0];
    let cy = r.y + r.height * 0.5;
    assert_eq!(
        px(&fb, r.x + 5.0 + 6.0, cy),
        [0, 255, 0],
        "icon at style.icon_x"
    );
    assert_ne!(px(&fb, r.x + COLUMN.icon_x + 6.0 + 7.0, cy), [0, 255, 0]);
}

#[test]
fn rust_only_default_style_has_no_selection_column() {
    assert_eq!(MenuStyle::default().selection_column, None);
    // An unselected radio row keeps today's look: nothing in the icon slot.
    let style = MenuStyle::default();
    let (fb, rows) = paint(
        vec![MenuItem::action("Wireframe", "wire").radio(false).into()],
        style,
    );
    let r = rows[0];
    let cy = r.y + r.height * 0.5;
    assert_eq!(ink(&fb, r, r.x + style.icon_x + 2.0, cy, 10.0), 0);
}

#[test]
fn fitted_width_moves_check_and_radio_icon_rows_to_the_icon_label_edge() {
    let style = column_style();
    let measure = FitMeasure::new(test_font(), 14.0, &style);
    let plain = MenuItem::action("Shaded", "shaded");
    let radio_icon = plain.clone().radio(true).image_icon(green_icon());
    let radio_bare = plain.clone().radio(true);
    let base = measure.row_natural_width(&plain);
    assert_eq!(
        measure.row_natural_width(&radio_icon) - base,
        COLUMN.icon_label_x - style.label_x
    );
    assert_eq!(measure.row_natural_width(&radio_bare), base);
}
