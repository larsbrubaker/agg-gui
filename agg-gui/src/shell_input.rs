//! Shell-neutral input bookkeeping: [`InputForwarder`] turns
//! [`ForwarderEvent`]s into calls on [`App`]'s platform entry points
//! (`on_mouse_move`, `on_mouse_down`, `on_key_down`, …) while tracking what
//! every shell needs to know about input: the cursor position, held buttons
//! and modifiers, the click count of the last press, and whether real
//! platform input is let through at all. Each press reaches the `App` with
//! its click count; a [`ClickCount::Explicit`] count arrives *stated* (it
//! overrides the text editors' own multi-click timing), an
//! [`ClickCount::Auto`] one does not (`event::current_click_count`).
//!
//! `agg-gui-shell` (winit) and `agg-gui-web-shell` (DOM) translate OS events
//! into `ForwarderEvent`s and hand them here; agg-gui-automation's simulated
//! input produces the same events, so a simulated click and a real one run
//! identical code. Shell-specific side effects (the OS cursor icon, redraw
//! requests, the web shell's DOM `buttons` resync) stay in the shells.

use std::path::PathBuf;
use std::time::Duration;

use web_time::Instant;

use crate::event::{DroppedFileData, Key, Modifiers, MouseButton};
use crate::touch_state::{TouchDeviceId, TouchId, TouchPhase};
use crate::widget::App;

/// How a press's click count is decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ClickCount {
    /// Count from the [`ClickPolicy`]: a press of the same button within the
    /// double-click time and travel tolerance of the previous press continues
    /// its sequence; anything else starts a new one at 1.
    #[default]
    Auto,
    /// The platform (or a simulated gesture) states the count.
    Explicit(u32),
}

/// The double-click rule for [`ClickCount::Auto`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClickPolicy {
    /// Longest gap between two presses of one sequence. A shell that can read
    /// the OS setting passes it in; the default matches agg-gui's text
    /// editors (`widgets::multi_click`).
    pub double_click_time: Duration,
    /// Farthest the pointer may travel between two presses of one sequence,
    /// in logical pixels.
    pub travel_tolerance: f64,
}

impl Default for ClickPolicy {
    fn default() -> Self {
        Self {
            double_click_time: crate::widgets::multi_click::MULTI_CLICK_TIME,
            travel_tolerance: crate::widgets::multi_click::MULTI_CLICK_DIST,
        }
    }
}

/// Where an event came from, for the real-input gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputSource {
    /// The OS (winit, the DOM). Dropped while
    /// [`InputForwarder::set_platform_input_enabled`] is off.
    Platform,
    /// Synthesised by a test driver; always delivered.
    Simulated,
}

/// One input event, in the physical-pixel, Y-down space the `App` entry
/// points take.
///
/// `modifiers: None` means "the held modifiers" (a winit shell, which hears
/// modifier changes separately); `Some` carries the event's own state (a DOM
/// event) and updates the held set. `at: None` means "at the tracked
/// cursor"; `Some` updates the tracked cursor first.
#[derive(Clone, Debug)]
pub enum ForwarderEvent {
    /// The cursor moved. With `modifiers` this is `App::on_mouse_move_mods`.
    MouseMove {
        x: f64,
        y: f64,
        modifiers: Option<Modifiers>,
    },
    MouseDown {
        at: Option<(f64, f64)>,
        button: MouseButton,
        modifiers: Option<Modifiers>,
        clicks: ClickCount,
    },
    MouseUp {
        at: Option<(f64, f64)>,
        button: MouseButton,
        modifiers: Option<Modifiers>,
    },
    /// The cursor left the window.
    MouseLeave,
    /// Wheel notches (see `App::on_mouse_wheel_xy_mods` for the sign rule).
    Wheel {
        at: Option<(f64, f64)>,
        delta_x: f64,
        delta_y: f64,
        modifiers: Option<Modifiers>,
    },
    /// One trackpad magnify event: the incremental change of scale (`0.1` is
    /// 10% bigger). Becomes a marked wheel and virtual fingers; see
    /// `App::on_trackpad_magnify`.
    TrackpadPinch {
        at: Option<(f64, f64)>,
        magnification: f64,
        phase: crate::trackpad_pinch_fingers::TrackpadGesturePhase,
        modifiers: Option<Modifiers>,
    },
    /// One trackpad rotate event: degrees, counter-clockwise positive.
    /// Becomes virtual fingers; see `App::on_trackpad_rotate`.
    TrackpadRotate {
        at: Option<(f64, f64)>,
        degrees: f64,
        phase: crate::trackpad_pinch_fingers::TrackpadGesturePhase,
    },
    /// A modifier-only change (Shift pressed mid-drag).
    ModifiersChanged(Modifiers),
    KeyDown {
        key: Key,
        modifiers: Option<Modifiers>,
    },
    KeyUp {
        key: Key,
        modifiers: Option<Modifiers>,
    },
    /// A raw touch contact; the app aggregates gestures and emulates the mouse.
    Touch {
        phase: TouchPhase,
        device: TouchDeviceId,
        id: TouchId,
        x: f64,
        y: f64,
        force: Option<f32>,
    },
    /// A file drag is over the window (entered or moved). The position is
    /// explicit and does not move the tracked cursor: during an OS file drag
    /// the shell may read a live position the cursor tracking never saw.
    FileDragHover {
        x: f64,
        y: f64,
        paths: Vec<PathBuf>,
    },
    FileDragLeave,
    FileDropped {
        x: f64,
        y: f64,
        paths: Vec<PathBuf>,
    },
    FileDataDropped {
        x: f64,
        y: f64,
        files: Vec<DroppedFileData>,
    },
    /// The window became active again (`App::on_window_activated`).
    WindowActivated,
    /// The window lost activation (`App::on_window_deactivated`). The
    /// releases of any held buttons go to the app the user switched to, so
    /// the held-button bookkeeping is cleared too.
    WindowDeactivated,
}

