//! Shared harness for the on-screen-keyboard *lift* tests:
//! `keyboard_lift_overlays.rs`, `keyboard_lift_tooltips.rs`,
//! `keyboard_lift_popups.rs`, `keyboard_lift_offscreen.rs` and the lifted
//! cases in `window_snap_coords.rs`.
//!
//! # The lift contract those tests pin
//!
//! While the on-screen keyboard is up, `App::paint` translates the whole tree
//! (and every global overlay queue drain) up by the keyboard's *lift* `L`
//! (`widget::keyboard_scroll`), and every pointer event has `L` subtracted
//! (`keyboard_scroll::lift_to_world`). So:
//!
//! * **Root logical space is UNLIFTED layout space.** Layout bounds,
//!   `current_mouse_world`, `event_root_transform`,
//!   `logical_root_transform(ctx)`, `PopupMenu` root coords and every
//!   overlay / popup request queue are all in it.
//! * The App applies the lift exactly **once**, at paint: something at root
//!   `(x, y)` shows on screen at `(x, y + L)`.
//! * While lifted, the on-screen part of root space is `(0, −L, w, h)`, so
//!   every viewport clamp / flip must behave exactly as with `L = 0`, measured
//!   in on-screen coordinates.
//!
//! The framebuffer a test paints into is the screen: a root point `(x, y)` is
//! drawn at framebuffer logical `(x, y + L)`. To press or hover on-screen
//! logical `(x, ys)` at effective scale `s` in a viewport `H` tall, send
//! physical `(x·s, (H − ys)·s)`; the event then lands at root `(x, ys − L)`.
//!
//! The lift is pinned with [`LiftGuard`], which settles the lift tween at a
//! value (no animation in flight) and restores the previous lift on drop.
//! Every guard here restores the thread-local (or, under its lock,
//! process-wide) state it changed, so nothing leaks into a later test on a
//! reused harness thread. The on-screen
//! keyboard itself stays disabled, so `App::layout`'s post-layout relift never
//! retargets the pinned lift; a click that moves focus does retarget it to 0
//! (`keyboard_scroll::notify_focus_change`), so tests call
//! [`LiftGuard::repin`] after such clicks.

use super::*;
use crate::geometry::{Point, Rect};
use crate::{DrawCtx, Event, EventResult};

/// Pins the keyboard lift at a settled value for one test and, on drop
/// (including on a failing assert), restores the lift and the on-screen
/// keyboard's enabled flag it found, so neither thread-local leaks into a
/// later test on a reused harness thread.
pub(super) struct LiftGuard {
    lift: f64,
    prev_lift: f64,
    prev_keyboard_enabled: bool,
}

impl LiftGuard {
    /// Disable the on-screen keyboard (so layout never relifts) and pin the
    /// lift at `lift`.
    pub(super) fn set(lift: f64) -> Self {
        let prev_keyboard_enabled = crate::widgets::on_screen_keyboard::is_enabled();
        let prev_lift = crate::widget::keyboard_scroll::current_lift();
        crate::widgets::on_screen_keyboard::set_enabled(false);
        crate::widget::keyboard_scroll::set_lift_for_test(lift);
        LiftGuard {
            lift,
            prev_lift,
            prev_keyboard_enabled,
        }
    }

    /// Re-pin the lift after an interaction that retargeted it (a click that
    /// moves focus asks the lift to slide back to 0).
    pub(super) fn repin(&self) {
        crate::widget::keyboard_scroll::set_lift_for_test(self.lift);
    }

    /// The pinned lift.
    pub(super) fn value(&self) -> f64 {
        self.lift
    }
}

impl Drop for LiftGuard {
    fn drop(&mut self) {
        // A settled tween at the previous value (0 → the pristine state).
        crate::widget::keyboard_scroll::set_lift_for_test(self.prev_lift);
        crate::widgets::on_screen_keyboard::set_enabled(self.prev_keyboard_enabled);
    }
}

/// Sets device × UX scale for one test and restores the previous scales on
/// drop.
pub(super) struct ScaleGuard {
    prev: (f64, f64),
}

impl ScaleGuard {
    pub(super) fn set(device: f64, ux: f64) -> Self {
        let prev = (
            crate::device_scale::device_scale(),
            crate::ux_scale::ux_scale(),
        );
        crate::set_device_scale(device);
        crate::ux_scale::set_ux_scale(ux);
        ScaleGuard { prev }
    }
}

impl Drop for ScaleGuard {
    fn drop(&mut self) {
        crate::set_device_scale(self.prev.0);
        crate::ux_scale::set_ux_scale(self.prev.1);
    }
}

/// Clears the shared tooltip timing state and the central controller and
/// puts the thread on the virtual clock; on drop clears both again and
/// restores the clock it found (real time, normally).
pub(super) struct TooltipGuard {
    _clock: crate::clock::ClockGuard,
}

impl TooltipGuard {
    pub(super) fn new() -> Self {
        crate::widgets::tooltip::reset_tooltip_test_state();
        crate::widgets::tooltip::controller::reset();
        TooltipGuard {
            _clock: crate::clock::scoped_virtual(None),
        }
    }
}

impl Drop for TooltipGuard {
    fn drop(&mut self) {
        crate::widgets::tooltip::reset_tooltip_test_state();
        crate::widgets::tooltip::controller::reset();
        // `_clock` drops after this, restoring the previous clock.
    }
}

/// Installs a system font (the central tooltip controller and popup menus
/// paint with it) and restores the previous one on drop.
pub(super) struct SystemFontGuard {
    prev: Option<std::sync::Arc<crate::text::Font>>,
}

