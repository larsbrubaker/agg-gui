//! `Opacity` — a wrapped subtree paints at its shared opacity: unchanged at 1,
//! blended once at 0.5, not at all at 0 (global overlays included); at 0 the
//! pointer passes through to what is behind and keyboard focus leaves, while
//! layout keeps the subtree's space at every opacity.

use super::*;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::Rect;
use crate::text::Font;
use crate::widgets::{Opacity, SizedBox, Stack, TextField};

/// A full-size red block that counts its presses and paints.
struct Probe {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    color: Color,
    presses: Rc<Cell<u32>>,
    overlays: Rc<Cell<u32>>,
    animating: bool,
}

impl Probe {
    fn new(color: Color) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            color,
            presses: Rc::new(Cell::new(0)),
            overlays: Rc::new(Cell::new(0)),
            animating: false,
        }
    }
}

impl Widget for Probe {
    fn type_name(&self) -> &'static str {
        "Probe"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        available
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_fill_color(self.color);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
    }
    fn paint_global_overlay(&mut self, _ctx: &mut dyn DrawCtx) {
        self.overlays.set(self.overlays.get() + 1);
    }
    fn needs_draw(&self) -> bool {
        self.animating
    }
    fn on_event(&mut self, e: &Event) -> EventResult {
        if matches!(e, Event::MouseDown { .. }) {
            self.presses.set(self.presses.get() + 1);
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }
}

const SIZE: u32 = 20;

/// Paint `app` over white into a fresh framebuffer and return its centre pixel.
fn centre_after_paint(app: &mut App) -> [u8; 4] {
    let mut fb = Framebuffer::new(SIZE, SIZE);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::white());
    app.paint(&mut ctx);
    drop(ctx);
    sample(&fb, SIZE / 2, SIZE / 2)
}

fn red() -> Color {
    Color::rgba(1.0, 0.0, 0.0, 1.0)
}

fn red_app(opacity: &Rc<Cell<f64>>) -> (App, Rc<Cell<u32>>) {
    let probe = Probe::new(red());
    let overlays = Rc::clone(&probe.overlays);
    let mut app = App::new(Box::new(Opacity::new(Rc::clone(opacity), Box::new(probe))));
    app.layout(Size::new(SIZE as f64, SIZE as f64));
    (app, overlays)
}

#[test]
fn opaque_paints_the_child_unchanged_with_no_layer() {
    let opacity = Rc::new(Cell::new(1.0));
    let (mut app, overlays) = red_app(&opacity);
    assert_eq!(centre_after_paint(&mut app), [255, 0, 0, 255]);
    assert_eq!(overlays.get(), 1);
    let mut w = Opacity::new(Rc::clone(&opacity), Box::new(Probe::new(red())));
    assert!(w.compositing_layer().is_none(), "1.0 costs no layer");
    // Out-of-range and NaN values read as their clamp / opaque.
    opacity.set(f64::NAN);
    assert!(w.compositing_layer().is_none());
    assert_eq!(w.opacity(), 1.0);
    opacity.set(3.0);
    assert_eq!(w.opacity(), 1.0);
}

#[test]
fn half_opacity_blends_the_child_once_over_what_is_behind() {
    let opacity = Rc::new(Cell::new(0.5));
    let (mut app, _) = red_app(&opacity);
    let [r, g, b, _] = centre_after_paint(&mut app);
    // Red at 0.5 over white: red stays full, green and blue land half way.
    assert_eq!(r, 255);
    assert!((120..=136).contains(&g), "green half way; got {g}");
    assert!((120..=136).contains(&b), "blue half way; got {b}");
}

#[test]
fn zero_opacity_paints_nothing_not_even_global_overlays() {
    let opacity = Rc::new(Cell::new(0.0));
    let (mut app, overlays) = red_app(&opacity);
    assert_eq!(centre_after_paint(&mut app), [255, 255, 255, 255]);
    assert_eq!(overlays.get(), 0, "no global overlay at 0");
    // The same tree comes back as the source changes, with no rebuild.
    opacity.set(1.0);
    assert_eq!(centre_after_paint(&mut app), [255, 0, 0, 255]);
}

