//! The out-of-view pointer filter: agg-sharp's `OutOfViewMouseCapture`
//! (`Gui/SystemWindow/OutOfViewMouseCapture.cs`), the rule that keeps a drag
//! that has wandered outside the window's content area alive, so the widget
//! that started it still gets its moves and, critically, its mouse up.
//!
//! winit keeps routing a drag's moves and release to the window that saw the
//! press however far the pointer travels; what this decides is which of
//! winit's pointer events reach agg-gui. A button only becomes the window's
//! through a press *inside* the content area, which is what keeps a press
//! agg never saw (a title-bar or resize-border press on a platform that
//! reports one) from delivering a phantom drag or release. A plain hover
//! outside the content area is dropped: with no button held it is nobody's
//! business but the window manager's. `shell_loop.rs` maps winit's
//! `CursorMoved` / `MouseInput` onto [`PointerEventKind`] and asks
//! [`OutOfViewMouseCapture::should_deliver`] before forwarding.
//!
//! The "cursor left" half of C#'s class (`IsRealPointerExit`) lives in
//! `agg_gui::App::on_mouse_leave`, which never tells a captured drag the
//! pointer left; winit's `CursorLeft` carries no position for the
//! cursor-rect geometry test, and the recovery calls
//! (`ReleaseCapturedButtonsWhere`, `ClearCapturedButtons`) have no winit
//! caller, so they are not ported.

use agg_gui::MouseButton;

/// The kind of pointer event [`OutOfViewMouseCapture::should_deliver`] is
/// asked about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PointerEventKind {
    /// Anything that is not a press, a release or a drag: a hover move, a scroll.
    Other,
    /// A button going down.
    Down,
    /// A button coming up.
    Up,
    /// A move with a button held.
    Drag,
}

/// The buttons whose press the content area saw and whose release has not
/// come yet.
#[derive(Debug, Default)]
pub(crate) struct OutOfViewMouseCapture {
    // Not a bit set: more than one button can be held at once, and
    // `MouseButton::Other` carries a number.
    captured_buttons: Vec<MouseButton>,
}

impl OutOfViewMouseCapture {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Whether a drag this window owns is in flight, and so the pointer is
    /// its business wherever it is.
    pub(crate) fn has_captured_buttons(&self) -> bool {
        !self.captured_buttons.is_empty()
    }

    /// The kind of a winit `CursorMoved`, which carries no button: a drag
    /// while any captured button is held, else a hover.
    pub(crate) fn move_kind(&self) -> (PointerEventKind, Option<MouseButton>) {
        if !self.has_captured_buttons() {
            return (PointerEventKind::Other, None);
        }
        (PointerEventKind::Drag, self.captured_buttons.first().copied())
    }

    /// Whether `(x, y)` lies within a `width` x `height` content area. The
    /// edges count as inside, so the last row of pixels still belongs to it
    /// (C#'s `IsInsideBounds`).
    pub(crate) fn is_inside(x: f64, y: f64, width: f64, height: f64) -> bool {
        x >= 0.0 && y >= 0.0 && x <= width && y <= height
    }

    /// Decides whether an event should reach agg-gui, and updates the
    /// captured-button set. `button` is `None` for a hover or scroll.
    pub(crate) fn should_deliver(
        &mut self,
        kind: PointerEventKind,
        button: Option<MouseButton>,
        inside_view: bool,
    ) -> bool {
        match kind {
            PointerEventKind::Down => {
                if !inside_view {
                    return false;
                }
                if let Some(b) = button {
                    if !self.captured_buttons.contains(&b) {
                        self.captured_buttons.push(b);
                    }
                }
                true
            }
            PointerEventKind::Up => {
                // Removed whether or not it is delivered, so a button can
                // never stay captured.
                let before = self.captured_buttons.len();
                self.captured_buttons.retain(|b| Some(*b) != button);
                let was_captured = self.captured_buttons.len() != before;
                inside_view || was_captured
            }
            PointerEventKind::Drag => {
                inside_view || button.is_some_and(|b| self.captured_buttons.contains(&b))
            }
            PointerEventKind::Other => inside_view,
        }
    }
}

/// agg-sharp `OutOfViewMouseCaptureTests`, 1:1 (the macOS-only legs and the
/// recovery calls are not ported, see the header).
#[cfg(test)]
mod tests {
    use super::PointerEventKind::{Down, Drag, Other, Up};
    use super::*;

    const LEFT: Option<MouseButton> = Some(MouseButton::Left);
    const RIGHT: Option<MouseButton> = Some(MouseButton::Right);
    const MIDDLE: Option<MouseButton> = Some(MouseButton::Middle);

