//! agg-sharp `DropDownListChromeTests`
//! (Tests/Agg.Tests/Agg.UI/DropDownListChromeTests.cs): an open drop down
//! (agg-gui's `ComboBox`) rounds its panel by the popup-menu radius and
//! insets its rounded row highlights clear of the panel's corners, and the
//! closed field paints no stray line at a fractional position.
//!
//! C# asserts on its widgets' `BackgroundRadius`; agg-gui paints the popup
//! directly, so these read the painted pixels.
//!
//! - `DropDownPopupIsRounded`: the panel rounds by the installed popup-menu
//!   style's radius (`set_menu_style`), agg-gui's `ThemeConfig.MenuPopupRadius`.
//! - The two `ArrowFade*` tests: agg-gui's field has no fade under the arrow,
//!   so they pin the outcome C# fixed: across the fade's columns every row
//!   matches the plain field beside it.
//!
//! Not ported: `IconRowStatesAreRounded` — agg-gui's `ComboBox` has no icon
//! rows, and MatterCAD never adds one (its `DropDownList.AddItem` calls all
//! pass text).

use agg_gui::widgets::combo_box::ComboBoxStyle;
use agg_gui::widgets::menu::{current_menu_style, reset_menu_style, set_menu_style, MenuStyle};
use agg_gui::{App, Color, ComboBox, ComboBoxStateStyle, Framebuffer, GfxCtx, Modifiers};
use agg_gui::{MouseButton, Rect, Size, Widget};

use super::support::{font, install_theme, is_near, pixel, render_at, rgb};

const W: f64 = 160.0;
const H: f64 = 200.0;
/// The closed box's height: the popup hangs from its top edge.
const CLOSED_H: f64 = 24.0;
const ITEM_H: f64 = 22.0;

