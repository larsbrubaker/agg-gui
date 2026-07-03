//! Touch-input forwarding methods on [`App`] — start/move/end/cancel
//! plus the active-finger count. Split out of `app.rs` (800-line
//! guardrail); pure plumbing into [`crate::touch_state`] with the same
//! screen→world conversion (Y-flip + keyboard lift) the mouse path uses.

use super::super::keyboard_scroll;
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
        crate::touch_state::note_touch_event();
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
        crate::touch_state::note_touch_event();
    }
    pub fn on_touch_end(
        &mut self,
        device: crate::touch_state::TouchDeviceId,
        id: crate::touch_state::TouchId,
    ) {
        self.touch_state.on_end_or_cancel(device, id);
        crate::touch_state::note_touch_event();
    }
    pub fn on_touch_cancel(
        &mut self,
        device: crate::touch_state::TouchDeviceId,
        id: crate::touch_state::TouchId,
    ) {
        self.touch_state.on_end_or_cancel(device, id);
        crate::touch_state::note_touch_event();
    }
    /// Current number of fingers down across all devices.  Used by
    /// widgets that want to know the gesture has *begun* before the
    /// first frame has had a chance to produce a delta (where
    /// `current_multi_touch()` may still be `None`).
    pub fn active_touch_count(&self) -> usize {
        self.touch_state.active_count()
    }
}
