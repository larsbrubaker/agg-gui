//! Tests for [`super::ColorPicker`]'s round-swatch popup mode
//! (`with_round_popup_swatch`) and that the default inline mode keeps its
//! layout.  Driven through the production `layout` / `on_event` /
//! `hit_test_global_overlay` / `paint` paths.

use super::*;
use crate::event::{Key, Modifiers};

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");
const ROOM: Size = Size {
    width: 300.0,
    height: 400.0,
};

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn left(pos: Point, down: bool) -> Event {
    let modifiers = Modifiers::default();
    if down {
        Event::MouseDown {
            pos,
            button: MouseButton::Left,
            modifiers,
        }
    } else {
        Event::MouseUp {
            pos,
            button: MouseButton::Left,
            modifiers,
        }
    }
}

fn click(p: &mut ColorPicker, pos: Point) {
    p.on_event(&left(pos, true));
    p.on_event(&left(pos, false));
    let _ = p.layout(ROOM);
}

fn centre(r: Rect) -> Point {
    Point::new(r.x + r.width * 0.5, r.y + r.height * 0.5)
}

#[test]
fn inline_mode_layout_is_unchanged() {
    let cell = Rc::new(Cell::new(Color::rgb(1.0, 0.0, 0.0)));
    let mut p = ColorPicker::new(cell, font());
    assert_eq!(p.layout(ROOM), Size::new(PANEL_W, SWATCH_H));
    assert!(!p.is_focusable());
    click(&mut p, Point::new(5.0, 5.0));
    assert!(p.is_open());
    assert_eq!(
        p.layout(ROOM),
        Size::new(PANEL_W, SWATCH_H + panel_body_h(false))
    );
    // The panel rows sit inside the widget's own bounds.
    let r = p.regions();
    assert_eq!(r.hue.x, PAD);
    assert_eq!(r.hue.y + r.hue.height, p.bounds.height - SWATCH_H - PAD);
    assert_eq!(r.cancel.y, PAD);
    assert!(!p.hit_test_global_overlay(centre(r.hue)));
}

fn popup_picker(cell: &Rc<Cell<Color>>, selected: &Rc<RefCell<Vec<Color>>>) -> ColorPicker {
    let sel = Rc::clone(selected);
    let mut p = ColorPicker::new(Rc::clone(cell), font())
        .with_round_popup_swatch(24.0)
        .on_select(move |c| sel.borrow_mut().push(c));
    assert_eq!(p.layout(ROOM), Size::new(24.0, 24.0));
    p
}

#[test]
fn round_swatch_opens_a_floating_panel_without_growing() {
    let start = Color::rgb(1.0, 0.0, 0.0);
    let cell = Rc::new(Cell::new(start));
    let selected = Rc::new(RefCell::new(Vec::new()));
    let mut p = popup_picker(&cell, &selected);
    assert!(p.is_focusable());

    click(&mut p, Point::new(12.0, 12.0));
    assert!(p.is_open());
    // The widget stays the swatch; the panel floats below it.
    assert_eq!(p.layout(ROOM), Size::new(24.0, 24.0));
    let r = p.regions();
    assert!(r.panel.y + r.panel.height < 0.0);
    assert_eq!(r.panel.width, PANEL_W);
    assert!(p.hit_test_global_overlay(centre(r.sv)));
    assert!(p.hit_test(centre(r.sv)));
    assert!(!p.hit_test_global_overlay(Point::new(12.0, 12.0)));
    // Sub-widgets are placed in the floating panel.
    assert_eq!(p.children()[p.idx_select].bounds(), r.select);

    // Dragging the hue previews into the cell; the popup stays open.
    click(&mut p, centre(r.hue));
    assert!(p.is_open());
    assert_ne!(p.sync_color_from_hsva(), start);
}

