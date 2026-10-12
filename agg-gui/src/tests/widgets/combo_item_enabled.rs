//! `ComboBox::set_item_enabled` (`combo_box/item_enabled.rs`): a disabled
//! row is skipped by the keyboard, never picked by Enter, takes no hover and
//! paints its text dim.  The click half (a press on a disabled row leaves
//! the list open) is agg-sharp's `MenuTests.OpenAndCloseMenus`
//! (`menu_tests.rs`).

use super::*;
use crate::text::Font;
use crate::{ComboBox, Key};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

fn focused(combo: ComboBox) -> App {
    let mut app = App::new(Box::new(combo));
    app.layout(Size::new(160.0, 200.0));
    app.focus_first();
    app
}

fn combo(app: &App) -> &ComboBox {
    app.root()
        .as_any()
        .and_then(|a| a.downcast_ref::<ComboBox>())
        .expect("the root is the combo")
}

fn down(app: &mut App, key: Key) {
    app.on_key_down(key, Modifiers::default());
}

#[test]
fn rust_only_keys_never_pick_a_disabled_row() {
    let changes = Rc::new(Cell::new(0));
    let count = Rc::clone(&changes);
    let mut app = focused(
        ComboBox::new(vec!["Apple", "Banana", "Cherry"], 0, font())
            .with_item_enabled(1, false)
            .on_change(move |_| count.set(count.get() + 1)),
    );
    assert!(!combo(&app).item_enabled(1));
    assert!(combo(&app).item_enabled(0) && combo(&app).item_enabled(2));

    // Down from Apple skips Banana
    down(&mut app, Key::Enter);
    assert!(combo(&app).is_open());
    down(&mut app, Key::ArrowDown);
    down(&mut app, Key::Enter);
    assert_eq!(combo(&app).selected(), 2);
    assert_eq!(changes.get(), 1);

    // and Up from Cherry skips it too
    down(&mut app, Key::Enter);
    down(&mut app, Key::ArrowUp);
    down(&mut app, Key::Enter);
    assert_eq!(combo(&app).selected(), 0);
    assert_eq!(changes.get(), 2);

    // Type-ahead does not land on it: "b" matches nothing enabled, so Enter
    // picks the row the highlight was already on.
    down(&mut app, Key::Enter);
    down(&mut app, Key::Char('b'));
    down(&mut app, Key::Enter);
    assert_eq!(combo(&app).selected(), 0);
    assert_eq!(changes.get(), 2);
    assert!(!combo(&app).is_open());
}

#[test]
fn rust_only_enter_on_a_disabled_selected_row_keeps_the_list_open() {
    // The selected row itself is disabled: the list opens highlighting it,
    // and Enter must not pick it.
    let mut app =
        focused(ComboBox::new(vec!["Apple", "Banana"], 1, font()).with_item_enabled(1, false));
    down(&mut app, Key::Enter);
    down(&mut app, Key::Enter);
    assert!(combo(&app).is_open(), "nothing was picked");
    // Re-enabling the row lets Enter pick it again.
    let mut app = focused(
        ComboBox::new(vec!["Apple", "Banana"], 1, font())
            .with_item_enabled(1, false)
            .with_item_enabled(1, true),
    );
    down(&mut app, Key::Enter);
    down(&mut app, Key::Enter);
    assert!(!combo(&app).is_open());
}

/// Red channel of the brightest pixel in row band `y0..y1`, left of x 60.
fn brightest_text(fb: &Framebuffer, y0: u32, y1: u32) -> u8 {
    let mut max = 0;
    for y in y0..y1 {
        for x in 8..60 {
            max = max.max(fb.pixels()[((y * fb.width() + x) * 4) as usize]);
        }
    }
    max
}

#[test]
fn rust_only_a_disabled_row_takes_no_hover_and_paints_dim() {
    use crate::widgets::combo_box::ComboBoxStateStyle;
    let style = ComboBoxStateStyle {
        popup_fill: Some(Color::black()),
        item_text: Some(Color::white()),
        item_hover_fill: Some(Color::rgb(1.0, 0.0, 0.0)),
        ..Default::default()
    };
    let combo_box = ComboBox::new(vec!["Alpha", "Beta", "Gamma"], 0, font())
        .with_state_style(style)
        .with_item_enabled(1, false);
    let mut app = App::new(Box::new(combo_box));
    app.layout(Size::new(160.0, 200.0));
    app.on_mouse_down(12.0, 188.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(12.0, 188.0, MouseButton::Left, Modifiers::default());
    // Row 1 of three, the popup above the 24 px box: y_up 46..68.
    app.on_mouse_move(140.0, 200.0 - 57.0);
    let mut fb = Framebuffer::new(160, 200);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        app.paint(&mut ctx);
    }
    let i = ((57 * fb.width() + 140) * 4) as usize;
    assert_eq!(
        &fb.pixels()[i..i + 3],
        &[0, 0, 0],
        "no hover fill on a disabled row"
    );
    // White text at alpha 50 over black: dim, while Gamma's is full white.
    let disabled = brightest_text(&fb, 50, 64);
    let enabled = brightest_text(&fb, 28, 42);
    assert!(enabled > 200, "enabled row text {enabled}");
    assert!(
        disabled > 20 && disabled < 80,
        "disabled row text {disabled}"
    );
}
