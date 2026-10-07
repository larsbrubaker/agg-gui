//! Tests for [`super::super::style::MenuStyle`]: the defaults keep the
//! crate's historical look, each option changes only what it names, and the
//! thread-local default reaches popups created after it is set.
//!
//! Layout depends on [`super::super::effective_metrics`], which reads the
//! process-global input profile, so layout tests hold
//! `input_profile::profile_test_lock()` and pin `InputProfile::Desktop`.

use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::geometry::{Point, Rect, Size};
use crate::input_profile::{profile_test_lock, set_input_profile, InputProfile};
use crate::platform::Platform;
use crate::tests::paint_recorder::{PaintRecorder, PathKind};
use crate::text::Font;
use crate::Color;

use super::super::fit_width::{MenuWidth, FIT_MIN_W};
use super::super::geometry::{MENU_W, ROW_H, SEP_H};
use super::super::model::{MenuEntry, MenuItem};
use super::super::paint::{paint_panel, MenuStyle};
use super::super::style::{current_menu_style, reset_menu_style, set_menu_style, ShortcutFormat};
use super::{MenuBar, PopupMenu};

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

fn items() -> Vec<MenuEntry> {
    vec![
        MenuItem::action("Cut", "cut").shortcut("Ctrl+X").into(),
        MenuEntry::Separator,
        MenuItem::action("Paste", "paste").shortcut("Ctrl+V").into(),
    ]
}

/// The root panel of `popup` opened at a fixed anchor.
fn root_layout(popup: &mut PopupMenu) -> super::super::geometry::PopupLayout {
    popup.set_measure_font(test_font(), 14.0);
    popup.open_at(Point::new(20.0, 500.0));
    popup.state.layouts(&popup.items, Size::new(1200.0, 600.0))[0].clone()
}

/// The MatterCAD / agg-sharp `PopupMenu` look the style exists for.
fn agg_sharp_style() -> MenuStyle {
    MenuStyle {
        row_h: 28.0,
        label_x: 40.0,
        background: Some(Color::from_rgb8(0xf1, 0xf1, 0xf1)),
        border_color: Some(Color::from_rgb8(0xcc, 0xcc, 0xcc)),
        border_width: 1.0,
        shadow: false,
        width: MenuWidth::FitContent,
        min_width: 0.0,
        shortcut_format: ShortcutFormat::Plain,
        ..MenuStyle::default()
    }
}

#[test]
fn rust_only_default_style_is_the_historical_look() {
    let s = MenuStyle::default();
    assert_eq!(s.radius, 5.0);
    assert_eq!(s.shadow_offset, (5.0, -5.0));
    assert_eq!(s.shadow_alpha, 0.22);
    assert!(s.shadow);
    assert_eq!(s.pad_x, 8.0);
    assert_eq!(s.icon_x, 14.0);
    assert_eq!(s.label_x, 32.0);
    assert_eq!(s.shortcut_right, 28.0);
    assert_eq!(s.row_h, ROW_H);
    assert_eq!(s.background, None);
    assert_eq!(s.border_color, None);
    assert_eq!(s.border_width, 1.0);
    assert_eq!(s.width, MenuWidth::Fixed);
    assert_eq!(s.min_width, FIT_MIN_W);
    assert_eq!(s.shortcut_format, ShortcutFormat::Platform);
}

#[test]
fn default_popup_layout_is_unchanged() {
    let _guard = profile_test_lock();
    reset_env();
    let layout = root_layout(&mut PopupMenu::new(items()));
    assert_eq!(layout.rect.width, MENU_W);
    assert_eq!(layout.rows[0].rect.height, ROW_H);
    assert_eq!(layout.rows[1].rect.height, SEP_H);
    assert_eq!(layout.rows[2].rect.height, ROW_H);
    assert_eq!(layout.rect.height, ROW_H * 2.0 + SEP_H);
}

#[test]
fn row_height_option_sets_item_rows_and_panel_height() {
    let _guard = profile_test_lock();
    reset_env();
    let style = MenuStyle {
        row_h: 28.0,
        ..MenuStyle::default()
    };
    let layout = root_layout(&mut PopupMenu::new(items()).with_style(style));
    assert_eq!(layout.rows[0].rect.height, 28.0);
    assert_eq!(layout.rows[1].rect.height, SEP_H, "separators keep SEP_H");
    assert_eq!(layout.rows[2].rect.height, 28.0);
    assert_eq!(layout.rect.height, 56.0 + SEP_H);
    // Hit-testing uses the same rows: 1 px above the bottom of the taller
    // last row (past where a 24 px panel would end) still hits "Paste".
    let row = layout.rows[2].rect;
    let hit = super::super::geometry::hit_test(
        std::slice::from_ref(&layout),
        Point::new(row.x + 10.0, row.y + 1.0),
    );
    assert!(matches!(hit, Some(super::super::geometry::MenuHit::Item(p)) if p == vec![2]));
}

#[test]
fn row_height_is_still_touch_floored() {
    let _guard = profile_test_lock();
    reset_env();
    crate::touch_state::note_touch_event();
    let style = MenuStyle {
        row_h: 28.0,
        ..MenuStyle::default()
    };
    let layout = root_layout(&mut PopupMenu::new(items()).with_style(style));
    let h = layout.rows[0].rect.height;
    reset_env();
    assert_eq!(h, crate::widgets::menu::TOUCH_MIN);
}