#[test]
fn from_fn_reads_its_source_on_every_paint() {
    let t = Rc::new(Cell::new(0.0));
    let source = Rc::clone(&t);
    let w = Opacity::from_fn(move || source.get() * 0.5, Box::new(Probe::new(red())));
    let mut app = App::new(Box::new(w));
    app.layout(Size::new(SIZE as f64, SIZE as f64));
    assert_eq!(centre_after_paint(&mut app), [255, 255, 255, 255]);
    t.set(2.0);
    assert_eq!(centre_after_paint(&mut app), [255, 0, 0, 255]);
}

/// A backdrop under a faded panel, both filling a 20 x 20 stack.
fn stacked(opacity: &Rc<Cell<f64>>) -> (App, Rc<Cell<u32>>, Rc<Cell<u32>>) {
    let back = Probe::new(Color::rgba(0.0, 0.0, 1.0, 1.0));
    let front = Probe::new(red());
    let (back_hits, front_hits) = (Rc::clone(&back.presses), Rc::clone(&front.presses));
    let stack = Stack::new()
        .add(Box::new(back))
        .add(Box::new(Opacity::new(Rc::clone(opacity), Box::new(front))));
    let mut app = App::new(Box::new(stack));
    app.layout(Size::new(SIZE as f64, SIZE as f64));
    (app, back_hits, front_hits)
}

fn press(app: &mut App) {
    let c = SIZE as f64 / 2.0;
    app.on_mouse_move(c, c);
    app.on_mouse_down(c, c, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(c, c, MouseButton::Left, Modifiers::default());
}

#[test]
fn a_partly_faded_panel_takes_the_pointer_and_a_faded_out_one_passes_it_through() {
    let opacity = Rc::new(Cell::new(1.0));
    let (mut app, back, front) = stacked(&opacity);
    press(&mut app);
    assert_eq!((back.get(), front.get()), (0, 1));
    opacity.set(0.5);
    press(&mut app);
    assert_eq!((back.get(), front.get()), (0, 2), "usable while fading in");
    opacity.set(0.0);
    press(&mut app);
    assert_eq!((back.get(), front.get()), (1, 2), "passes through at 0");
}

#[test]
fn a_faded_out_panel_lets_go_of_focus_and_tab_skips_it() {
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let opacity = Rc::new(Cell::new(1.0));
    let field = TextField::new(font);
    let mut app = App::new(Box::new(Opacity::new(Rc::clone(&opacity), Box::new(field))));
    app.layout(Size::new(200.0, 40.0));
    app.focus_first();
    assert!(app.focused_path().is_some(), "the field takes focus at 1");
    opacity.set(0.0);
    app.layout(Size::new(200.0, 40.0));
    assert!(app.focused_path().is_none(), "focus leaves at 0");
    app.focus_first();
    assert!(app.focused_path().is_none(), "Tab cannot reach it at 0");
}

#[test]
fn a_faded_out_subtree_keeps_its_space_and_asks_for_no_frames() {
    let opacity = Rc::new(Cell::new(1.0));
    let mut probe = Probe::new(red());
    probe.animating = true;
    let faded = Opacity::new(
        Rc::clone(&opacity),
        Box::new(
            SizedBox::new()
                .with_height(30.0)
                .with_child(Box::new(probe)),
        ),
    );
    let mut col = FlexColumn::new();
    col.push(Box::new(faded), 0.0);
    col.push(Box::new(SizedBox::new().with_height(10.0)), 0.0);
    let mut app = App::new(Box::new(col));
    app.layout(Size::new(100.0, 100.0));
    let below_at_one = app.root().children()[1].bounds();
    assert!(app.root().needs_draw());
    opacity.set(0.0);
    app.layout(Size::new(100.0, 100.0));
    assert_eq!(app.root().children()[1].bounds(), below_at_one);
    assert!(
        !app.root().needs_draw(),
        "an invisible animation draws nothing"
    );
}
