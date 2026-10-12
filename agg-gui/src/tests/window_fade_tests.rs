//! Port of the shared-`Window` half of agg-sharp
//! `AggSharpDemo/DemoWindowFadeTests.cs` and `DemoWindowHostTests.cs`.
//!
//! agg-gui's `Window` fades out when it closes and keeps painting until the
//! fade ends; while it fades it must not take the mouse (press, hover, wheel)
//! or keep keyboard focus. C#'s `DemoWindowHost.SetOpen(spec, false)` is the
//! window's `visible_cell` going false (as agg-gui's demo sidebar closes a
//! window); C#'s `window.MouseDown` / `MouseWheel` handlers are a probe child
//! filling the window's content area. The title-bar close button and a theme
//! switch (the window's body, title bar, border and title follow
//! `Visuals`) are driven through the real `App` and paint paths.

use super::*;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect};
use crate::text::Font;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::window::{CloseReason, Window};
use crate::{DrawCtx, Stack, Visuals};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

const VP_W: f64 = 600.0;
const VP_H: f64 = 400.0;
const WIN: Rect = Rect {
    x: 100.0,
    y: 80.0,
    width: 300.0,
    height: 200.0,
};

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).expect("test font"))
}

/// Counts the pointer events that reach the window's content.
#[derive(Default)]
struct Counts {
    downs: Cell<u32>,
    moves: Cell<u32>,
    wheels: Cell<u32>,
}

struct Probe {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    counts: Rc<Counts>,
}

impl Widget for Probe {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
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
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        let c = &self.counts;
        match event {
            Event::MouseDown { .. } => c.downs.set(c.downs.get() + 1),
            Event::MouseMove { .. } => c.moves.set(c.moves.get() + 1),
            Event::MouseWheel { .. } => c.wheels.set(c.wheels.get() + 1),
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }
}

fn app_with(content: Box<dyn Widget>, open: &Rc<Cell<bool>>) -> App {
    let win = Window::new("Fading", font(), content)
        .with_bounds(WIN)
        .with_visible_cell(Rc::clone(open));
    let mut app = App::new(Box::new(Stack::new().add(Box::new(win))));
    app.layout(Size::new(VP_W, VP_H));
    app
}

fn probe_app(open: &Rc<Cell<bool>>) -> (App, Rc<Counts>) {
    let counts = Rc::new(Counts::default());
    let probe = Probe {
        bounds: Rect::default(),
        children: Vec::new(),
        counts: Rc::clone(&counts),
    };
    (app_with(Box::new(probe), open), counts)
}

fn window(app: &App) -> &Window {
    app.root().children()[0]
        .as_any()
        .and_then(|a| a.downcast_ref::<Window>())
        .expect("window")
}

/// Centre of the window's content area, in the App's Y-down screen space.
fn content_center() -> (f64, f64) {
    let x = WIN.x + WIN.width * 0.5;
    let y_up = WIN.y + (WIN.height - 28.0) * 0.5;
    (x, VP_H - y_up)
}

