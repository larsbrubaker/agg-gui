//! Held-pointer bookkeeping behind [`crate::WebShellControl::pointer_idle`],
//! and which pointer events force a repaint.
//!
//! Platform-neutral so the rules are unit tested natively; the wasm listeners
//! in the crate's `web::input` / `web::lifecycle` modules feed it DOM events.
//! Mouse buttons come from the DOM `buttons` bitmask (authoritative, so a
//! lost release self-heals on the next event); touch contacts are counted by
//! `pointerId`, because a touch drag reports `buttons` only on its own events
//! and a finger on the glass must keep an auto-save from firing mid-drag.

use crate::dom_math::PointerKind;

/// Mouse buttons and touch contacts currently held.
#[derive(Debug, Default)]
pub(crate) struct PointerTracker {
    mouse_buttons: u32,
    touches: Vec<i32>,
}

impl PointerTracker {
    pub(crate) const fn new() -> Self {
        Self {
            mouse_buttons: 0,
            touches: Vec::new(),
        }
    }

    /// Adopt the held mouse-button count derived from an event's `buttons`.
    pub(crate) fn sync_mouse_buttons(&mut self, count: u32) {
        self.mouse_buttons = count;
    }

    /// A touch contact went down.
    pub(crate) fn touch_down(&mut self, pointer_id: i32) {
        if !self.touches.contains(&pointer_id) {
            self.touches.push(pointer_id);
        }
    }

    /// A touch contact lifted or was cancelled. Unknown ids are ignored, so
    /// the canvas and window listeners may both report the same release.
    pub(crate) fn touch_up(&mut self, pointer_id: i32) {
        self.touches.retain(|&id| id != pointer_id);
    }

    /// Forget every contact — the page was hidden, so no release will arrive.
    pub(crate) fn clear(&mut self) {
        self.mouse_buttons = 0;
        self.touches.clear();
    }

    #[cfg(test)]
    pub(crate) fn active_touches(&self) -> usize {
        self.touches.len()
    }

    /// No mouse button held and no finger down.
    pub(crate) fn is_idle(&self) -> bool {
        self.mouse_buttons == 0 && self.touches.is_empty()
    }
}

/// Whether a `pointermove` of this kind must force a repaint on its own.
///
/// Mouse / pen moves don't: hover changes that matter set a widget
/// invalidation and surface through `App::wants_draw()`, so a reactive idle
/// page stays idle while the cursor sweeps across it (the pre-0.5
/// `demo-wgpu` behaviour). Touch moves do: they feed the gesture recogniser,
/// whose state is not visible to `wants_draw()` until the next frame runs.
pub(crate) fn move_forces_repaint(kind: PointerKind) -> bool {
    kind == PointerKind::Touch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_with_nothing_held() {
        assert!(PointerTracker::new().is_idle());
    }

    #[test]
    fn a_touch_drag_is_not_idle() {
        let mut t = PointerTracker::new();
        t.touch_down(7);
        // A touch drag's moves carry no mouse buttons.
        t.sync_mouse_buttons(0);
        assert!(!t.is_idle(), "finger on the glass keeps the guard closed");
        t.touch_up(7);
        assert!(t.is_idle());
    }

    #[test]
    fn multi_touch_needs_every_finger_up() {
        let mut t = PointerTracker::new();
        t.touch_down(1);
        t.touch_down(2);
        t.touch_down(2);
        assert_eq!(t.active_touches(), 2);
        t.touch_up(1);
        assert!(!t.is_idle());
        t.touch_up(2);
        t.touch_up(2);
        assert!(t.is_idle());
    }

    #[test]
    fn mouse_buttons_follow_the_bitmask_count() {
        let mut t = PointerTracker::new();
        t.sync_mouse_buttons(1);
        assert!(!t.is_idle());
        t.sync_mouse_buttons(0);
        assert!(t.is_idle());
    }

    #[test]
    fn clear_reopens_the_guard() {
        let mut t = PointerTracker::new();
        t.touch_down(3);
        t.sync_mouse_buttons(2);
        t.clear();
        assert!(t.is_idle());
    }

    #[test]
    fn only_touch_moves_force_a_repaint() {
        assert!(!move_forces_repaint(PointerKind::Mouse));
        assert!(!move_forces_repaint(PointerKind::Pen));
        assert!(move_forces_repaint(PointerKind::Touch));
    }
}