#[test]
fn fit_content_and_min_width_options_come_from_the_style() {
    let _guard = profile_test_lock();
    reset_env();
    let style = MenuStyle {
        width: MenuWidth::FitContent,
        min_width: 0.0,
        ..MenuStyle::default()
    };
    let popup = PopupMenu::new(items()).with_style(style);
    assert_eq!(popup.state.width(), MenuWidth::FitContent);
    assert_eq!(popup.state.min_width(), 0.0);
    let width = root_layout(&mut popup.clone()).rect.width;
    assert!(
        width < MENU_W,
        "fitted rows are narrower than the fixed width, got {width}"
    );
    // A floor above the widest row sets the width exactly.
    let floored = MenuStyle {
        min_width: 300.0,
        ..style
    };
    let width = root_layout(&mut PopupMenu::new(items()).with_style(floored))
        .rect
        .width;
    assert_eq!(width, 300.0);
}

#[test]
fn label_x_option_widens_fitted_rows_by_the_same_amount() {
    let _guard = profile_test_lock();
    reset_env();
    let fit = |label_x: f64| {
        let style = MenuStyle {
            width: MenuWidth::FitContent,
            min_width: 0.0,
            label_x,
            ..MenuStyle::default()
        };
        root_layout(&mut PopupMenu::new(items()).with_style(style))
            .rect
            .width
    };
    assert_eq!(fit(40.0) - fit(32.0), 8.0);
}

#[test]
fn default_panel_paints_shadow_theme_fill_and_theme_outline() {
    let mut ctx = PaintRecorder::new();
    let v = ctx.visuals();
    paint_panel(
        &mut ctx,
        Rect::new(0.0, 0.0, 100.0, 50.0),
        &MenuStyle::default(),
    );
    assert_eq!(ctx.fills.len(), 2, "shadow + panel");
    assert_eq!(ctx.fills[0].color, Color::black().with_alpha(0.22));
    assert_eq!(ctx.fills[1].color, v.panel_fill);
    assert_eq!(ctx.strokes.len(), 1);
    assert_eq!(ctx.strokes[0].color, v.widget_stroke);
    assert!(ctx.fills.iter().all(|f| f.kind == PathKind::RoundedRect));
}

#[test]
fn styled_panel_paints_background_and_border_without_shadow() {
    let mut ctx = PaintRecorder::new();
    let style = agg_sharp_style();
    paint_panel(&mut ctx, Rect::new(0.0, 0.0, 100.0, 50.0), &style);
    assert_eq!(ctx.fills.len(), 1, "no shadow fill");
    assert_eq!(ctx.fills[0].color, Color::from_rgb8(0xf1, 0xf1, 0xf1));
    assert_eq!(ctx.strokes.len(), 1);
    assert_eq!(ctx.strokes[0].color, Color::from_rgb8(0xcc, 0xcc, 0xcc));
}

#[test]
fn zero_border_width_paints_no_outline() {
    let mut ctx = PaintRecorder::new();
    let style = MenuStyle {
        border_width: 0.0,
        ..MenuStyle::default()
    };
    paint_panel(&mut ctx, Rect::new(0.0, 0.0, 100.0, 50.0), &style);
    assert!(ctx.strokes.is_empty());
}

#[test]
fn shortcut_format_plain_spells_ctrl_on_every_platform() {
    let font = test_font();
    let item = MenuItem::action("Redo", "redo").shortcut("Ctrl+Shift+Z");
    for p in [
        Platform::MacOS,
        Platform::Windows,
        Platform::Linux,
        Platform::Other,
    ] {
        assert_eq!(
            item.shortcut_text_formatted(p, &font, ShortcutFormat::Plain)
                .as_deref(),
            Some("Ctrl+Shift+Z")
        );
    }
    // Platform keeps today's per-platform text.
    assert_eq!(
        item.shortcut_text_formatted(Platform::MacOS, &font, ShortcutFormat::Platform),
        item.shortcut_text_for_font(Platform::MacOS, &font)
    );
    // Free-form text is shown as declared under either format.
    let custom = MenuItem::action("Go", "go").shortcut("Hold Fn");
    assert_eq!(
        custom
            .shortcut_text_formatted(Platform::MacOS, &font, ShortcutFormat::Plain)
            .as_deref(),
        Some("Hold Fn")
    );
}

#[test]
fn thread_default_style_reaches_new_popups_and_menu_bars() {
    let _guard = profile_test_lock();
    reset_env();
    let style = agg_sharp_style();
    set_menu_style(style);
    assert_eq!(current_menu_style(), style);
    let popup = PopupMenu::new(items());
    let bar = MenuBar::new(test_font(), Vec::new(), |_| {});
    let layout = root_layout(&mut popup.clone());
    reset_env();
    assert_eq!(popup.style, style);
    assert_eq!(bar.popup.style, style);
    assert_eq!(layout.rows[0].rect.height, 28.0);
    assert_eq!(popup.state.width(), MenuWidth::FitContent);
    // Reset returns new popups to the built-in look.
    assert_eq!(PopupMenu::new(items()).style, MenuStyle::default());
}

#[test]
fn explicit_width_call_after_style_still_wins() {
    let _guard = profile_test_lock();
    reset_env();
    let popup = PopupMenu::new(items())
        .with_style(agg_sharp_style())
        .with_width(MenuWidth::Fixed);
    let width = root_layout(&mut popup.clone()).rect.width;
    assert_eq!(width, MENU_W);
}