/// C#'s `SetOpen(spec, false)` and the frame that starts the fade: the
/// window reads its cell in layout and starts fading out when it next paints.
fn close_and_fade(app: &mut App, open: &Rc<Cell<bool>>) {
    open.set(false);
    app.layout(Size::new(VP_W, VP_H));
    let mut fb = Framebuffer::new(VP_W as u32, VP_H as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

fn click(app: &mut App, x: f64, y: f64) {
    app.on_mouse_move(x, y);
    app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
}

#[test]
fn a_closing_window_lets_clicks_and_hover_through_while_it_fades() {
    let open = Rc::new(Cell::new(true));
    let (mut app, counts) = probe_app(&open);
    let (x, y) = content_center();

    close_and_fade(&mut app, &open);
    assert!(
        window(&app).is_visible(),
        "the closing window is still drawn while it fades"
    );

    app.on_mouse_move(x, y);
    assert_eq!(
        counts.moves.get(),
        0,
        "a fading window is not under the mouse"
    );
    app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
    assert_eq!(counts.downs.get(), 0);

    // Reopened, it takes the mouse again.
    open.set(true);
    app.layout(Size::new(VP_W, VP_H));
    click(&mut app, x, y);
    assert_eq!(counts.downs.get(), 1);
}

#[test]
fn a_closing_window_takes_no_mouse_wheel() {
    let open = Rc::new(Cell::new(true));
    let (mut app, counts) = probe_app(&open);
    let (x, y) = content_center();

    app.on_mouse_move(x, y);
    app.on_mouse_wheel(x, y, 120.0);
    assert_eq!(counts.wheels.get(), 1);

    close_and_fade(&mut app, &open);
    app.on_mouse_wheel(x, y, 120.0);
    assert_eq!(counts.wheels.get(), 1);
}

#[test]
fn a_closing_window_gives_up_keyboard_focus() {
    let open = Rc::new(Cell::new(true));
    let mut app = app_with(Box::new(TextField::new(font())), &open);
    app.focus_first();
    let focused = app.focused_path().map(<[usize]>::to_vec);
    assert!(
        focused.as_deref().is_some_and(|p| p.starts_with(&[0])),
        "the field inside the window holds focus: {focused:?}"
    );

    close_and_fade(&mut app, &open);
    assert!(window(&app).is_visible(), "still fading");
    assert_eq!(
        app.focused_path(),
        None,
        "the closing window keeps no focus"
    );
}

#[test]
fn the_title_bar_close_button_closes_the_window() {
    let open = Rc::new(Cell::new(true));
    let closes = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&closes);
    let win = Window::new("Gallery", font(), Box::new(SizedBox::new()))
        .with_bounds(WIN)
        .with_visible_cell(Rc::clone(&open))
        .on_close(move |reason| seen.borrow_mut().push(reason));
    let mut app = App::new(Box::new(Stack::new().add(Box::new(win))));
    app.layout(Size::new(VP_W, VP_H));

    // The close button's centre sits 10 px in from the right edge, in the
    // middle of the 28 px title bar.
    let close = Point::new(WIN.x + WIN.width - 10.0, WIN.y + WIN.height - 14.0);
    click(&mut app, close.x, VP_H - close.y);

    assert!(!open.get(), "the window is no longer open");
    assert_eq!(*closes.borrow(), vec![CloseReason::CloseButton]);
}

/// The C# test's ComingSoon text and the page's theme subscription are the
/// demo's; agg-gui's `Window` reads its colours from `Visuals` when it
/// paints, so a theme switch is `set_visuals` and the next paint, and there is
/// no subscription to let go of.
#[test]
fn theme_changes_recolour_windows_and_their_coming_soon_text() {
    let mut win = Window::new("Screen Share", font(), Box::new(SizedBox::new()))
        .with_bounds(Rect::new(20.0, 20.0, 200.0, 140.0));
    win.layout(Size::new(240.0, 180.0));
    let paint = |win: &mut Window| {
        let mut fb = Framebuffer::new(240, 180);
        {
            let mut ctx = GfxCtx::new(&mut fb);
            ctx.clear(Color::rgb(1.0, 0.0, 1.0));
            ctx.save();
            ctx.translate(20.0, 20.0);
            paint_subtree(win, &mut ctx);
            ctx.restore();
        }
        fb
    };
    // Body middle, title bar left of the title text, and the border's
    // outermost pixel on the window's left edge at mid height.
    let body = (120, 60);
    let title = (30, 20 + 140 - 6);
    let border = (20, 60);
    let close = |px: [u8; 4], c: Color, what: &str| {
        let want = [c.r, c.g, c.b].map(|v| (v * 255.0).round() as i32);
        for ch in 0..3 {
            assert!(
                (px[ch] as i32 - want[ch]).abs() <= 24,
                "{what}: got {px:?}, want {want:?}"
            );
        }
    };

    crate::set_visuals(Visuals::dark());
    let dark = paint(&mut win);
    let d = Visuals::dark();
    close(sample(&dark, body.0, body.1), d.window_fill, "dark body");
    close(
        sample(&dark, title.0, title.1),
        d.window_title_fill,
        "dark title bar",
    );

    crate::set_visuals(Visuals::light());
    let light = paint(&mut win);
    let l = Visuals::light();
    close(sample(&light, body.0, body.1), l.window_fill, "light body");
    close(
        sample(&light, title.0, title.1),
        l.window_title_fill,
        "light title bar",
    );
    let edge = sample(&light, border.0, border.1);
    assert_ne!(
        edge,
        sample(&dark, border.0, border.1),
        "the border follows the theme"
    );
    crate::set_visuals(Visuals::dark());
}