    #[test]
    fn a_drag_that_leaves_the_view_still_delivers_its_move_and_up() {
        let mut capture = OutOfViewMouseCapture::new();

        assert!(capture.should_deliver(Down, LEFT, true));
        assert!(capture.should_deliver(Drag, LEFT, false));

        // The one that matters: dropping this is what left the widget believing the button was still down.
        assert!(capture.should_deliver(Up, LEFT, false));
    }

    #[test]
    fn a_hover_outside_the_view_is_still_dropped() {
        let mut capture = OutOfViewMouseCapture::new();

        assert!(!capture.should_deliver(Other, None, false));
        assert!(capture.should_deliver(Other, None, true));
    }

    #[test]
    fn a_drag_ended_outside_does_not_capture_the_next_one() {
        let mut capture = OutOfViewMouseCapture::new();

        capture.should_deliver(Down, LEFT, true);
        capture.should_deliver(Up, LEFT, false);

        // With the button released, a drag whose down agg never saw is not this view's to deliver.
        assert!(!capture.should_deliver(Drag, LEFT, false));
        assert!(!capture.should_deliver(Up, LEFT, false));
    }

    #[test]
    fn a_title_bar_press_never_becomes_an_agg_drag_or_up() {
        let mut capture = OutOfViewMouseCapture::new();

        // The title bar is part of the same window, so its events reach this code; none of them are
        // agg's, and a phantom up out of one would be as bad as the missing up this class exists to fix.
        assert!(!capture.should_deliver(Down, LEFT, false));
        assert!(!capture.should_deliver(Drag, LEFT, false));
        assert!(!capture.should_deliver(Up, LEFT, false));
    }

    #[test]
    fn an_ordinary_in_view_drag_delivers_every_event_its_button_and_points() {
        let mut capture = OutOfViewMouseCapture::new();

        // The overwhelmingly common case, and the one a regression is most likely to break: press,
        // drag, release, all of it well inside the view. Nothing here may be filtered.
        assert!(capture.should_deliver(Down, LEFT, true));

        for _ in 0..5 {
            assert!(capture.should_deliver(Drag, LEFT, true));
        }

        assert!(capture.should_deliver(Up, LEFT, true));
    }

    #[test]
    fn a_drag_that_leaves_and_comes_back_keeps_delivering() {
        let mut capture = OutOfViewMouseCapture::new();

        capture.should_deliver(Down, LEFT, true);

        assert!(capture.should_deliver(Drag, LEFT, true));
        assert!(capture.should_deliver(Drag, LEFT, false));
        assert!(capture.should_deliver(Drag, LEFT, true));
        assert!(capture.should_deliver(Up, LEFT, true));
    }

    #[test]
    fn a_captured_drag_owns_the_pointer_until_its_button_comes_up() {
        let mut capture = OutOfViewMouseCapture::new();

        assert!(!capture.has_captured_buttons());

        capture.should_deliver(Down, LEFT, true);
        assert!(capture.has_captured_buttons());

        capture.should_deliver(Up, LEFT, false);
        assert!(!capture.has_captured_buttons());
    }

    #[test]
    fn each_button_is_captured_on_its_own() {
        let mut capture = OutOfViewMouseCapture::new();

        capture.should_deliver(Down, RIGHT, true);

        assert!(capture.should_deliver(Drag, RIGHT, false));
        assert!(!capture.should_deliver(Drag, MIDDLE, false));
    }

    /// winit's `CursorMoved` names no button: it is a drag of a captured
    /// button while one is held (so it survives leaving the window), and a
    /// hover otherwise; the edges of the content area count as inside.
    #[test]
    fn rust_only_a_cursor_move_is_a_drag_only_while_a_button_is_captured() {
        let mut capture = OutOfViewMouseCapture::new();
        assert_eq!(capture.move_kind(), (Other, None));
        capture.should_deliver(Down, RIGHT, true);
        assert_eq!(capture.move_kind(), (Drag, RIGHT));
        let (kind, button) = capture.move_kind();
        assert!(capture.should_deliver(kind, button, false));
        capture.should_deliver(Up, RIGHT, false);
        assert_eq!(capture.move_kind(), (Other, None));

        assert!(OutOfViewMouseCapture::is_inside(0.0, 0.0, 400.0, 300.0));
        assert!(OutOfViewMouseCapture::is_inside(400.0, 300.0, 400.0, 300.0));
        assert!(!OutOfViewMouseCapture::is_inside(-1.0, 10.0, 400.0, 300.0));
        assert!(!OutOfViewMouseCapture::is_inside(10.0, 301.0, 400.0, 300.0));
    }
}
