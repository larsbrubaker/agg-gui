//! How simulated pointer input reaches the window — the port of agg-sharp
//! `GuiAutomation/AggInputMethods.cs` (`IInputMethod`, `AggInputMethods`)
//! and `MouseConsts.cs`.
//!
//! [`InputMethod`] is C#'s `IInputMethod`: what the runner's pointer gestures
//! (`runner/pointer.rs`) talk to. [`SimulatedInput`] is C#'s
//! `AggInputMethods`: it turns each move, press and release into a
//! [`ForwarderEvent`] and hands it to the driver's
//! [`InputForwarder`](agg_gui::shell_input::InputForwarder), the same
//! bookkeeping the native and web shells feed their OS events through, so a
//! simulated click runs the product's input path.
//!
//! Positions are the runner's pointer space: physical pixels, Y-down from the
//! window's top-left corner — what `App`'s entry points take. C# works in
//! desktop coordinates and maps each event into the window it lands in; a
//! runner drives one window, so its pointer space *is* that window's.
//!
//! C#'s rules are kept:
//! - A move always goes to the window, wherever the pointer is.
//! - A press or release goes to the window only when the pointer is inside
//!   it (edges included, as C#'s `RectangleDouble.Contains`).
//! - A press reports the click count it is given when that is 2 (the second
//!   press of a double click) and 1 otherwise; every release reports 1, as
//!   WinForms delivers real input.
//! - [`InputMethod::left_button_down`] follows the last button event
//!   whether or not it reached the window.
//!
//! The keyboard members of `IInputMethod` (`PressModifierKeys`,
//! `ReleaseModifierKeys`, `Type`) and `GetCurrentScreen` join this trait
//! with the keyboard and image slices of `docs/design/gui-automation.md`;
//! `GetCurrentScreenHeight` (always 0 in C#) and `Dispose` are not ported.

use agg_gui::shell_input::{ClickCount, ForwarderEvent};
use agg_gui::MouseButton;

use crate::driver::UiDriver;

/// C# `Point2D`: an integer point. In the runner's pointer space it is
/// physical pixels, Y-down; as a widget offset it is logical units, Y-up
/// from the widget's lower-left corner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Point2D {
    pub x: i32,
    pub y: i32,
}

impl Point2D {
    /// C# `Point2D.Zero`.
    pub const ZERO: Point2D = Point2D { x: 0, y: 0 };

    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// C# `new Point2D(double x, double y)`: each coordinate rounded with
    /// `Math.Round` (ties to even). Out-of-range values saturate where C#'s
    /// `(int)` cast would be undefined; window coordinates never get there.
    pub fn from_f64(x: f64, y: f64) -> Self {
        Self {
            x: x.round_ties_even() as i32,
            y: y.round_ties_even() as i32,
        }
    }
}

impl std::ops::Add for Point2D {
    type Output = Point2D;

    fn add(self, other: Point2D) -> Point2D {
        Point2D::new(self.x + other.x, self.y + other.y)
    }
}

/// C#'s `MouseConsts` flags (`MOUSEEVENTF_LEFTDOWN`, ...) as an enum: one
/// button transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    LeftDown,
    LeftUp,
    RightDown,
    RightUp,
    MiddleDown,
    MiddleUp,
}

impl MouseAction {
    /// C# `GetMouseDown`: the press of `button`. agg-gui's `Other` buttons
    /// have no C# counterpart; they press as the left button, as C#'s
    /// `MapButtons` falls back to `Left`.
    pub fn down(button: MouseButton) -> Self {
        match button {
            MouseButton::Right => MouseAction::RightDown,
            MouseButton::Middle => MouseAction::MiddleDown,
            MouseButton::Left | MouseButton::Other(_) => MouseAction::LeftDown,
        }
    }

    /// C# `GetMouseUp`: the release of `button` (see [`down`](Self::down)).
    pub fn up(button: MouseButton) -> Self {
        match button {
            MouseButton::Right => MouseAction::RightUp,
            MouseButton::Middle => MouseAction::MiddleUp,
            MouseButton::Left | MouseButton::Other(_) => MouseAction::LeftUp,
        }
    }

    /// C# `MapButtons`: the button this transition is of.
    pub fn button(self) -> MouseButton {
        match self {
            MouseAction::LeftDown | MouseAction::LeftUp => MouseButton::Left,
            MouseAction::RightDown | MouseAction::RightUp => MouseButton::Right,
            MouseAction::MiddleDown | MouseAction::MiddleUp => MouseButton::Middle,
        }
    }

