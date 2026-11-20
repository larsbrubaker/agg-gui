//! The shells' frame policy, in one place: what feeds layout (the
//! [`LayoutKey`]), when a frame must lay out ([`LayoutTracker`]), when a tick
//! paints at all ([`wants_frame`]), and the reactive tick a headless driver
//! runs ([`LayoutTracker::tick`]).
//!
//! A frame lays out when its key changed, when a widget requested a layout,
//! or when a widget in the tree asked to be drawn ([`Widget::needs_draw`]):
//! a widget may step what it shows in `layout` (an animation, a status read
//! from a model), and the frame it asks for has to reach it there. That last
//! case leaves the invalidation epoch alone, so retained layers stay valid
//! and a widget that tells a change of its view alone from any other (a node
//! editor's pan) still can.
//!
//! [`Widget::needs_draw`]: crate::widget::Widget::needs_draw
//!
//! `agg-gui-shell` (winit + wgpu), `agg-gui-web-shell` (rAF + wgpu) and
//! headless drivers (agg-gui-automation's `HeadlessDriver`, MatterCAD's UI
//! test harness) all make the same decisions around `App::layout` and
//! `App::paint`; this module is the single copy so they cannot drift. The
//! GPU-side work (surface acquire, present) stays in each shell.

use web_time::Instant;

use crate::geometry::Size;
use crate::widget::App;

/// Everything that feeds layout: the viewport size, the device scale and the
/// agg-gui invalidation epoch ([`crate::animation::invalidation_epoch`]).
/// Layout is skipped while it is unchanged and no widget asked for one.
///
/// Sizes are kept as `f64` bits so a GPU shell (integer surface pixels) and a
/// headless driver (a logical `f64` size) build the same kind of key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayoutKey {
    width: u64,
    height: u64,
    device_scale: u64,
    epoch: u64,
}

impl LayoutKey {
    /// The key for a `width × height` viewport at the current device scale
    /// and invalidation epoch.
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            width: width.to_bits(),
            height: height.to_bits(),
            device_scale: crate::device_scale().to_bits(),
            epoch: crate::animation::invalidation_epoch(),
        }
    }

    /// The key for a surface of `width × height` physical pixels.
    pub fn for_surface(width: u32, height: u32) -> Self {
        Self::new(f64::from(width), f64::from(height))
    }

    /// The key for `viewport`.
    pub fn for_viewport(viewport: Size) -> Self {
        Self::new(viewport.width, viewport.height)
    }
}

/// Whether a frame keyed `next` must lay out after the last laid-out frame
/// keyed `last`: the key changed, or a widget called
/// [`crate::animation::request_layout`] (which survives `App::paint`'s
/// draw-request clear and is consumed only by `App::layout`).
pub fn frame_needs_layout(last: Option<LayoutKey>, next: LayoutKey) -> bool {
    last != Some(next) || crate::animation::layout_requested()
}

/// What a shell knows when it decides whether this tick paints.
#[derive(Clone, Copy, Debug)]
pub struct FrameDemand {
    /// The shell paints every tick (`RedrawPolicy::Continuous`).
    pub continuous: bool,
    /// The shell's own repaint flag: input arrived, the page asked for a
    /// repaint. Native shells, which turn those into window redraw requests
    /// instead, pass `false`.
    pub dirty: bool,
    /// `App::wants_draw()` — widget invalidations, draw requests, a pending
    /// layout request, a due scheduled deadline.
    pub app_wants_draw: bool,
    /// `App::next_draw_deadline()`, for a shell whose wake-up does not itself
    /// request a redraw (the browser's rAF loop). Native shells pass `None`:
    /// their `WaitUntil` wake requests the redraw.
    pub next_deadline: Option<Instant>,
    pub now: Instant,
}

/// Whether a tick with this demand paints: continuous mode, a dirty shell,
/// an app that wants to draw, or a scheduled deadline that has passed.
pub fn wants_frame(demand: &FrameDemand) -> bool {
    demand.continuous
        || demand.dirty
        || demand.app_wants_draw
        || demand.next_deadline.is_some_and(|d| demand.now >= d)
}

/// When a tick runs regardless of [`App::wants_draw`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickMode {
    /// Paint only if the app wants to draw — the reactive idle tick.
    Reactive,
    /// Paint unconditionally — the frame a shell runs after input (the web
    /// shell marks itself dirty on every event) or a forced redraw.
    Forced,
}

/// The layout-skip memory of one window: the key of the last laid-out frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct LayoutTracker {
    last: Option<LayoutKey>,
}

impl LayoutTracker {
    pub const fn new() -> Self {
        Self { last: None }
    }

    /// Whether a frame keyed `next` must lay out ([`frame_needs_layout`]).
    pub fn needs_layout(&self, next: LayoutKey) -> bool {
        frame_needs_layout(self.last, next)
    }

    /// Whether a frame keyed `next` must lay `app` out: [`Self::needs_layout`],
    /// or a widget in its tree asked to be drawn (see the module docs). What
    /// the shells and [`Self::layout_if_needed`] decide by.
    pub fn needs_layout_for(&self, app: &App, next: LayoutKey) -> bool {
        self.needs_layout(next) || app.root().needs_draw()
    }

    /// Record that the frame keyed `key` was laid out and painted. Record the
    /// key computed *before* layout and paint: an invalidation during either
    /// moves the epoch, so the next frame lays out again.
    pub fn record(&mut self, key: LayoutKey) {
        self.last = Some(key);
    }

    /// Forget the last layout, so the next frame lays out from scratch (a
    /// rebuilt render context, a new device).
    pub fn reset(&mut self) {
        self.last = None;
    }

    /// The key of the last laid-out frame.
    pub fn last(&self) -> Option<LayoutKey> {
        self.last
    }

    /// Lay `app` out at `viewport` if [`Self::needs_layout`] says so, and
    /// record the key. Returns whether layout ran.
    pub fn layout_if_needed(&mut self, app: &mut App, viewport: Size) -> bool {
        let key = LayoutKey::for_viewport(viewport);
        let needed = self.needs_layout_for(app, key);
        if needed {
            app.layout(viewport);
            self.record(key);
        }
        needed
    }

    /// One shell tick without a GPU: drain [`crate::ui_thread`], then — if
    /// `mode` is [`TickMode::Forced`] or the app wants to draw — lay out when
    /// due and call `paint`. Returns whether the tick painted.
    ///
    /// As in the GPU shells, the only draw-request clear is the one at the
    /// start of `App::paint`, so a request made during paint wakes the next
    /// tick.
    pub fn tick(
        &mut self,
        app: &mut App,
        viewport: Size,
        mode: TickMode,
        paint: impl FnOnce(&mut App),
    ) -> bool {
        crate::ui_thread::invoke_pending_actions();
        if mode == TickMode::Reactive && !app.wants_draw() {
            return false;
        }
        self.layout_if_needed(app, viewport);
        paint(app);
        true
    }
}

#[cfg(test)]
mod tests;
