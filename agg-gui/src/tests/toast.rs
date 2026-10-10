//! Toasts (`widgets/toast/`): a `Toasts` handle shows messages that a
//! `ToastHost` paints stacked at a corner over its child.  Covers stacking,
//! auto-dismiss, the hover pause, click to dismiss, fades, kind icons and
//! colours, ellipsis, the max-visible cap, wake-up deadlines, and pointer
//! pass-through to the child.
//!
//! Every test drives the production path — `App` layout, pointer events and
//! `App::paint` into a `PaintRecorder` (seven pixels a character) — on the
//! virtual clock, advancing it instead of sleeping.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use super::paint_recorder::PaintRecorder;
use super::TEST_FONT;
use crate::clock;
use crate::event::{Event, EventResult, Modifiers, MouseButton};
use crate::geometry::{Rect, Size};
use crate::layout_props::Insets;
use crate::text::Font;
use crate::widget::{App, Widget};
use crate::widgets::toast::{
    PaintedToast, Toast, ToastHost, ToastKind, Toasts, DEFAULT_TOAST_DURATION,
    ERROR_TOAST_DURATION, TOAST_FADE_IN, TOAST_FADE_OUT,
};
use crate::widgets::Align2;
use crate::DrawCtx;

const VP: Size = Size {
    width: 400.0,
    height: 300.0,
};
const MS: Duration = Duration::from_millis(1);

/// A viewport-filling child that counts left presses.
struct Probe {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    presses: Rc<Cell<u32>>,
}

impl Widget for Probe {
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
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Event::MouseDown { .. } = event {
            self.presses.set(self.presses.get() + 1);
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }
}

struct Rig {
    app: App,
    toasts: Toasts,
    presses: Rc<Cell<u32>>,
    _clock: clock::ClockGuard,
}

fn rig_with(configure: impl FnOnce(ToastHost) -> ToastHost) -> Rig {
    let clock = clock::scoped_virtual(None);
    let presses = Rc::new(Cell::new(0));
    let probe = Probe {
        bounds: Rect::default(),
        children: Vec::new(),
        presses: Rc::clone(&presses),
    };
    let toasts = Toasts::new();
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("font"));
    let host = configure(ToastHost::new(Box::new(probe), toasts.clone(), font));
    let mut app = App::new(Box::new(host));
    app.layout(VP);
    Rig {
        app,
        toasts,
        presses,
        _clock: clock,
    }
}

fn rig() -> Rig {
    rig_with(|h| h)
}

impl Rig {
    fn paint(&mut self) -> PaintRecorder {
        let mut rec = PaintRecorder::new();
        self.app.paint(&mut rec);
        rec
    }

    fn painted(&self) -> Vec<PaintedToast> {
        self.app
            .root()
            .as_any()
            .and_then(|a| a.downcast_ref::<ToastHost>())
            .expect("host")
            .painted()
            .to_vec()
    }

    /// Advance the clock by `d` and paint.
    fn after(&mut self, d: Duration) -> PaintRecorder {
        clock::advance(d);
        self.paint()
    }

    /// Move the pointer to the centre of painted toast `i` (newest first).
    fn hover_toast(&mut self, i: usize) {
        let r = self.painted()[i].rect;
        self.hover(r.x + r.width * 0.5, r.y + r.height * 0.5);
    }

    /// Move the pointer to logical `(x, y)`, Y-up.
    fn hover(&mut self, x: f64, y: f64) {
        self.app.on_mouse_move(x, VP.height - y);
    }

    fn click(&mut self, x: f64, y: f64) {
        let m = Modifiers::default();
        self.app
            .on_mouse_down(x, VP.height - y, MouseButton::Left, m);
        self.app.on_mouse_up(x, VP.height - y, MouseButton::Left, m);
    }
}

fn texts(rec: &PaintRecorder) -> Vec<&str> {
    rec.texts.iter().map(|(t, _, _)| t.as_str()).collect()
}

#[test]
fn toasts_stack_at_the_bottom_right_newest_lowest() {
    let mut r = rig();
    r.toasts.show("first");
    r.toasts.show("second");
    r.toasts.show("third");
    let rec = r.after(TOAST_FADE_IN);
    assert_eq!(texts(&rec), ["third", "second", "first"]);
    let p = r.painted();
    assert_eq!(p.len(), 3);
    for t in &p {
        let right = t.rect.x + t.rect.width;
        assert!((right - (VP.width - 16.0)).abs() < 1e-9, "{t:?}");
        assert!((t.alpha - 1.0).abs() < 1e-9, "faded in: {t:?}");
    }
    assert!(
        (p[0].rect.y - 16.0).abs() < 1e-9,
        "newest at the bottom inset"
    );
    for pair in p.windows(2) {
        assert!(
            pair[1].rect.y >= pair[0].rect.y + pair[0].rect.height,
            "stacked upward without overlap: {pair:?}"
        );
    }
}