impl SystemFontGuard {
    pub(super) fn set(font: std::sync::Arc<crate::text::Font>) -> Self {
        let prev = crate::font_settings::current_system_font();
        crate::font_settings::set_system_font(Some(font));
        SystemFontGuard { prev }
    }
}

impl Drop for SystemFontGuard {
    fn drop(&mut self) {
        crate::font_settings::set_system_font(self.prev.take());
    }
}

/// Advance the virtual clock just past the tooltip initial delay.
pub(super) fn elapse_tooltip_delay() {
    crate::clock::advance(
        crate::widgets::tooltip::tooltip_timings().initial_delay
            + std::time::Duration::from_millis(10),
    );
}

/// Root that places each child at a fixed logical (root-space) rect.
pub(super) struct Place {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    slots: Vec<Rect>,
}

impl Place {
    pub(super) fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            slots: Vec::new(),
        }
    }

    pub(super) fn at(mut self, slot: Rect, child: Box<dyn Widget>) -> Self {
        self.children.push(child);
        self.slots.push(slot);
        self
    }
}

impl Widget for Place {
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
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        for (child, slot) in self.children.iter_mut().zip(&self.slots) {
            child.layout(Size::new(slot.width, slot.height));
            child.set_bounds(*slot);
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Leaf that takes whatever size it is offered, optionally fixed to `size`
/// and filled with `fill`.
pub(super) struct Block {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    size: Option<Size>,
    fill: Option<Color>,
}

impl Block {
    /// Takes whatever size its parent offers and paints nothing.
    pub(super) fn empty() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            size: None,
            fill: None,
        }
    }

    /// Always `size`, filled with `fill`.
    pub(super) fn solid(size: Size, fill: Color) -> Self {
        Self {
            size: Some(size),
            fill: Some(fill),
            ..Self::empty()
        }
    }
}

impl Widget for Block {
    fn type_name(&self) -> &'static str {
        "Block"
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
        let s = self.size.unwrap_or(available);
        self.bounds = Rect::new(0.0, 0.0, s.width, s.height);
        s
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if let Some(c) = self.fill {
            ctx.set_fill_color(c);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
            ctx.fill();
        }
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Physical, Y-down event coordinates for the ON-SCREEN logical point `p`
/// (Y-up) in a viewport `vp_h` logical units tall at effective `scale`.
pub(super) fn screen_phys(scale: f64, vp_h: f64, p: Point) -> (f64, f64) {
    (p.x * scale, (vp_h - p.y) * scale)
}

/// Run one full `App::paint` into a fresh physical-size framebuffer cleared
/// to `clear`, and return the framebuffer (the screen).
pub(super) fn paint_frame(app: &mut App, phys: Size, clear: Color) -> Framebuffer {
    let mut fb = Framebuffer::new(phys.width as u32, phys.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(clear);
        app.paint(&mut ctx);
    }
    fb
}

/// The pixel under on-screen logical point `(x, y)` (Y-up) in a framebuffer
/// painted at `scale` physical px per logical unit.
pub(super) fn sample_logical(fb: &Framebuffer, scale: f64, x: f64, y: f64) -> [u8; 4] {
    sample(fb, (x * scale) as u32, (y * scale) as u32)
}

pub(super) fn is_pure_green(p: [u8; 4]) -> bool {
    p[1] > 200 && p[0] < 60 && p[2] < 60
}

/// A neutral grey: tooltip / menu / popup panel fills and their text, in
/// either theme. Against a pure-red clear this skips antialiased edges, the
/// outer stroke half (both blend toward red) and black drop shadows (dark
/// red), so a bbox of grey pixels is the panel body.
pub(super) fn is_grey(p: [u8; 4]) -> bool {
    let (r, g, b) = (p[0] as i32, p[1] as i32, p[2] as i32);
    (r - g).abs() < 40 && (g - b).abs() < 40
}

/// On-screen logical bounding box `(x0, y0, x1, y1)` of every pixel matching
/// `pred`, in a framebuffer painted at `scale` physical px per logical unit.
pub(super) fn bbox_logical(
    fb: &Framebuffer,
    scale: f64,
    pred: impl Fn([u8; 4]) -> bool,
) -> Option<(f64, f64, f64, f64)> {
    let (w, h) = (fb.width(), fb.height());
    let mut bbox: Option<(u32, u32, u32, u32)> = None;
    for y in 0..h {
        for x in 0..w {
            if pred(sample(fb, x, y)) {
                bbox = Some(match bbox {
                    None => (x, y, x + 1, y + 1),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
                });
            }
        }
    }
    bbox.map(|(x0, y0, x1, y1)| {
        (
            x0 as f64 / scale,
            y0 as f64 / scale,
            x1 as f64 / scale,
            y1 as f64 / scale,
        )
    })
}

/// On-screen logical vertical extent `(y0, y1)` of the pixels matching `pred`
/// in the pixel column under logical `x` — a one-column bbox, for failure
/// messages that report where something actually painted.
pub(super) fn column_extent(
    fb: &Framebuffer,
    scale: f64,
    x: f64,
    pred: impl Fn([u8; 4]) -> bool,
) -> Option<(f64, f64)> {
    let px = (x * scale) as u32;
    let mut extent: Option<(u32, u32)> = None;
    for y in 0..fb.height() {
        if pred(sample(fb, px, y)) {
            extent = Some(match extent {
                None => (y, y + 1),
                Some((y0, y1)) => (y0.min(y), y1.max(y + 1)),
            });
        }
    }
    extent.map(|(y0, y1)| (y0 as f64 / scale, y1 as f64 / scale))
}

/// `|a − b| ≤ tol`.
pub(super) fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}
