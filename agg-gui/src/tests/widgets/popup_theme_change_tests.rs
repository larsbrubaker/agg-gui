//! agg-sharp `PopupThemeChangeTests` (Tests/Agg.Tests/AggSharpDemo/
//! PopupThemeChangeTests.cs), the two tests MatterCAD relies on: a theme
//! picked while a drop-down list or a menu is open shows on it at once,
//! and only the colours it took from the theme change.
//!
//! agg-gui paints its popups from the current [`Visuals`] every frame, so a
//! theme change is [`set_visuals`] followed by the next paint; C#'s
//! `ThemeConfig.NotifyChanged` subscription has no counterpart, so the C#
//! `ChangedSubscribers` checks (nothing left listening once the popup is
//! down) have nothing to count.  C#'s `DropDownContainer` is the open
//! [`ComboBox`] list; its `MenuItemColorStatesView` rows are the list's
//! painted rows, sampled here as pixels.  The C# icon row
//! (`MenuItemStatesView`) has no combo counterpart: a combo row is text.
//! In the menu test, C#'s row `FillColor` (the button fill) has no
//! counterpart either (an agg-gui row is unfilled at rest), and its
//! `ThemedTextEditWidget` half is `ThemedTextEditWidgetColorTests`'
//! per-state colours, which an agg-gui `TextField` paints from the visuals
//! wherever it sits.
//!
//! Not ported (AggSharpDemo only): `SwatchRowsShowOnlyTheSwatchAndATapOnItPicksTheRow`,
//! `OpenViewColorMenuFollowsTheThemePickedFromIt`.

use super::*;
use crate::text::Font;
use crate::theme::{set_visuals, AccentColor, Visuals};
use crate::widgets::combo_box::ComboBoxStateStyle;
use crate::widgets::menu::{MenuEntry, MenuItem, PopupMenu};
use crate::{Container, Point};
use std::sync::Arc;

const W: f64 = 160.0;
const H: f64 = 200.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

fn rgb(c: Color) -> [u8; 3] {
    [
        (c.r * 255.0).round() as u8,
        (c.g * 255.0).round() as u8,
        (c.b * 255.0).round() as u8,
    ]
}

fn sample(fb: &Framebuffer, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = &fb.pixels()[i..i + 3];
    [p[0], p[1], p[2]]
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| x.abs_diff(*y) <= 3)
}

/// Whether any pixel of row band `y` (left of x 60) is near `c`.
fn has_ink(fb: &Framebuffer, y: u32, c: [u8; 3]) -> bool {
    (y - 4..y + 4).any(|y| (8..60).any(|x| near(sample(fb, x, y), c)))
}

/// Centre height (Y-up) of row `i` of a three-row list hanging above a
/// combo at the bottom of the window.
fn row_y(i: usize) -> u32 {
    (24.0 + (2 - i) as f64 * 22.0 + 11.0) as u32
}