    /// Whether this is a press.
    pub fn is_down(self) -> bool {
        matches!(
            self,
            MouseAction::LeftDown | MouseAction::RightDown | MouseAction::MiddleDown
        )
    }
}

/// C# `IInputMethod`: where the runner's pointer gestures go. Every call
/// that sends input is handed the window's driver.
pub trait InputMethod {
    /// C# `CurrentMousePosition`: the pointer, in the runner's pointer space.
    fn current_mouse_position(&self) -> Point2D;

    /// C# `LeftButtonDown`: whether the last button event was a left press.
    fn left_button_down(&self) -> bool;

    /// C# `ClickCount`: the click count the last delivered press reported
    /// (0 before any).
    fn click_count(&self) -> u32;

    /// C# `SetCursorPosition`: move the pointer to (`x`, `y`).
    fn set_cursor_position(&mut self, driver: &mut dyn UiDriver, x: i32, y: i32);

    /// C# `CreateMouseEvent`: send one button transition. For a press,
    /// `clicks` is the click number the event should report — 2 for the
    /// second press of a double click, anything else a single click. The
    /// caller states it rather than the input method inferring it from the
    /// spacing of events, so a loaded machine cannot silently turn an
    /// intended double click into two singles.
    fn mouse_event(
        &mut self,
        driver: &mut dyn UiDriver,
        action: MouseAction,
        x: i32,
        y: i32,
        clicks: u32,
    );
}

/// C# `AggInputMethods`: input delivered through the driver's input
/// forwarder; see the module docs.
#[derive(Clone, Debug, Default)]
pub struct SimulatedInput {
    current_mouse_position: Point2D,
    left_button_down: bool,
    right_button_down: bool,
    middle_button_down: bool,
    click_count: u32,
}

impl SimulatedInput {
    pub fn new() -> Self {
        Self::default()
    }

    /// C# `RightButtonDown`.
    pub fn right_button_down(&self) -> bool {
        self.right_button_down
    }

    /// C# `MiddleButtonDown`.
    pub fn middle_button_down(&self) -> bool {
        self.middle_button_down
    }

    /// Whether `p` is inside the window (edges included).
    fn window_contains(driver: &dyn UiDriver, p: Point2D) -> bool {
        let (width, height) = driver.size_px();
        p.x >= 0
            && i64::from(p.x) <= i64::from(width)
            && p.y >= 0
            && i64::from(p.y) <= i64::from(height)
    }
}

impl InputMethod for SimulatedInput {
    fn current_mouse_position(&self) -> Point2D {
        self.current_mouse_position
    }

    fn left_button_down(&self) -> bool {
        self.left_button_down
    }

    fn click_count(&self) -> u32 {
        self.click_count
    }

    fn set_cursor_position(&mut self, driver: &mut dyn UiDriver, x: i32, y: i32) {
        self.current_mouse_position = Point2D::new(x, y);
        // The forwarder knows which buttons are held, so a move with the
        // left button down is a drag without saying so here.
        driver.send(ForwarderEvent::MouseMove {
            x: f64::from(x),
            y: f64::from(y),
            modifiers: None,
        });
    }

    /// Presses and releases go where the pointer is: C#'s `AggInputMethods`
    /// ignores the event's own coordinates, and so does this.
    fn mouse_event(
        &mut self,
        driver: &mut dyn UiDriver,
        action: MouseAction,
        _x: i32,
        _y: i32,
        clicks: u32,
    ) {
        let at = self.current_mouse_position;
        if Self::window_contains(driver, at) {
            let position = Some((f64::from(at.x), f64::from(at.y)));
            let button = action.button();
            if action.is_down() {
                // Only "second press of a double click" is meaningful;
                // everything else is a single click.
                let clicks = if clicks == 2 { 2 } else { 1 };
                self.click_count = clicks;
                driver.send(ForwarderEvent::MouseDown {
                    at: position,
                    button,
                    modifiers: None,
                    clicks: ClickCount::Explicit(clicks),
                });
            } else {
                // Every release reports 1; the forwarder carries no count on
                // a release at all, which `App` reads the same way.
                driver.send(ForwarderEvent::MouseUp {
                    at: position,
                    button,
                    modifiers: None,
                });
            }
        }

        self.left_button_down = action == MouseAction::LeftDown;
        self.middle_button_down = action == MouseAction::MiddleDown;
        self.right_button_down = action == MouseAction::RightDown;
    }
}
