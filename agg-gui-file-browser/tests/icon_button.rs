//! The browser's square icon button (`BrowserIconButton`, the breadcrumb's up button): a press
//! survives the pointer leaving and coming back, disabling drops a press, its tooltip is the
//! widget's own so any holder can change it, and the hover fill paints under the pointer.

use std::cell::Cell;
use std::rc::Rc;

use agg_gui::widget::paint_subtree;
use agg_gui::{
    Color, Event, Framebuffer, GfxCtx, Modifiers, MouseButton, Point, Rect, Size, Widget,
};
use agg_gui_file_browser::{
    BrowserIcon, BrowserIconButton, BrowserLook, BrowserNames, BrowserStyle,
};

const SIDE: f64 = 32.0;
const HOVER: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 1.0,
    a: 1.0,
};

/// The default style with an opaque blue hover fill.
struct BlueHoverLook {
    names: BrowserNames,
}

impl BrowserLook for BlueHoverLook {
    fn style(&self) -> BrowserStyle {
        BrowserStyle {
            hover_fill: HOVER,
            ..BrowserStyle::default()
        }
    }
    fn names(&self) -> &BrowserNames {
        &self.names
    }
}

/// A laid-out up button counting its clicks.
fn button() -> (BrowserIconButton, Rc<Cell<u32>>) {
    agg_gui::set_device_scale(1.0);
    let look: Rc<dyn BrowserLook> = Rc::new(BlueHoverLook {
        names: BrowserNames::default(),
    });
    let clicks = Rc::new(Cell::new(0));
    let count = Rc::clone(&clicks);
    let mut button = BrowserIconButton::new(look, BrowserIcon::Up, SIDE)
        .on_click(move || count.set(count.get() + 1));
    let size = button.layout(Size::new(SIDE, SIDE));
    button.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    (button, clicks)
}

const INSIDE: Point = Point { x: 10.0, y: 10.0 };
const OUTSIDE: Point = Point { x: 50.0, y: 10.0 };

fn down(widget: &mut dyn Widget, pos: Point) {
    widget.on_event(&Event::MouseDown {
        pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
}

fn up(widget: &mut dyn Widget, pos: Point) {
    widget.on_event(&Event::MouseUp {
        pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
}

#[test]
fn a_press_dragged_off_and_back_still_clicks() {
    let (mut button, clicks) = button();
    down(&mut button, INSIDE);
    button.on_event(&Event::MouseMove { pos: OUTSIDE });
    button.on_event(&Event::MouseLeave);
    button.on_event(&Event::MouseEnter);
    button.on_event(&Event::MouseMove { pos: INSIDE });
    up(&mut button, INSIDE);
    assert_eq!(clicks.get(), 1);
}

#[test]
fn a_release_outside_does_not_click() {
    let (mut button, clicks) = button();
    down(&mut button, INSIDE);
    up(&mut button, OUTSIDE);
    assert_eq!(clicks.get(), 0);
}

#[test]
fn disabling_mid_press_drops_the_press() {
    let (mut button, clicks) = button();
    down(&mut button, INSIDE);
    button.set_enabled(false);
    button.set_enabled(true);
    up(&mut button, INSIDE);
    assert_eq!(
        clicks.get(),
        0,
        "the press began before the button was disabled"
    );
    down(&mut button, INSIDE);
    up(&mut button, INSIDE);
    assert_eq!(clicks.get(), 1, "a fresh press clicks");
}

#[test]
fn the_tooltip_is_the_widgets_own() {
    let (button, _) = button();
    let button = button.with_tooltip_text("Click to go back");
    assert_eq!(button.tooltip_text(), Some("Click to go back"));
    let mut boxed: Box<dyn Widget> = Box::new(button);
    boxed.set_tooltip_text(Some("Up one folder".into()));
    assert_eq!(boxed.tooltip_text(), Some("Up one folder"));
    let (other, _) = self::button();
    let other = other.with_tooltip("Back");
    assert_eq!(other.tooltip_text(), Some("Back"));
}

/// The button painted alone: RGBA, top row first.
fn paint(button: &mut BrowserIconButton) -> Vec<u8> {
    let mut fb = Framebuffer::new(SIDE as u32, SIDE as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        paint_subtree(button, &mut ctx);
    }
    fb.pixels_flipped()
}

fn corner(pixels: &[u8]) -> [u8; 4] {
    [pixels[0], pixels[1], pixels[2], pixels[3]]
}

#[test]
fn the_hover_fill_paints_only_under_the_pointer() {
    let (mut button, _) = button();
    assert_eq!(corner(&paint(&mut button))[3], 0, "no fill until hovered");
    button.on_event(&Event::MouseMove { pos: INSIDE });
    assert_eq!(corner(&paint(&mut button)), [0, 0, 255, 255]);
    button.on_event(&Event::MouseMove { pos: OUTSIDE });
    assert_eq!(
        corner(&paint(&mut button))[3],
        0,
        "gone once the pointer moves off"
    );
    button.on_event(&Event::MouseMove { pos: INSIDE });
    button.on_event(&Event::MouseLeave);
    assert_eq!(
        corner(&paint(&mut button))[3],
        0,
        "gone once the pointer leaves"
    );
}

#[test]
fn a_disabled_button_shows_no_hover_fill() {
    let (mut button, _) = button();
    button.on_event(&Event::MouseMove { pos: INSIDE });
    button.set_enabled(false);
    button.on_event(&Event::MouseMove { pos: INSIDE });
    assert_eq!(corner(&paint(&mut button))[3], 0);
}
