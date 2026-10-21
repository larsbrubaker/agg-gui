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
//!
//! Momentum scrolling ([`crate::touch_emulation::fling`]) needs two things
//! from here: a finger landing on a flinging view catches it (and is then no
//! tap), and only a finger's lift — not a second finger landing, not a
//! cancel — may throw the content.
//!
//! A finger that lands on the on-screen keyboard is the keyboard's alone:
//! it presses its key the moment it lands, slides across the keys and types
//! when it lifts, straight through the keyboard's pointer handlers. The
//! mouse emulation would hold a tap's press back until the lift (it can't
//! tell a tap from a pan before then), so the pressed key, its preview,
//! the long-press alternates and Backspace's repeat would never show, and
//! a slide would become a pan that types nothing.
//!
//! The long press ([`crate::touch_emulation::long_press`]) needs time to
//! pass with no input: `layout` polls the emulator every frame
//! (`tick_long_press`), and `paint` keeps a wake-up armed for the next
//! moment that matters, publishes the cue and draws the default ring
//! (`paint_long_press_cue`), so an idle app still fires on time.

use super::super::keyboard_scroll;
use crate::draw_ctx::DrawCtx;
use crate::event::{Modifiers, MouseButton};
use crate::touch_emulation::{long_press, EmuCmd, LongPressCue, LONG_PRESS_CUE_DELAY};
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
        if self.keyboard_touch_start(device, id, screen_x, screen_y) {
            return;
        }
        let cmds = self
            .touch_mouse_emu
            .on_start(device, id, screen_x, screen_y);
        self.apply_emu_cmds(cmds);
        // A finger on content that is still flinging stops it, and is not a
        // tap on whatever row the fling carried under it.
        if crate::touch_emulation::fling::catch_at(pos) {
            self.touch_mouse_emu.catch_fling();
        }
        // Wake an idle app when the hold's cue is due; each paint after
        // that re-arms until the long press fires or the finger lets go.
        if self.touch_mouse_emu.long_press_deadline().is_some() {
            crate::animation::request_draw_after(LONG_PRESS_CUE_DELAY);
        }
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
        if self.keyboard_touch_move(device, id, screen_x, screen_y) {
            return;
        }
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
        if self.keyboard_touch_end(device, id, true) {
            return;
        }
        let cmds = self.touch_mouse_emu.on_end(device, id);
        // The finger lifting is the one release that may fling a pan.
        crate::touch_emulation::fling::while_lifting(|| self.apply_emu_cmds(cmds));
    }
    pub fn on_touch_cancel(
        &mut self,
        device: crate::touch_state::TouchDeviceId,
        id: crate::touch_state::TouchId,
    ) {
        self.touch_state.on_end_or_cancel(device, id);
        crate::touch_points::publish(self.touch_state.active_points());
        crate::touch_state::note_touch_event();
        if self.keyboard_touch_end(device, id, false) {
            return;
        }
        let cmds = self.touch_mouse_emu.on_cancel(device, id);
        self.apply_emu_cmds(cmds);
    }

    /// A finger landing on the on-screen keyboard presses the key under it
    /// and is the keyboard's until it lifts. A second finger on the keyboard
    /// first types the key the first one is on, as a fast typist's fingers
    /// overlap. `false` when the finger is not on the keyboard.
    fn keyboard_touch_start(
        &mut self,
        device: crate::touch_state::TouchDeviceId,
        id: crate::touch_state::TouchId,
        screen_x: f64,
        screen_y: f64,
    ) -> bool {
        use crate::widgets::on_screen_keyboard as osk;
        let at = self.flip_y(screen_x, screen_y);
        if !osk::contains_point(at) {
            return false;
        }
        if let Some((d, i, _)) = self.keyboard_touch {
            self.keyboard_touch_end(d, i, true);
        }
        osk::handle_software_keyboard_mouse_down(at, MouseButton::Left, Modifiers::default());
        self.keyboard_touch = Some((device, id, at));
        true
    }

    /// The keyboard's finger moved: the pressed key (or the highlighted
    /// alternate) follows it. `false` for any other finger.
    fn keyboard_touch_move(
        &mut self,
        device: crate::touch_state::TouchDeviceId,
        id: crate::touch_state::TouchId,
        screen_x: f64,
        screen_y: f64,
    ) -> bool {
        let at = self.flip_y(screen_x, screen_y);
        match &mut self.keyboard_touch {
            Some((d, i, last)) if *d == device && *i == id => {
                *last = at;
                crate::widgets::on_screen_keyboard::handle_software_keyboard_mouse_move(at);
                true
            }
            _ => false,
        }
    }

    /// The keyboard's finger lifted (`lifted`) or was cancelled: a lift
    /// types the key under it and hands the key to the focused field; a
    /// cancel types nothing. `false` for any other finger.
    fn keyboard_touch_end(
        &mut self,
        device: crate::touch_state::TouchDeviceId,
        id: crate::touch_state::TouchId,
        lifted: bool,
    ) -> bool {
        use crate::widgets::on_screen_keyboard as osk;
        match self.keyboard_touch {
            Some((d, i, last)) if d == device && i == id => {
                self.keyboard_touch = None;
                if lifted {
                    osk::handle_software_keyboard_mouse_up(
                        last,
                        MouseButton::Left,
                        Modifiers::default(),
                    );
                    self.drain_keyboard_synthetic_keys();
                } else {
                    osk::cancel_press();
                }
                true
            }
            _ => false,
        }
    }

    /// Replay emulation commands through the regular mouse entry points.
    /// `note_touch_event()` has already run by the time this is called,
    /// so widgets can tell these synthetic events apart from real mouse
    /// input via [`crate::touch_state::last_touch_event_age`].
    fn apply_emu_cmds(&mut self, cmds: Vec<EmuCmd>) {
        long_press::while_emulating(|| self.replay_emu_cmds(cmds));
        if let Some((x, y)) = self.touch_mouse_emu.take_long_press() {
            let at = self.flip_y(x, y);
            long_press::notify(LongPressCue {
                x: at.x,
                y: at.y,
                progress: 1.0,
            });
        }
    }

    fn replay_emu_cmds(&mut self, cmds: Vec<EmuCmd>) {
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

    /// Fire a long press that has come due (start of every layout, so the
    /// menu it opens is laid out in the same frame).
    pub(super) fn tick_long_press(&mut self) {
        let cmds = self.touch_mouse_emu.poll();
        if !cmds.is_empty() {
            // The right click is the finger's doing: widgets that tell
            // touch-synthesised presses apart by the age of the last touch
            // event (the menus) must see it as fresh.
            crate::touch_state::note_touch_event();
            self.apply_emu_cmds(cmds);
        }
    }

    /// Publish the pending long press's cue, draw the default ring, and keep
    /// the frames coming: continuously while the ring fills, otherwise one
    /// wake-up when the cue is due. Runs inside `paint`, after the draw
    /// request was cleared and with `ctx` in logical Y-up coordinates.
    pub(super) fn paint_long_press_cue(&mut self, ctx: &mut dyn DrawCtx) {
        let cue = self
            .touch_mouse_emu
            .long_press_progress()
            .map(|((x, y), progress)| {
                let at = self.flip_y(x, y);
                LongPressCue {
                    x: at.x,
                    y: at.y,
                    progress,
                }
            });
        long_press::publish_cue(cue);
        match (cue, self.touch_mouse_emu.long_press_deadline()) {
            (Some(cue), _) => {
                if long_press::long_press_ring_drawn() {
                    long_press::paint_ring(ctx, cue);
                }
                // The ring is an overlay painted fresh every frame: no
                // retained widget changed, so no cache invalidates.
                crate::animation::request_draw_without_invalidation();
            }
            (None, Some(deadline)) => {
                let cue_at =
                    deadline - crate::touch_emulation::LONG_PRESS_DELAY + LONG_PRESS_CUE_DELAY;
                let wait = cue_at.saturating_duration_since(crate::clock::now());
                crate::animation::request_draw_after(wait);
            }
            (None, None) => {}
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