#[test]
fn anchor_and_inset_place_the_stack() {
    let mut r = rig_with(|h| {
        h.with_anchor(Align2::LEFT_TOP)
            .with_inset(Insets::from_sides(10.0, 10.0, 20.0, 44.0))
    });
    r.toasts.show("one");
    r.toasts.show("two");
    r.after(TOAST_FADE_IN);
    let p = r.painted();
    assert_eq!(p[0].text, "two", "newest nearest the corner");
    assert!((p[0].rect.x - 10.0).abs() < 1e-9);
    let top = p[0].rect.y + p[0].rect.height;
    assert!((top - (VP.height - 20.0)).abs() < 1e-9, "{:?}", p[0]);
    assert!(p[1].rect.y + p[1].rect.height <= p[0].rect.y);
}

#[test]
fn a_toast_fades_in_expires_then_fades_out() {
    let mut r = rig();
    r.toasts.show("saved");
    r.paint();
    let a0 = r.painted()[0].alpha;
    assert!(a0 < 0.05, "starts transparent: {a0}");
    r.after(TOAST_FADE_IN / 2);
    let mid = r.painted()[0].alpha;
    assert!(mid > a0 && mid < 1.0, "fading in: {mid}");
    r.after(TOAST_FADE_IN);
    assert_eq!(r.painted()[0].alpha, 1.0);

    // Just before the countdown ends it is fully up; just after it fades.
    let elapsed = TOAST_FADE_IN / 2 + TOAST_FADE_IN;
    r.after(DEFAULT_TOAST_DURATION - elapsed - MS);
    assert_eq!(r.painted()[0].alpha, 1.0);
    r.after(TOAST_FADE_OUT / 2);
    let out = r.painted()[0].alpha;
    assert!(out > 0.0 && out < 1.0, "fading out: {out}");
    r.after(TOAST_FADE_OUT);
    assert!(r.painted().is_empty());
    assert!(r.toasts.is_empty());
}

#[test]
fn errors_stay_longer_and_sticky_toasts_stay() {
    let mut r = rig();
    r.toasts.show_kind(ToastKind::Error, "failed");
    r.toasts.show_toast(Toast::new("pinned").sticky());
    r.after(DEFAULT_TOAST_DURATION + TOAST_FADE_OUT + MS);
    assert_eq!(r.toasts.texts(), ["failed", "pinned"]);
    r.after(ERROR_TOAST_DURATION - DEFAULT_TOAST_DURATION);
    assert_eq!(r.toasts.texts(), ["pinned"]);
    r.after(Duration::from_secs(60));
    assert_eq!(r.toasts.texts(), ["pinned"]);
}

#[test]
fn hovering_pauses_the_countdown() {
    let mut r = rig();
    r.toasts.show("hover me");
    r.after(TOAST_FADE_IN);
    r.hover_toast(0);
    assert!(r.toasts.is_paused());
    r.after(DEFAULT_TOAST_DURATION * 3);
    assert_eq!(r.painted().len(), 1, "paused while hovered");
    assert_eq!(r.painted()[0].alpha, 1.0);

    // Off the toast: the countdown resumes where it stopped.
    r.hover(20.0, 250.0);
    assert!(!r.toasts.is_paused());
    let left = DEFAULT_TOAST_DURATION - TOAST_FADE_IN;
    r.after(left - MS);
    assert_eq!(r.painted()[0].alpha, 1.0, "the remaining time is kept");
    r.after(TOAST_FADE_OUT + MS * 2);
    assert!(r.toasts.is_empty());
}

#[test]
fn clicking_a_toast_dismisses_it_without_reaching_the_child() {
    let mut r = rig();
    r.toasts.show("keep");
    r.toasts.show("dismiss me");
    r.after(TOAST_FADE_IN);
    let t = r.painted()[0].clone();
    assert_eq!(t.text, "dismiss me");
    r.click(t.rect.x + 5.0, t.rect.y + 5.0);
    assert_eq!(r.presses.get(), 0, "the toast took the press");
    r.after(TOAST_FADE_OUT / 2);
    let p = r.painted();
    let dismissed = p.iter().find(|p| p.id == t.id).expect("still fading");
    assert!(dismissed.alpha < 1.0);
    r.after(TOAST_FADE_OUT);
    assert_eq!(r.toasts.texts(), ["keep"]);

    // Off the toasts the child gets the pointer.
    r.click(20.0, 250.0);
    assert_eq!(r.presses.get(), 1);
}

