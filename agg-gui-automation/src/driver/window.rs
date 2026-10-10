//! `HeadlessWindow`: the C# `SystemWindow` of ported tests that build widgets
//! and drive them without the runner (`new SystemWindow(300, 200)`,
//! `AddChild`, `OnMouseDown(new MouseEventArgs(...))`).
//!
//! A [`HeadlessDriver`] whose root is a [`ProbeWidget`] filling the window.
//! Positions are C#'s window coordinates — logical units, Y-up from the
//! lower-left corner — converted here to the Y-down physical pixels the
//! `App` entry points take, and sent through the driver's input forwarder.
//! Every input call lays the tree out first when it is due, so a freshly
//! added child can be hit.

use agg_gui::event::{Key, Modifiers, MouseButton};
use agg_gui::shell_input::{ClickCount, ForwarderEvent};
use agg_gui::{Size, Widget};

use super::{FrameKind, HeadlessDriver, UiDriver};
use crate::probe::ProbeWidget;
use crate::tree_query::{self, WidgetHandle};

/// The name of a headless window's root widget.
pub const WINDOW_ROOT_NAME: &str = "HeadlessWindow";

/// A headless top-level window; see the module docs.
pub struct HeadlessWindow {
    driver: HeadlessDriver,
}

impl HeadlessWindow {
    /// A `width × height` (logical) window at the thread's device scale, on
    /// the virtual clock.
    pub fn new(width: f64, height: f64) -> Self {
        let scale = agg_gui::ux_scale::effective_scale();
        let root = ProbeWidget::new(WINDOW_ROOT_NAME);
        Self {
            driver: HeadlessDriver::new(
                Box::new(root),
                (width * scale).round() as u32,
                (height * scale).round() as u32,
            ),
        }
    }

    /// The driver underneath (frames, the framebuffer, raw `App` access).
    pub fn driver(&self) -> &HeadlessDriver {
        &self.driver
    }

    /// The driver underneath, mutably.
    pub fn driver_mut(&mut self) -> &mut HeadlessDriver {
        &mut self.driver
    }

    /// The root widget (for [`crate::tree_query`]).
    pub fn root(&self) -> &dyn Widget {
        self.driver.root()
    }

    /// The window's logical size.
    pub fn size(&self) -> Size {
        self.driver.logical_size()
    }

    /// C# `AddChild`: add `child` to the window, kept at its own bounds.
    /// Returns its handle.
    pub fn add_child(&mut self, child: Box<dyn Widget>) -> Option<WidgetHandle> {
        let root = self.driver.root_mut();
        root.children_mut().push(child);
        let index = root.children().len() - 1;
        agg_gui::animation::request_layout();
        WidgetHandle::new(self.driver.root(), &[index])
    }

    /// Every widget named `name`, in paint order.
    pub fn find_by_name(&self, name: &str) -> Vec<WidgetHandle> {
        tree_query::find_by_name(self.root(), name)
    }

    /// The first widget named `name`.
    pub fn handle(&self, name: &str) -> Option<WidgetHandle> {
        self.find_by_name(name).into_iter().next()
    }

    /// Lay out (when due) and paint now (C# `Invalidate` + draw).
    pub fn draw(&mut self) -> bool {
        self.driver.pump(FrameKind::Forced)
    }

    /// Window point (logical, Y-up) to the App's input space (physical,
    /// Y-down).
    pub fn window_to_pointer(&self, x: f64, y: f64) -> (f64, f64) {
        let scale = agg_gui::ux_scale::effective_scale();
        let height = self.size().height;
        (x * scale, (height - y) * scale)
    }

    fn deliver(&mut self, event: ForwarderEvent) {
        self.driver.layout_if_needed();
        self.driver.send(event);
    }

    /// C# `OnMouseMove`.
    pub fn on_mouse_move(&mut self, x: f64, y: f64) {
        let (x, y) = self.window_to_pointer(x, y);
        self.deliver(ForwarderEvent::MouseMove {
            x,
            y,
            modifiers: None,
        });
    }

    /// C# `OnMouseDown` with C#'s explicit click count.
    pub fn on_mouse_down(&mut self, x: f64, y: f64, button: MouseButton, clicks: u32) {
        let at = self.window_to_pointer(x, y);
        self.deliver(ForwarderEvent::MouseDown {
            at: Some(at),
            button,
            modifiers: None,
            clicks: ClickCount::Explicit(clicks),
        });
    }

    /// C# `OnMouseUp`.
    pub fn on_mouse_up(&mut self, x: f64, y: f64, button: MouseButton) {
        let at = self.window_to_pointer(x, y);
        self.deliver(ForwarderEvent::MouseUp {
            at: Some(at),
            button,
            modifiers: None,
        });
    }

    /// C# `Keyboard.SetKeyDownState` for the modifier keys: the modifiers
    /// held from now on, which later presses, moves and releases carry.
    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.deliver(ForwarderEvent::ModifiersChanged(modifiers));
    }

    /// C# `OnKeyDown`.
    pub fn on_key_down(&mut self, key: Key, modifiers: Modifiers) {
        self.deliver(ForwarderEvent::KeyDown {
            key,
            modifiers: Some(modifiers),
        });
    }

    /// C# `OnKeyUp`.
    pub fn on_key_up(&mut self, key: Key, modifiers: Modifiers) {
        self.deliver(ForwarderEvent::KeyUp {
            key,
            modifiers: Some(modifiers),
        });
    }

    /// C# `SystemWindow.OnDeactivated`: the window loses activation, as when
    /// the user switches to another application.
    pub fn on_deactivated(&mut self) {
        self.deliver(ForwarderEvent::WindowDeactivated);
    }

    /// The window becomes active again (the counterpart of
    /// [`on_deactivated`](Self::on_deactivated); agg-sharp has no
    /// `OnActivated`).
    pub fn on_activated(&mut self) {
        self.deliver(ForwarderEvent::WindowActivated);
    }
}