/// The press a click sequence continues from.
#[derive(Clone, Copy, Debug)]
struct LastPress {
    at: Instant,
    pos: (f64, f64),
    button: MouseButton,
    count: u32,
}

/// Input bookkeeping for one window. See the module docs.
#[derive(Debug)]
pub struct InputForwarder {
    cursor: (f64, f64),
    modifiers: Modifiers,
    /// Presses minus releases, saturating at zero — a release whose press
    /// the window never saw (pressed outside, released inside) cannot wedge
    /// it below zero.
    buttons_down: u32,
    held: Vec<MouseButton>,
    last_press: Option<LastPress>,
    click_policy: ClickPolicy,
    platform_input: bool,
}

impl Default for InputForwarder {
    fn default() -> Self {
        Self::new()
    }
}

impl InputForwarder {
    pub fn new() -> Self {
        Self {
            cursor: (0.0, 0.0),
            modifiers: Modifiers::default(),
            buttons_down: 0,
            held: Vec::new(),
            last_press: None,
            click_policy: ClickPolicy::default(),
            platform_input: true,
        }
    }

    /// Last tracked cursor position (physical px, Y-down).
    pub fn cursor(&self) -> (f64, f64) {
        self.cursor
    }

    /// The held modifiers, as last reported.
    pub fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// Number of presses not yet released (0 = the pointer is idle).
    pub fn buttons_down(&self) -> u32 {
        self.buttons_down
    }

    /// Whether `button` has been pressed and not yet released.
    pub fn is_button_down(&self, button: MouseButton) -> bool {
        self.held.contains(&button)
    }

    /// Click count of the most recent press (0 before any press).
    pub fn click_count(&self) -> u32 {
        self.last_press.map_or(0, |p| p.count)
    }

    pub fn click_policy(&self) -> ClickPolicy {
        self.click_policy
    }

    pub fn set_click_policy(&mut self, policy: ClickPolicy) {
        self.click_policy = policy;
    }

    /// Whether [`InputSource::Platform`] events are delivered.
    pub fn platform_input_enabled(&self) -> bool {
        self.platform_input
    }

    /// Let real input through, or drop it (a test driving the live window
    /// must not be disturbed by the user's mouse).
    pub fn set_platform_input_enabled(&mut self, enabled: bool) {
        self.platform_input = enabled;
    }

    /// Deliver an OS event. Returns `false` when the real-input gate dropped it.
    pub fn platform(&mut self, app: &mut App, event: ForwarderEvent) -> bool {
        self.forward(app, InputSource::Platform, event)
    }

    /// Deliver a synthesised event; never gated.
    pub fn simulated(&mut self, app: &mut App, event: ForwarderEvent) {
        self.forward(app, InputSource::Simulated, event);
    }