/// `combo` as the root of an `App`, opened by a click.
fn opened(combo: ComboBox) -> App {
    let mut app = App::new(Box::new(combo));
    app.layout(Size::new(W, H));
    app.on_mouse_down(12.0, H - 12.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(12.0, H - 12.0, MouseButton::Left, Modifiers::default());
    app
}

fn frame(app: &mut App) -> Framebuffer {
    let mut fb = Framebuffer::new(W as u32, H as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        app.paint(&mut ctx);
    }
    fb
}

#[test]
fn open_drop_down_list_follows_a_the_theme_change() {
    set_visuals(Visuals::light().with_accent_color(AccentColor::Blue));
    let mut themed = opened(ComboBox::new(vec!["Item 0", "Item 1", "Item 2"], 0, font()));
    let fb = frame(&mut themed);
    assert!(near(
        sample(&fb, (W - 20.0) as u32, row_y(1)),
        rgb(Visuals::light().widget_bg)
    ));

    // Light to dark with a green accent while the list is open
    let dark = Visuals::dark().with_accent_color(AccentColor::Green);
    set_visuals(dark.clone());
    let fb = frame(&mut themed);
    assert!(
        near(
            sample(&fb, (W - 20.0) as u32, row_y(1)),
            rgb(dark.widget_bg)
        ),
        "the open list's fill follows"
    );
    assert!(
        near(
            sample(&fb, (W - 20.0) as u32, row_y(0)),
            rgb(AccentColor::Green.color())
        ),
        "the current choice is marked in the accent picked"
    );
    assert!(
        has_ink(&fb, row_y(1), rgb(dark.text_color)),
        "a row's text is drawn in the new text colour"
    );

    // A caller's own colours (MatterCAD's MHDropDownList sets these) are not
    // the theme's to change
    let custom_background = Color::rgb(10.0 / 255.0, 200.0 / 255.0, 30.0 / 255.0);
    let custom_text = Color::rgb(250.0 / 255.0, 240.0 / 255.0, 5.0 / 255.0);
    let custom = ComboBox::new(vec!["Custom 0", "Custom 1", "Custom 2"], 0, font())
        .with_state_style(ComboBoxStateStyle {
            popup_fill: Some(custom_background),
            item_text: Some(custom_text),
            ..Default::default()
        });
    let mut custom = opened(custom);
    for visuals in [dark, Visuals::light()] {
        set_visuals(visuals);
        let fb = frame(&mut custom);
        assert!(near(
            sample(&fb, (W - 20.0) as u32, row_y(1)),
            rgb(custom_background)
        ));
        assert!(has_ink(&fb, row_y(1), rgb(custom_text)));
    }
}

#[test]
fn only_the_colours_the_menu_took_from_the_theme_follow_it() {
    // A light theme whose panel fill and widget fill are the same colour:
    // each must still go its own way
    let shared = Color::rgb(240.0 / 255.0, 240.0 / 255.0, 240.0 / 255.0);
    let mut light = Visuals::light();
    light.panel_fill = shared;
    light.widget_bg = shared;
    light.text_color = Color::rgb(20.0 / 255.0, 20.0 / 255.0, 20.0 / 255.0);
    set_visuals(light);

    // A caller's own widget whose fill happens to equal the theme's
    let mut menu = PopupMenu::new(vec![MenuEntry::from(MenuItem::action("Row", "row"))]);
    menu.push_widget_row(Box::new(Container::new().with_background(shared)), 22.0);
    let viewport = Size::new(300.0, 300.0);
    menu.open_at(Point::new(20.0, 280.0));
    let paint = |menu: &mut PopupMenu| {
        let mut fb = Framebuffer::new(300, 300);
        {
            let mut ctx = GfxCtx::new(&mut fb);
            menu.paint(&mut ctx, font(), 13.0, viewport);
        }
        fb
    };
    let layout = menu.state.layouts(&menu.items, viewport)[0].clone();
    let row = layout.rows[0].rect;
    let literal = layout.rows[1].rect;
    let fb = paint(&mut menu);
    let panel_at = (
        (row.x + row.width - 10.0) as u32,
        (row.y + row.height * 0.5) as u32,
    );
    assert!(near(sample(&fb, panel_at.0, panel_at.1), rgb(shared)));

    let dark_window = Color::rgb(38.0 / 255.0, 38.0 / 255.0, 46.0 / 255.0);
    let dark_button = Color::rgb(56.0 / 255.0, 56.0 / 255.0, 66.0 / 255.0);
    let dark_text = Color::rgb(230.0 / 255.0, 230.0 / 255.0, 235.0 / 255.0);
    let mut dark = Visuals::dark();
    dark.panel_fill = dark_window;
    dark.widget_bg = dark_button;
    dark.text_color = dark_text;
    set_visuals(dark);
    let fb = paint(&mut menu);

    assert!(near(sample(&fb, panel_at.0, panel_at.1), rgb(dark_window)));
    let text_y = (row.y + row.height * 0.5) as u32;
    let text = (text_y - 4..text_y + 4).any(|y| {
        (row.x as u32..(row.x + 60.0) as u32).any(|x| near(sample(&fb, x, y), rgb(dark_text)))
    });
    assert!(text, "the row's label is drawn in the new text colour");
    assert!(
        near(
            sample(
                &fb,
                (literal.x + literal.width * 0.5) as u32,
                (literal.y + literal.height * 0.5) as u32
            ),
            rgb(shared)
        ),
        "the caller's own fill stays"
    );
}