#[test]
fn focus_loss_closes_keeping_the_colour() {
    let start = Color::rgb(1.0, 0.0, 0.0);
    let cell = Rc::new(Cell::new(start));
    let selected = Rc::new(RefCell::new(Vec::new()));
    let mut p = popup_picker(&cell, &selected);
    click(&mut p, Point::new(12.0, 12.0));
    let r = p.regions();
    click(&mut p, centre(r.hue));
    let working = p.sync_color_from_hsva();

    p.on_event(&Event::FocusLost);
    assert!(!p.is_open());
    assert_eq!(cell.get(), working);
    assert_eq!(selected.borrow().as_slice(), &[working]);
    assert!(!p.hit_test_global_overlay(centre(r.hue)));
}

#[test]
fn escape_cancels_and_restores() {
    let start = Color::rgb(1.0, 0.0, 0.0);
    let cell = Rc::new(Cell::new(start));
    let selected = Rc::new(RefCell::new(Vec::new()));
    let mut p = popup_picker(&cell, &selected);
    click(&mut p, Point::new(12.0, 12.0));
    let r = p.regions();
    // A drag previews live into the cell.
    p.on_event(&left(centre(r.sv), true));
    p.on_event(&Event::MouseMove {
        pos: Point::new(r.sv.x + 10.0, r.sv.y + 10.0),
    });
    p.on_event(&left(Point::new(r.sv.x + 10.0, r.sv.y + 10.0), false));
    assert_ne!(cell.get(), start);

    p.on_event(&Event::KeyDown {
        key: Key::Escape,
        modifiers: Modifiers::default(),
    });
    assert!(!p.is_open());
    assert_eq!(cell.get(), start);
    assert!(selected.borrow().is_empty());
}

#[test]
fn select_button_in_the_popup_commits_through_the_picker() {
    let start = Color::rgb(1.0, 0.0, 0.0);
    let cell = Rc::new(Cell::new(start));
    let selected = Rc::new(RefCell::new(Vec::new()));
    let mut p = popup_picker(&cell, &selected);
    click(&mut p, Point::new(12.0, 12.0));
    let r = p.regions();
    click(&mut p, centre(r.hue));
    let working = p.sync_color_from_hsva();
    // The picker claims the pointer and forwards the click to Select.
    assert!(p.claims_pointer_exclusively(centre(r.select)));
    click(&mut p, centre(r.select));
    assert!(!p.is_open());
    assert_eq!(cell.get(), working);
    assert_eq!(selected.borrow().as_slice(), &[working]);
}

#[test]
fn clicking_the_swatch_again_closes() {
    let cell = Rc::new(Cell::new(Color::rgb(0.0, 0.0, 1.0)));
    let selected = Rc::new(RefCell::new(Vec::new()));
    let mut p = popup_picker(&cell, &selected);
    click(&mut p, Point::new(12.0, 12.0));
    assert!(p.is_open());
    click(&mut p, Point::new(12.0, 12.0));
    assert!(!p.is_open());
}

#[test]
fn round_swatch_paints_a_circle_of_the_colour() {
    let blue = Color::rgb(0.0, 0.0, 1.0);
    let cell = Rc::new(Cell::new(blue));
    let mut p = ColorPicker::new(cell, font())
        .with_round_popup_swatch(24.0)
        .with_swatch_outline(Color::rgb(1.0, 0.0, 0.0), 1.0);
    let _ = p.layout(ROOM);
    let mut fb = crate::Framebuffer::new(24, 24);
    {
        let mut ctx = crate::GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        crate::widget::paint_subtree(&mut p, &mut ctx);
    }
    let at = |x: u32, y: u32| {
        let i = ((y * 24 + x) * 4) as usize;
        [fb.pixels()[i], fb.pixels()[i + 1], fb.pixels()[i + 2]]
    };
    assert_eq!(at(12, 12), [0, 0, 255]);
    // Corners are outside the circle.
    assert_eq!(at(0, 0), [0, 0, 0]);
    // The outline is on the circle's edge.
    let edge = at(12, 23);
    assert!(edge[0] > 150, "outline {edge:?}");
}