#[test]
fn kind_toasts_paint_their_icon_and_theme_accent() {
    let mut r = rig();
    r.toasts.show_kind(ToastKind::Success, "done");
    let rec = r.after(TOAST_FADE_IN);
    let icon = ToastKind::Success.icon().to_string();
    assert_eq!(texts(&rec), [icon.as_str(), "done"]);
    let accent = ToastKind::Success.accent(&crate::theme::current_visuals());
    assert_eq!(accent, crate::theme::current_visuals().success_color());
    // The recorder counts shape fills (text fills aren't recorded): the
    // accent stripe.
    assert_eq!(
        rec.fills_with(accent),
        1,
        "accent stripe in the kind colour"
    );
    // A plain toast has no icon.
    r.toasts.dismiss_all();
    r.after(TOAST_FADE_OUT + MS);
    r.toasts.show("plain");
    let rec = r.after(TOAST_FADE_IN);
    assert_eq!(texts(&rec), ["plain"]);
    let v = crate::theme::current_visuals();
    assert_eq!(ToastKind::Info.accent(&v), v.accent);
}

#[test]
fn long_text_ends_in_an_ellipsis_within_the_max_width() {
    let mut r = rig_with(|h| h.with_max_toast_width(200.0));
    let long = "Moved a very long file name indeed.txt to the Trash";
    r.toasts.show(long);
    r.after(TOAST_FADE_IN);
    let t = &r.painted()[0];
    assert!(t.text.ends_with("..."), "{}", t.text);
    assert!(t.rect.width <= 200.0, "{:?}", t.rect);
}

#[test]
fn the_oldest_toasts_close_beyond_max_visible() {
    let mut r = rig();
    r.toasts.set_max_visible(2);
    for i in 0..4 {
        r.toasts.show(format!("t{i}"));
    }
    r.after(TOAST_FADE_OUT + MS);
    assert_eq!(r.toasts.texts(), ["t2", "t3"]);
}

#[test]
fn the_host_asks_for_frames_while_fading_and_wakes_at_expiry() {
    let mut r = rig();
    r.toasts.show("hi");
    r.paint();
    assert!(r.app.root().needs_draw(), "fading in");
    assert!(crate::animation::wants_draw());
    r.after(TOAST_FADE_IN);
    assert!(!r.app.root().needs_draw(), "settled");
    let now = clock::now();
    let want = now + DEFAULT_TOAST_DURATION - TOAST_FADE_IN;
    assert_eq!(r.app.root().next_draw_deadline(), Some(want));
    assert_eq!(crate::animation::peek_next_draw_deadline(), Some(want));
}

#[test]
fn a_dismissed_toast_under_the_pointer_releases_the_pause() {
    let mut r = rig();
    let id = r.toasts.show("only");
    r.after(TOAST_FADE_IN);
    r.hover_toast(0);
    assert!(r.toasts.is_paused());
    r.toasts.dismiss(id);
    r.after(TOAST_FADE_OUT + MS);
    assert!(r.toasts.is_empty());
    assert!(!r.toasts.is_paused(), "nothing left under the pointer");
}

/// Real rasterization: the kind icon (drawn with the icon font) comes out in
/// the kind's accent colour, right of the stripe.  Pixel colour only — which
/// glyph is drawn is covered by `kind_toasts_paint_their_icon_and_theme_accent`.
#[test]
fn kind_icon_renders_in_the_accent_colour() {
    use crate::framebuffer::Framebuffer;
    use crate::gfx_ctx::GfxCtx;
    let fa = Arc::new(Font::from_slice(crate::fonts::FONT_AWESOME_4_7).expect("fa"));
    let mut r = rig_with(|h| h.with_icon_font(fa));
    r.toasts.show_kind(ToastKind::Error, "boom");
    clock::advance(TOAST_FADE_IN);
    let mut fb = Framebuffer::new(VP.width as u32, VP.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    r.app.paint(&mut ctx);
    drop(ctx);
    let rect = r.painted()[0].rect;
    let c = crate::theme::current_visuals().error_color();
    let (cr, cg, cb) = (
        (c.r * 255.0) as i32,
        (c.g * 255.0) as i32,
        (c.b * 255.0) as i32,
    );
    let px = fb.pixels();
    let mut icon_px = 0;
    // Right of the stripe and its padding: only the icon is accent-coloured.
    for y in rect.y as u32..(rect.y + rect.height) as u32 {
        for x in (rect.x + 8.0) as u32..(rect.x + rect.width) as u32 {
            let i = ((y * fb.width() + x) * 4) as usize;
            let close = |v: u8, w: i32| (v as i32 - w).abs() < 40;
            if close(px[i], cr) && close(px[i + 1], cg) && close(px[i + 2], cb) {
                icon_px += 1;
            }
        }
    }
    assert!(
        icon_px > 20,
        "icon painted in the error colour ({icon_px} px)"
    );
}