    /// Update the bookkeeping and call the matching `App` entry point.
    /// Returns `false` when the event was dropped by the real-input gate.
    pub fn forward(&mut self, app: &mut App, source: InputSource, event: ForwarderEvent) -> bool {
        if source == InputSource::Platform && !self.platform_input {
            return false;
        }
        match event {
            ForwarderEvent::MouseMove { x, y, modifiers } => {
                self.cursor = (x, y);
                match modifiers {
                    Some(mods) => {
                        self.modifiers = mods;
                        app.on_mouse_move_mods(x, y, mods);
                    }
                    None => app.on_mouse_move(x, y),
                }
            }
            ForwarderEvent::MouseDown {
                at,
                button,
                modifiers,
                clicks,
            } => {
                let (x, y) = self.place(at);
                let mods = self.resolve(modifiers);
                self.buttons_down = self.buttons_down.saturating_add(1);
                if !self.held.contains(&button) {
                    self.held.push(button);
                }
                let count = self.register_press(button, (x, y), clicks);
                let stated = matches!(clicks, ClickCount::Explicit(_));
                app.on_mouse_down_counted(x, y, button, mods, count, stated);
            }
            ForwarderEvent::MouseUp {
                at,
                button,
                modifiers,
            } => {
                let (x, y) = self.place(at);
                let mods = self.resolve(modifiers);
                self.buttons_down = self.buttons_down.saturating_sub(1);
                self.held.retain(|b| *b != button);
                app.on_mouse_up(x, y, button, mods);
            }
            ForwarderEvent::MouseLeave => app.on_mouse_leave(),
            ForwarderEvent::Wheel {
                at,
                delta_x,
                delta_y,
                modifiers,
            } => {
                let (x, y) = self.place(at);
                let mods = self.resolve(modifiers);
                app.on_mouse_wheel_xy_mods(x, y, delta_x, delta_y, mods);
            }
            ForwarderEvent::TrackpadPinch {
                at,
                magnification,
                phase,
                modifiers,
            } => {
                let (x, y) = self.place(at);
                let mods = self.resolve(modifiers);
                app.on_trackpad_magnify(x, y, magnification, phase, mods);
            }
            ForwarderEvent::TrackpadRotate { at, degrees, phase } => {
                let (x, y) = self.place(at);
                app.on_trackpad_rotate(x, y, degrees, phase);
            }
            ForwarderEvent::ModifiersChanged(mods) => {
                self.modifiers = mods;
                app.on_modifiers_changed(mods);
            }
            ForwarderEvent::KeyDown { key, modifiers } => {
                let mods = self.resolve(modifiers);
                app.on_key_down(key, mods);
            }
            ForwarderEvent::KeyUp { key, modifiers } => {
                let mods = self.resolve(modifiers);
                app.on_key_up(key, mods);
            }
            ForwarderEvent::Touch {
                phase,
                device,
                id,
                x,
                y,
                force,
            } => match phase {
                TouchPhase::Start => app.on_touch_start(device, id, x, y, force),
                TouchPhase::Move => app.on_touch_move(device, id, x, y, force),
                TouchPhase::End => app.on_touch_end(device, id),
                TouchPhase::Cancel => app.on_touch_cancel(device, id),
            },
            ForwarderEvent::FileDragHover { x, y, paths } => app.on_file_drag_hover(x, y, paths),
            ForwarderEvent::FileDragLeave => app.on_file_drag_leave(),
            ForwarderEvent::FileDropped { x, y, paths } => app.on_file_dropped(x, y, paths),
            ForwarderEvent::FileDataDropped { x, y, files } => {
                app.on_file_data_dropped(x, y, files)
            }
            ForwarderEvent::WindowActivated => app.on_window_activated(),
            ForwarderEvent::WindowDeactivated => {
                self.buttons_down = 0;
                self.held.clear();
                app.on_window_deactivated();
            }
        }
        true
    }

    /// The event position: `at` (which becomes the tracked cursor) or the
    /// tracked cursor.
    fn place(&mut self, at: Option<(f64, f64)>) -> (f64, f64) {
        if let Some(at) = at {
            self.cursor = at;
        }
        self.cursor
    }

    /// The event's own modifiers (which become the held set) or the held set.
    fn resolve(&mut self, modifiers: Option<Modifiers>) -> Modifiers {
        if let Some(mods) = modifiers {
            self.modifiers = mods;
        }
        self.modifiers
    }

    /// Record a press and return its click count.
    fn register_press(&mut self, button: MouseButton, pos: (f64, f64), clicks: ClickCount) -> u32 {
        let now = crate::clock::now();
        let count = match clicks {
            ClickCount::Explicit(n) => n,
            ClickCount::Auto => match self.last_press {
                Some(last) if self.continues(&last, button, pos, now) => last.count + 1,
                _ => 1,
            },
        };
        self.last_press = Some(LastPress {
            at: now,
            pos,
            button,
            count,
        });
        count
    }

    /// Whether a press of `button` at `pos` at `now` continues `last`'s
    /// sequence. Travel is measured in logical pixels (positions are
    /// physical, so they are divided by the device scale).
    fn continues(
        &self,
        last: &LastPress,
        button: MouseButton,
        pos: (f64, f64),
        now: Instant,
    ) -> bool {
        // Strictly inside the window, as `widgets::multi_click` counts.
        let in_time = now.saturating_duration_since(last.at) < self.click_policy.double_click_time;
        let scale = crate::device_scale().max(f64::MIN_POSITIVE);
        let dx = (pos.0 - last.pos.0) / scale;
        let dy = (pos.1 - last.pos.1) / scale;
        let tol = self.click_policy.travel_tolerance;
        last.button == button && in_time && dx * dx + dy * dy <= tol * tol
    }
}
