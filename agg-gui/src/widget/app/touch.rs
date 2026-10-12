//! Touch-input forwarding methods on [`App`] — start/move/end/cancel
//! plus the active-finger count. Split out of `app.rs` (800-line
//! guardrail). Each entry point feeds two consumers: the multi-touch
//! gesture recogniser in [`crate::touch_state`] (same screen→world
//! conversion — Y-flip + keyboard lift — the mouse path uses), and the
//! primary-finger mouse emulation in [`crate::touch_emulation`], whose
//! commands are replayed through `App::on_mouse_*` in raw screen
//! coordinates; a drag's middle press is offered with the finger's travel
//! and falls back to a left press when no `ScrollView` takes it
//! (`touch_drag_press`). Platform shells therefore forward raw touches ONLY —
//! synthesizing mouse events shell-side would double-fire them.

use super::super::keyboard_scroll;
use crate::event::{Modifiers, MouseButton};
use crate::touch_emulation::EmuCmd;
use crate::widget::App;

impl App {
    pub fn on_touch_start(
        &mut self,
        device: crate::touch_state::TouchDeviceId,
        id: crate::touch_state::TouchId,
        screen_x: f64,
        screen_y: f64,
        force: Option<f32>,
    ) {
        let pos = keyboard_scroll::lift_to_world(self.flip_y(screen_x, screen_y));
        self.touch_state.on_start(device, id, pos, force);
        crate::touch_points::publish(self.touch_state.active_points());
        crate::touch_state::note_touch_event();
        let cmds = self
            .touch_mouse_emu
            .on_start(device, id, screen_x, screen_y);
        self.apply_emu_cmds(cmds);
    }
    pub fn on_touch_move(
        &mut self,
        device: crate::touch_state::TouchDeviceId,
        id: crate::touch_state::TouchId,
        screen_x: f64,
        screen_y: f64,
        force: Option<f32>,
    ) {
        let pos = keyboard_scroll::lift_to_world(self.flip_y(screen_x, screen_y));
        self.touch_state.on_move(device, id, pos, force);
        crate::touch_points::publish(self.touch_state.active_points());
        crate::touch_state::note_touch_event();
        let cmds = self.touch_mouse_emu.on_move(device, id, screen_x, screen_y);
        self.apply_emu_cmds(cmds);
    }
    pub fn on_touch_end(
        &mut self,
        device: crate::touch_state::TouchDeviceId,
        id: crate::touch_state::TouchId,
    ) {
        self.touch_state.on_end_or_cancel(device, id);
        crate::touch_points::publish(self.touch_state.active_points());
        crate::touch_state::note_touch_event();
        let cmds = self.touch_mouse_emu.on_end(device, id);
        self.apply_emu_cmds(cmds);
    }
    pub fn on_touch_cancel(
        &mut self,
        device: crate::touch_state::TouchDeviceId,
        id: crate::touch_state::TouchId,
    ) {
        self.touch_state.on_end_or_cancel(device, id);
        crate::touch_points::publish(self.touch_state.active_points());
        crate::touch_state::note_touch_event();
        let cmds = self.touch_mouse_emu.on_cancel(device, id);
        self.apply_emu_cmds(cmds);
    }

    /// Replay emulation commands through the regular mouse entry points.
    /// `note_touch_event()` has already run by the time this is called,
    /// so widgets can tell these synthetic events apart from real mouse
    /// input via [`crate::touch_state::last_touch_event_age`].
    fn apply_emu_cmds(&mut self, cmds: Vec<EmuCmd>) {
        for cmd in cmds {
            match cmd {
                EmuCmd::MouseMove(x, y) => self.on_mouse_move(x, y),
                EmuCmd::MouseDown(x, y, MouseButton::Middle) => self.touch_drag_press(x, y),
                EmuCmd::MouseDown(x, y, btn) => self.on_mouse_down(x, y, btn, Modifiers::default()),
                EmuCmd::MouseUp(x, y, btn) => self.on_mouse_up(x, y, btn, Modifiers::default()),
                EmuCmd::MouseLeave => self.on_mouse_leave(),
                EmuCmd::CancelPress => self.cancel_pointer_capture(),
            }
        }
    }

    /// The press of a finger drag past the tap tolerance, at the
    /// touch-down point. Offered as a middle press with the finger's travel
    /// published ([`crate::touch_emulation::touch_pan_travel`]), so the
    /// nearest `ScrollView` that can scroll along the drag pans; when
    /// nothing takes it, the drag is the widget's own and the left button
    /// goes down there instead (a slider, a 3D view).
    fn touch_drag_press(&mut self, x: f64, y: f64) {
        let (dx, dy) = self.touch_mouse_emu.travel();
        let scale = crate::ux_scale::effective_scale().max(1e-6);
        crate::touch_emulation::set_touch_pan_travel(Some((dx / scale, -dy / scale)));
        self.on_mouse_down(x, y, MouseButton::Middle, Modifiers::default());
        crate::touch_emulation::set_touch_pan_travel(None);
        if !self.has_captured_pointer() {
            self.touch_mouse_emu.press_instead();
            self.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
        }
    }

    /// Current number of fingers down across all devices.  Used by
    /// widgets that want to know the gesture has *begun* before the
    /// first frame has had a chance to produce a delta (where
    /// `current_multi_touch()` may still be `None`).
    pub fn active_touch_count(&self) -> usize {
        self.touch_state.active_count()
    }
}
