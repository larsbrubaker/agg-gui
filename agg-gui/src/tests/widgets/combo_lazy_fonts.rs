//! `ComboBox::with_item_font_fn`: a row's font is asked for the first time
//! that row paints in the open list — never while the combo is built or
//! closed, never for rows scrolled out of view, and once per row.

use super::*;
use crate::text::Font;
use crate::ComboBox;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

const ROWS: usize = 20;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

fn options() -> Vec<String> {
    (0..ROWS).map(|i| format!("Row {i}")).collect()
}

/// A lazy combo whose resolver records every row it is asked about.
fn lazy_combo(row_font: Arc<Font>) -> (ComboBox, Rc<RefCell<Vec<usize>>>) {
    let asked = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&asked);
    let combo = ComboBox::new(options(), 0, font()).with_item_font_fn(move |i| {
        log.borrow_mut().push(i);
        Some(Arc::clone(&row_font))
    });
    (combo, asked)
}

fn property(app: &App, key: &str) -> String {
    app.root()
        .properties()
        .into_iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
        .unwrap_or_default()
}

fn root_combo(app: &App) -> &ComboBox {
    app.root()
        .as_any()
        .and_then(|a| a.downcast_ref::<ComboBox>())
        .expect("the root is the combo")
}

fn paint(app: &mut App, viewport: Size) {
    let mut fb = Framebuffer::new(viewport.width as u32, viewport.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

fn click(app: &mut App, x: f64, screen_y: f64) {
    app.on_mouse_down(x, screen_y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(x, screen_y, MouseButton::Left, Modifiers::default());
}

#[test]
fn test_combo_lazy_item_fonts_resolve_only_painted_rows_once() {
    crate::set_scroll_style(crate::widgets::ScrollBarStyle::default());
    let row_font = font();
    let (combo, asked) = lazy_combo(Arc::clone(&row_font));
    let mut app = App::new(Box::new(combo));
    let viewport = Size::new(180.0, 120.0);
    app.layout(viewport);

    // Built and painted closed: nothing asked.
    paint(&mut app, viewport);
    assert!(
        asked.borrow().is_empty(),
        "a closed combo asks for no row font"
    );
    assert!(root_combo(&app).item_font(0).is_none());

    click(&mut app, 12.0, viewport.height - 12.0);
    assert_eq!(property(&app, "open"), "true");
    paint(&mut app, viewport);

    let visible: usize = property(&app, "popup_visible_count").parse().unwrap();
    assert!(visible < ROWS, "the list must be taller than its popup");
    assert_eq!(*asked.borrow(), (0..visible).collect::<Vec<_>>());
    let c = root_combo(&app);
    assert!(Arc::ptr_eq(&c.item_font(0).unwrap(), &row_font));
    assert!(c.item_font_requested(visible - 1));
    assert!(!c.item_font_requested(visible));
    assert!(
        c.item_font(ROWS - 1).is_none(),
        "the last row never painted"
    );

    // Painting again asks nothing new.
    paint(&mut app, viewport);
    assert_eq!(asked.borrow().len(), visible);

    // Scrolling down reveals rows, which are asked about as they paint.
    app.on_mouse_wheel(12.0, viewport.height - 101.0, -44.0);
    paint(&mut app, viewport);
    let first: usize = property(&app, "scroll_offset").parse().unwrap();
    assert!(first > 0, "the wheel scrolled the list");
    // Rows the scroll jumped past never painted, so were never asked about.
    let mut expected: Vec<usize> = (0..visible).collect();
    expected.extend(first.max(visible)..first + visible);
    assert_eq!(*asked.borrow(), expected);
    assert!(first > visible, "the jump skips rows, which stay unasked");
    assert!(!root_combo(&app).item_font_requested(visible));
}

#[test]
fn test_combo_lazy_item_font_none_keeps_the_combo_font_and_is_asked_once() {
    let asked = Rc::new(RefCell::new(0_usize));
    let log = Rc::clone(&asked);
    let combo = ComboBox::new(vec!["Only"], 0, font()).with_item_font_fn(move |_| {
        *log.borrow_mut() += 1;
        None
    });
    let mut app = App::new(Box::new(combo));
    let viewport = Size::new(180.0, 120.0);
    app.layout(viewport);
    click(&mut app, 12.0, viewport.height - 12.0);
    paint(&mut app, viewport);
    paint(&mut app, viewport);
    assert_eq!(*asked.borrow(), 1);
    assert!(root_combo(&app).item_font_requested(0));
    assert!(root_combo(&app).item_font(0).is_none());
}

#[test]
fn test_combo_eager_item_fonts_replace_the_lazy_resolver() {
    let eager = font();
    let (combo, asked) = lazy_combo(font());
    let combo = combo.with_item_fonts(vec![Arc::clone(&eager); ROWS]);
    let mut app = App::new(Box::new(combo));
    let viewport = Size::new(180.0, 120.0);
    app.layout(viewport);
    click(&mut app, 12.0, viewport.height - 12.0);
    paint(&mut app, viewport);
    assert!(asked.borrow().is_empty());
    assert!(Arc::ptr_eq(&root_combo(&app).item_font(3).unwrap(), &eager));
}