/// C#'s `OpenDropDown`: a two-row list as the root of an `App`, opened, so
/// the popup paints. The root combo sits at the bottom (Y-up) and its list
/// opens upward from y = 24.
fn open_drop_down(combo: ComboBox, hover: Option<usize>) -> Framebuffer {
    let mut app = App::new(Box::new(combo));
    app.layout(Size::new(W, H));
    app.on_mouse_down(12.0, H - 12.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(12.0, H - 12.0, MouseButton::Left, Modifiers::default());
    let mut fb = Framebuffer::new(W as u32, H as u32);
    let paint = |app: &mut App, fb: &mut Framebuffer| {
        let mut ctx = GfxCtx::new(fb);
        ctx.clear(Color::rgb(1.0, 1.0, 1.0));
        app.paint(&mut ctx);
    };
    paint(&mut app, &mut fb);
    // The list's rows take hover once a frame has placed them.
    if let Some(i) = hover {
        app.on_mouse_move(W - 20.0, H - row_bottom(i) - ITEM_H * 0.5);
        paint(&mut app, &mut fb);
    }
    fb
}

fn two_rows() -> ComboBox {
    ComboBox::new(vec!["Item 0", "Item 1"], 0, font())
}

/// Y-up bottom of row `i` of the two-row list.
fn row_bottom(i: usize) -> f64 {
    CLOSED_H + (1 - i) as f64 * ITEM_H
}

#[test]
fn drop_down_popup_is_rounded() {
    install_theme(false);
    set_menu_style(MenuStyle {
        radius: 8.0,
        ..current_menu_style()
    });
    let image = open_drop_down(two_rows(), None);
    reset_menu_style();

    let background = pixel(&image, 1, (H - 10.0) as i32);
    let panel_top = (CLOSED_H + 2.0 * ITEM_H) as i32;
    // One pixel in from the panel's top-left corner: outside an 8 px arc
    // (inside the 4 px one the panel used before it shared the menu radius).
    let corner = pixel(&image, 1, panel_top - 2);
    let panel = pixel(&image, (W * 0.5) as i32, panel_top - 2);
    assert!(
        is_near(corner, background, 30),
        "a drop down panel rounds by the popup-menu radius, found {corner:?}"
    );
    assert!(
        !is_near(panel, background, 30),
        "the panel is painted beside its corner, found {panel:?}"
    );
}

#[test]
fn text_row_highlight_is_rounded_and_inset_from_the_panel() {
    let v = install_theme(false);
    let red = Color::rgb(1.0, 0.0, 0.0);
    // Row 0 is the selection (the accent row), row 1 is hovered (red here).
    let combo = two_rows().with_state_style(ComboBoxStateStyle {
        item_hover_fill: Some(red),
        ..Default::default()
    });
    let image = open_drop_down(combo, Some(1));
    let panel_top = (CLOSED_H + 2.0 * ITEM_H) as i32;
    let panel_bottom = CLOSED_H as i32;

    for (row, fill) in [(0usize, rgb(v.accent)), (1, rgb(red))] {
        let y0 = row_bottom(row) as i32;
        let mid = y0 + (ITEM_H * 0.5) as i32;
        let x = (W - 20.0) as i32;
        assert!(
            is_near(pixel(&image, x, mid), fill, 30),
            "row {row} is highlighted"
        );

        // No highlight reaches the panel's left or right edge...
        assert!(
            !is_near(pixel(&image, 0, mid), fill, 30),
            "row {row} left inset"
        );
        assert!(
            !is_near(pixel(&image, W as i32 - 1, mid), fill, 30),
            "row {row} right inset"
        );
        // ...nor the top edge (the top row) or the bottom edge (the bottom row).
        if row == 0 {
            assert!(
                !is_near(pixel(&image, x, panel_top - 1), fill, 30),
                "top inset"
            );
        } else {
            assert!(
                !is_near(pixel(&image, x, panel_bottom), fill, 30),
                "bottom inset"
            );
        }

        // A rounded highlight leaves its own corner pixel partly unfilled.
        let corner = pixel(&image, 2, y0 + 1);
        assert!(
            !is_near(corner, fill, 30),
            "a square highlight in a rounded panel is what the inset look avoids: row {row} corner {corner:?}"
        );
    }
}

/// A 140 px field with a 1 px outline at a fractional position on white,
/// as the markdown toolbar sizes its text-size list. Returns the image and
/// the field's bounds in it.
fn fractional_field(fill: Color, border: Color) -> (Framebuffer, Rect) {
    let mut drop_down = ComboBox::new(Vec::<String>::new(), 0, font())
        .with_no_selection("")
        .with_style(ComboBoxStyle {
            fill: Some(fill),
            border: Some(border),
            ..Default::default()
        });
    let size = drop_down.layout(Size::new(140.0, 60.0));
    drop_down.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    let (x, y) = (10.5, 10.37);
    let image = render_at(
        &mut drop_down,
        (400, 200),
        (x, y),
        Color::rgb(1.0, 1.0, 1.0),
    );
    (image, Rect::new(x, y, size.width, size.height))
}

/// Every row of the field outside the chevron's band, as `(y, reference,
/// shown)` red values for each column of the fade (`right - 70` to
/// `right - 12`), the reference taken `left + 12`.
fn fade_rows(image: &Framebuffer, field: Rect) -> Vec<(i32, i32, i32, i32)> {
    let (left, right) = (field.x, field.x + field.width);
    let (bottom, top) = (field.y, field.y + field.height);
    let center_y = field.y + field.height * 0.5;
    let reference_x = left as i32 + 12;
    let (fade_left, fade_right) = ((right - 70.0) as i32, (right - 12.0) as i32);
    let mut out = Vec::new();
    for y in bottom.floor() as i32..=top.ceil() as i32 {
        if (y as f64 - center_y).abs() < 8.0 {
            continue;
        }
        let reference = pixel(image, reference_x, y)[0];
        for x in fade_left..=fade_right {
            out.push((x, y, reference, pixel(image, x, y)[0]));
        }
    }
    out
}

#[test]
fn arrow_fade_draws_no_line_at_a_fractional_position() {
    install_theme(false);
    let (image, field) = fractional_field(Color::rgb(1.0, 1.0, 1.0), Color::rgb(0.5, 0.5, 0.5));
    assert!(
        field.width >= 140.0,
        "the field ({field:?}) must lie on the image"
    );
    for (x, y, reference, shown) in fade_rows(&image, field) {
        assert!(
            shown >= reference - 8,
            "the fade at ({x}, {y}) is darker than the field beside it: {shown} vs {reference}"
        );
    }
}

#[test]
fn arrow_fade_leaves_the_outline_alone_on_a_dark_field() {
    install_theme(false);
    let dark = Color::rgb(4.0 / 255.0, 4.0 / 255.0, 4.0 / 255.0);
    let outline = Color::rgb(200.0 / 255.0, 200.0 / 255.0, 200.0 / 255.0);
    let (image, field) = fractional_field(dark, outline);
    for (x, y, reference, shown) in fade_rows(&image, field) {
        assert!(
            (shown - reference).abs() <= 8,
            "the fade at ({x}, {y}) shows {shown} where the field beside it shows {reference}"
        );
    }
}
