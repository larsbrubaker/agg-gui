//! How simulated pointer and keyboard input reaches the window — the port
//! of agg-sharp `GuiAutomation/AggInputMethods.cs` (`IInputMethod`,
//! `AggInputMethods`) and `MouseConsts.cs`.
//!
//! [`InputMethod`] is C#'s `IInputMethod`: what the runner's pointer gestures
//! (`runner/pointer.rs`) and keyboard calls (`runner/keyboard.rs`) talk to.
//! [`SimulatedInput`] is C#'s `AggInputMethods`: it turns each move, press,
//! release, key stroke and modifier change into a [`ForwarderEvent`] and
//! hands it to the driver's
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
//! - Rust-only, [`InputMethod::platform_counted_mouse_event`] sends a press
//!   with no count ([`ClickCount::Auto`]): the forwarder counts it from the
//!   time and travel since the previous press, as for an OS click.
//! - [`InputMethod::left_button_down`] follows the last button event
//!   whether or not it reached the window.
//! - [`InputMethod::record_cursor_position`] moves the pointer and sends
//!   nothing (an OS file drag owns the pointer; Rust-only).
//!
//!
//! Keyboard (C#'s `Type`, `SendStroke`, `PressModifierKeys`,
//! `ReleaseModifierKeys`):
//! - A stroke is a key down then a key up of [`TypedKey::agg_key`]; agg-gui
//!   has no separate KeyPress (a typed character is `Key::Char`), so a
//!   widget that would suppress C#'s KeyPress consumes the key down instead.
//! - A stroke's modifiers are the held modifiers plus its own, carried in
//!   both events; afterwards the held set is restored (C# sets and clears
//!   `Keyboard` state around each stroke), so `"^a"` leaves no Control
//!   behind for the next click.
//! - Pressing modifiers sends the new held set (`ModifiersChanged`), then a
//!   key down for each modifier key; releasing sends the reduced set, then a
//!   key up for each. Only the named modifiers are released: C#'s
//!   `ReleaseModifierKeys` cleared every held modifier whatever it was asked
//!   to release (it derived the remaining set from a key event that carried
//!   no modifier bits), so releasing Shift also dropped a held Control.
//!
//! The close chord (`%{F4}`) is not a stroke: the runner asks the window to
//! close itself. `GetCurrentScreen` joins this trait with the image slice of
//! `docs/design/gui-automation.md`; `GetCurrentScreenHeight` (always 0 in
//! C#) and `Dispose` are not ported.

use agg_gui::shell_input::{ClickCount, ForwarderEvent};
use agg_gui::MouseButton;

use crate::driver::UiDriver;
use crate::keys::Keys;
use crate::typed_key_parser::TypedKey;

/// The modifier bits an input method holds (C# `Keys.Shift`, `Control`,
/// `Alt`).
const MODIFIER_BITS: Keys = Keys(Keys::SHIFT.0 | Keys::CONTROL.0 | Keys::ALT.0);

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
    /// The press of an extra button (agg-gui's [`MouseButton::Other`]: a
    /// mouse's back or forward button).
    OtherDown(u8),
    /// The release of an extra button.
    OtherUp(u8),
}

impl MouseAction {
    /// C# `GetMouseDown`: the press of `button`. An extra button
    /// ([`MouseButton::Other`]) presses as itself: C#'s automation can press
    /// any WinForms button (`XButton1` is the back button), so a test of a
    /// back-button gesture needs it delivered as that button, not folded into
    /// the left one.
    pub fn down(button: MouseButton) -> Self {
        match button {
            MouseButton::Left => MouseAction::LeftDown,
            MouseButton::Right => MouseAction::RightDown,
            MouseButton::Middle => MouseAction::MiddleDown,
            MouseButton::Other(n) => MouseAction::OtherDown(n),
        }
    }

    /// C# `GetMouseUp`: the release of `button` (see [`down`](Self::down)).
    pub fn up(button: MouseButton) -> Self {
        match button {
            MouseButton::Left => MouseAction::LeftUp,
            MouseButton::Right => MouseAction::RightUp,
            MouseButton::Middle => MouseAction::MiddleUp,
            MouseButton::Other(n) => MouseAction::OtherUp(n),
        }
    }

    /// C# `MapButtons`: the button this transition is of.
    pub fn button(self) -> MouseButton {
        match self {
            MouseAction::LeftDown | MouseAction::LeftUp => MouseButton::Left,
            MouseAction::RightDown | MouseAction::RightUp => MouseButton::Right,
            MouseAction::MiddleDown | MouseAction::MiddleUp => MouseButton::Middle,
            MouseAction::OtherDown(n) | MouseAction::OtherUp(n) => MouseButton::Other(n),
        }
    }

    /// Whether this is a press.
    pub fn is_down(self) -> bool {
        matches!(
            self,
            MouseAction::LeftDown
                | MouseAction::RightDown
                | MouseAction::MiddleDown
                | MouseAction::OtherDown(_)
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

    /// The pointer is now at (`x`, `y`), and the window saw no mouse move
    /// get it there: an OS drag-and-drop from another application owns the
    /// pointer, and the window hears of it only through file-drag events
    /// (`runner/file_drop.rs`). Later moves start from here. No C#
    /// counterpart (C#'s `IInputMethod` has no file drags).
    fn record_cursor_position(&mut self, x: i32, y: i32);

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

    /// A button transition whose press's click count the window works out,
    /// as it does for an OS click: the press goes to the driver's input
    /// forwarder with [`ClickCount::Auto`], so it continues the previous
    /// press's sequence when it is of the same button within the
    /// forwarder's double-click time and travel tolerance
    /// ([`ClickPolicy`](agg_gui::shell_input::ClickPolicy)), and starts a
    /// new one at 1 otherwise. A release is sent as by
    /// [`mouse_event`](Self::mouse_event). No C# counterpart: C#'s automation
    /// always states the count; this is for tests of how the product reads
    /// counts the platform worked out.
    fn platform_counted_mouse_event(
        &mut self,
        driver: &mut dyn UiDriver,
        action: MouseAction,
        x: i32,
        y: i32,
    );

    /// C# `PressModifierKeys`: hold the modifiers whose bits
    /// ([`Keys::SHIFT`], [`Keys::CONTROL`], [`Keys::ALT`]) are set in
    /// `modifier_keys`, on top of any already held. Key codes are ignored:
    /// or'd together they cannot be told apart (`ShiftKey | ControlKey` is
    /// `ControlKey`).
    fn press_modifier_keys(&mut self, driver: &mut dyn UiDriver, modifier_keys: Keys);

    /// C# `ReleaseModifierKeys`: let go of the modifiers whose bits are set
    /// in `modifier_keys`; the others stay held.
    fn release_modifier_keys(&mut self, driver: &mut dyn UiDriver, modifier_keys: Keys);

    /// C# `Type` after parsing: put each stroke through the window, in
    /// order.
    fn type_strokes(&mut self, driver: &mut dyn UiDriver, strokes: &[TypedKey]);
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
    /// The modifiers held by [`InputMethod::press_modifier_keys`] (modifier
    /// bits only).
    held_modifiers: Keys,
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

    /// The modifiers held by [`InputMethod::press_modifier_keys`] and not
    /// yet released (C#'s `Keyboard` state), as modifier bits.
    pub fn held_modifiers(&self) -> Keys {
        self.held_modifiers
    }

    /// The modifier keys whose bits are set in `bits`, in Shift, Control,
    /// Alt order, as agg-gui keys.
    fn modifier_keys_of(bits: Keys) -> Vec<agg_gui::Key> {
        [
            (Keys::SHIFT, Keys::SHIFT_KEY),
            (Keys::CONTROL, Keys::CONTROL_KEY),
            (Keys::ALT, Keys::MENU),
        ]
        .into_iter()
        .filter(|(bit, _)| bits.contains(*bit))
        .map(|(_, key)| key.to_agg_key())
        .collect()
    }

    /// Whether `p` is inside the window (edges included).
    fn window_contains(driver: &dyn UiDriver, p: Point2D) -> bool {
        window_contains(driver.size_px(), p)
    }

    /// Send one button transition where the pointer is, a press with
    /// `clicks`, when the pointer is inside the window; the held-button
    /// state follows the transition either way.
    fn button_event(&mut self, driver: &mut dyn UiDriver, action: MouseAction, clicks: ClickCount) {
        let at = self.current_mouse_position;
        if Self::window_contains(driver, at) {
            let position = Some((f64::from(at.x), f64::from(at.y)));
            let button = action.button();
            if action.is_down() {
                driver.send(ForwarderEvent::MouseDown {
                    at: position,
                    button,
                    modifiers: None,
                    clicks,
                });
                self.click_count = match clicks {
                    ClickCount::Explicit(n) => n,
                    // The count the forwarder worked out, as the press just
                    // dispatched on this (the UI) thread reported it.
                    ClickCount::Auto => agg_gui::event::current_click_count(),
                };
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

/// Whether the pointer position `p` is inside a window of `size_px`
/// (physical pixels), edges included, as C#'s `RectangleDouble.Contains`.
pub(crate) fn window_contains(size_px: (u32, u32), p: Point2D) -> bool {
    let (width, height) = size_px;
    p.x >= 0
        && i64::from(p.x) <= i64::from(width)
        && p.y >= 0
        && i64::from(p.y) <= i64::from(height)
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

    fn record_cursor_position(&mut self, x: i32, y: i32) {
        self.current_mouse_position = Point2D::new(x, y);
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
        // Only "second press of a double click" is meaningful; everything
        // else is a single click.
        let clicks = if clicks == 2 { 2 } else { 1 };
        self.button_event(driver, action, ClickCount::Explicit(clicks));
    }

    /// Like [`mouse_event`](InputMethod::mouse_event), the event's own
    /// coordinates are ignored.
    fn platform_counted_mouse_event(
        &mut self,
        driver: &mut dyn UiDriver,
        action: MouseAction,
        _x: i32,
        _y: i32,
    ) {
        self.button_event(driver, action, ClickCount::Auto);
    }

    fn press_modifier_keys(&mut self, driver: &mut dyn UiDriver, modifier_keys: Keys) {
        let pressed = modifier_keys & MODIFIER_BITS;
        self.held_modifiers |= pressed;
        let mods = self.held_modifiers.to_agg_modifiers();
        driver.send(ForwarderEvent::ModifiersChanged(mods));
        for key in Self::modifier_keys_of(pressed) {
            driver.send(ForwarderEvent::KeyDown {
                key,
                modifiers: Some(mods),
            });
        }
    }

    fn release_modifier_keys(&mut self, driver: &mut dyn UiDriver, modifier_keys: Keys) {
        let released = modifier_keys & MODIFIER_BITS;
        self.held_modifiers = Keys(self.held_modifiers.0 & !released.0);
        let mods = self.held_modifiers.to_agg_modifiers();
        driver.send(ForwarderEvent::ModifiersChanged(mods));
        for key in Self::modifier_keys_of(released) {
            driver.send(ForwarderEvent::KeyUp {
                key,
                modifiers: Some(mods),
            });
        }
    }

    fn type_strokes(&mut self, driver: &mut dyn UiDriver, strokes: &[TypedKey]) {
        let held = self.held_modifiers.to_agg_modifiers();
        for stroke in strokes {
            let mods = (self.held_modifiers | stroke.key().modifiers()).to_agg_modifiers();
            let key = stroke.agg_key();
            driver.send(ForwarderEvent::KeyDown {
                key: key.clone(),
                modifiers: Some(mods),
            });
            driver.send(ForwarderEvent::KeyUp {
                key,
                modifiers: Some(mods),
            });
            // C#'s `finally`: the stroke's own modifiers end with it.
            if mods != held {
                driver.send(ForwarderEvent::ModifiersChanged(held));
            }
        }
    }
}
