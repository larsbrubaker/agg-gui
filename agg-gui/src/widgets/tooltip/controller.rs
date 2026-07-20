//! Central tooltip controller — the first-class, wrapper-free tooltip path.
//!
//! Any widget can declare hover help by storing a string in its
//! [`WidgetBase::tooltip`](crate::layout_props::WidgetBase::tooltip) (via
//! [`Widget::with_tooltip`](crate::widget::Widget::with_tooltip)). Once per
//! frame the [`App`](crate::widget::App) finds the deepest hovered widget whose
//! [`tooltip_text`](crate::widget::Widget::tooltip_text) is `Some` and feeds it
//! to [`drive`]. This module owns the single, app-wide state machine that
//! decides *when* a tip is visible and, while it is, submits it to the shared
//! [`render`](super::render) queue so it paints in the global-overlay pass
//! above all clips.
//!
//! # One tip, app-wide
//!
//! There is exactly one controller (a thread-local — the GUI is single
//! threaded, matching [`super::timings`] / [`super::render`]). Because it keys
//! off the deepest *tipped* widget, only one tip is ever live. It shares the
//! reshow window in [`super::timings`] with the legacy [`Tooltip`](super::Tooltip)
//! wrapper, so moving between a wrapped control and a `with_tooltip` control
//! still gets the fast "reshow" delay and never shows two tips at once.
//!
//! # Timing
//!
//! The state machine mirrors [`Tooltip::update_visibility`](super::Tooltip):
//! initial delay → reshow delay when warm → autopop dismiss → hide-on-press /
//! hide-on-leave. It reuses [`super::timings`] wholesale rather than
//! duplicating the delays.
//!
//! # Font — host contract (required)
//!
//! The tip text is painted with the crate-wide font override
//! ([`font_settings::current_system_font`](crate::font_settings::current_system_font)),
//! read live on every paint so a tip re-renders in the new face the instant the
//! host swaps the system font. **This library is deliberately font-free: it
//! ships no bundled fallback.** Every host MUST call
//! [`font_settings::set_system_font`](crate::font_settings::set_system_font) with
//! a real font at startup, before any tip can appear. When no font is installed
//! the state machine still runs (timing, visibility) but [`Controller::submit`]
//! paints *nothing* — the tip is silently invisible. That is a host-setup bug,
//! not a headless-only edge case: the demos install their default font in
//! `demo-ui::font_init::init` precisely so tips are never blank. Tests install a
//! font explicitly via the same API.

use std::time::Duration;
use web_time::Instant;

use crate::geometry::{Point, Rect};
use crate::text::measure_advance;

use super::render::{
    current_tooltip_viewport, panel_size, place_panel, submit_tooltip, TooltipRequest,
};
use super::timings::{
    last_tooltip_visible_at, note_tooltip_visible, tooltip_now, tooltip_timings,
};
use super::{TooltipLine, TooltipLineKind, TOOLTIP_FONT_SIZE};

/// The single app-wide tip state machine. See the module docs.
#[derive(Default)]
struct Controller {
    /// Identity (child-index path) of the widget currently supplying the tip,
    /// used to detect target changes (move to another control ⇒ reshow).
    target: Option<Vec<usize>>,
    /// Tip text for the current target.
    text: String,
    /// Latest pointer position in world coords (the tip anchor).
    anchor: Point,
    /// When the pointer entered the current target.
    hover_started_at: Option<Instant>,
    /// Delay captured at target-enter: full initial, or the shorter reshow
    /// delay when the subsystem was warm.
    pending_delay: Duration,
    /// Whether the tip is currently visible.
    visible: bool,
    /// When the visible tip appeared, for autopop dismissal.
    tip_shown_at: Option<Instant>,
    /// Suppresses (re)showing until the pointer leaves and re-enters — set on a
    /// press and on autopop, mirroring Windows.
    suppressed: bool,
    /// Whether a mouse button is currently held (tips never show while pressed).
    pointer_down: bool,
    /// Panel rect from the last visible frame, in world coords — exposed to
    /// tests to assert edge clamping without paint introspection.
    last_rect: Option<Rect>,
}

thread_local! {
    static CONTROLLER: std::cell::RefCell<Controller> =
        std::cell::RefCell::new(Controller::default());
}

impl Controller {
    /// Delay for a hover starting now: the shorter reshow delay when a tip was
    /// visible within the reshow window, otherwise the full initial delay.
    fn effective_delay(&self) -> Duration {
        let t = tooltip_timings();
        match last_tooltip_visible_at() {
            Some(at) if tooltip_now().saturating_duration_since(at) <= t.reshow_window() => {
                t.reshow_delay
            }
            _ => t.initial_delay,
        }
    }

    /// Adopt `target` as the hovered tipped widget for this frame (or clear it),
    /// re-arming the hover timer on a target change.
    fn set_target(&mut self, target: Option<(Vec<usize>, String)>, anchor: Option<Point>) {
        if let Some(a) = anchor {
            self.anchor = a;
        }
        match target {
            Some((path, text)) => {
                if self.target.as_deref() != Some(path.as_slice()) {
                    // Entered a different tipped widget: re-arm.
                    self.target = Some(path);
                    self.hover_started_at = Some(tooltip_now());
                    self.pending_delay = self.effective_delay();
                    self.suppressed = false;
                    if self.visible {
                        self.hide();
                    }
                    crate::animation::request_draw_after(self.pending_delay);
                }
                self.text = text;
            }
            None => {
                if self.target.is_some() {
                    self.target = None;
                    self.hover_started_at = None;
                    self.suppressed = false;
                    self.hide();
                }
            }
        }
    }

    fn should_show(&self) -> bool {
        if self.suppressed || self.pointer_down || self.target.is_none() {
            return false;
        }
        match self.hover_started_at {
            Some(started) => tooltip_now().saturating_duration_since(started) >= self.pending_delay,
            None => false,
        }
    }

    fn remaining_delay(&self) -> Option<Duration> {
        if self.target.is_none() || self.suppressed || self.pointer_down {
            return None;
        }
        let elapsed = tooltip_now().saturating_duration_since(self.hover_started_at?);
        Some(self.pending_delay.saturating_sub(elapsed))
    }

    fn hide(&mut self) {
        if self.visible {
            crate::animation::request_draw();
        }
        self.visible = false;
        self.tip_shown_at = None;
        self.last_rect = None;
    }

    /// Advance the state machine; returns whether the tip is visible now.
    fn update_visibility(&mut self) -> bool {
        if self.visible {
            if let Some(shown) = self.tip_shown_at {
                let autopop = tooltip_timings().autopop;
                let elapsed = tooltip_now().saturating_duration_since(shown);
                if elapsed >= autopop {
                    self.hide();
                    self.suppressed = true;
                    return false;
                }
                crate::animation::request_draw_after(autopop - elapsed);
            }
        }

        if self.should_show() {
            if !self.visible {
                self.visible = true;
                self.tip_shown_at = Some(tooltip_now());
                crate::animation::request_draw();
                crate::animation::request_draw_after(tooltip_timings().autopop);
            }
            note_tooltip_visible();
            return true;
        }

        if self.visible {
            self.hide();
        } else if let Some(remaining) = self.remaining_delay() {
            if remaining.is_zero() {
                crate::animation::request_draw();
            } else {
                crate::animation::request_draw_after(remaining);
            }
        }
        false
    }

    /// When visible, compute the placed panel rect (for tests) and submit the
    /// tip to the shared render queue so it paints in the global-overlay pass.
    fn submit(&mut self) {
        // Host contract: a system font MUST be installed via
        // `font_settings::set_system_font` before a tip can paint (this library
        // ships no bundled fallback — see the module docs). Re-read live every
        // paint so a tip follows a mid-session font swap. When none is present
        // there is nothing to paint; keep the state machine state and bail.
        let Some(font) = crate::font_settings::current_system_font() else {
            self.last_rect = None;
            return;
        };
        let text_w = measure_advance(&font, &self.text, TOOLTIP_FONT_SIZE);
        let size = panel_size(text_w, 1);
        let viewport = current_tooltip_viewport();
        if viewport.width > 0.0 && viewport.height > 0.0 {
            self.last_rect = Some(place_panel(self.anchor, size, viewport, true));
        }
        submit_tooltip(TooltipRequest {
            font,
            lines: vec![TooltipLine {
                text: self.text.clone(),
                kind: TooltipLineKind::Text,
            }],
            anchor: self.anchor,
            at_pointer: true,
        });
    }
}

/// Per-frame entry point called by the App's tooltip pass. `target` is the
/// deepest hovered widget carrying a tip (its index-path identity + text), or
/// `None` when nothing tipped is hovered. `anchor` is the pointer world
/// position. Runs the state machine and, if a tip is visible, submits it.
pub(crate) fn drive(target: Option<(Vec<usize>, String)>, anchor: Option<Point>) {
    CONTROLLER.with(|c| {
        let mut c = c.borrow_mut();
        c.set_target(target, anchor);
        if c.update_visibility() {
            c.submit();
        }
    });
}

/// A press hides any visible tip and keeps it hidden until the pointer leaves
/// and re-enters a tipped widget (Windows press-hide behaviour).
pub(crate) fn on_pointer_down() {
    CONTROLLER.with(|c| {
        let mut c = c.borrow_mut();
        c.pointer_down = true;
        c.suppressed = true;
        c.hide();
    });
}

/// Release clears the held-button flag (re-showing still waits on re-entry
/// because `suppressed` stays set until the target changes).
pub(crate) fn on_pointer_up() {
    CONTROLLER.with(|c| c.borrow_mut().pointer_down = false);
}

// --- Test / introspection hooks ------------------------------------------------

/// Whether a tip is currently visible.
#[doc(hidden)]
pub fn is_visible() -> bool {
    CONTROLLER.with(|c| c.borrow().visible)
}

/// The visible tip's text, or `None` when hidden.
#[doc(hidden)]
pub fn visible_text() -> Option<String> {
    CONTROLLER.with(|c| {
        let c = c.borrow();
        c.visible.then(|| c.text.clone())
    })
}

/// The visible tip's placed panel rect (world coords), or `None`.
#[doc(hidden)]
pub fn visible_rect() -> Option<Rect> {
    CONTROLLER.with(|c| c.borrow().last_rect)
}

/// Reset the controller to its initial state (tests: avoid leaking state across
/// cases on a reused harness thread).
#[doc(hidden)]
pub fn reset() {
    CONTROLLER.with(|c| *c.borrow_mut() = Controller::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw_ctx::DrawCtx;
    use crate::event::{Event, EventResult};
    use crate::geometry::{Rect, Size};
    use crate::layout_props::WidgetBase;
    use crate::text::Font;
    use crate::widget::{App, Widget};
    use std::sync::Arc;

    use super::super::timings::{
        advance_tooltip_test_clock, reset_tooltip_test_state, set_tooltip_test_clock,
    };

    const FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf");

    /// Pin the tooltip clock and clear both the shared timing state and the
    /// controller so cases do not leak into one another; restores on drop.
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            reset_tooltip_test_state();
            reset();
            crate::font_settings::set_system_font(None);
        }
    }
    fn pin() -> Guard {
        reset_tooltip_test_state();
        reset();
        set_tooltip_test_clock(Some(Instant::now()));
        crate::font_settings::set_system_font(Some(Arc::new(
            Font::from_bytes(FONT_BYTES.to_vec()).expect("bundled font"),
        )));
        Guard
    }

    /// A leaf that fills whatever bounds it is given and carries a tip on its
    /// `WidgetBase`, so hovering anywhere in the viewport targets it.
    struct Tipped {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
        base: WidgetBase,
    }
    impl Tipped {
        fn new(tip: &str) -> Self {
            Self {
                bounds: Rect::default(),
                children: Vec::new(),
                base: WidgetBase::new().with_tooltip(tip),
            }
        }
    }
    impl Widget for Tipped {
        fn bounds(&self) -> Rect {
            self.bounds
        }
        fn set_bounds(&mut self, b: Rect) {
            self.bounds = b;
        }
        fn children(&self) -> &[Box<dyn Widget>] {
            &self.children
        }
        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
            &mut self.children
        }
        fn layout(&mut self, available: Size) -> Size {
            self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
            available
        }
        fn paint(&mut self, _: &mut dyn DrawCtx) {}
        fn on_event(&mut self, _: &Event) -> EventResult {
            EventResult::Ignored
        }
        fn widget_base(&self) -> Option<&WidgetBase> {
            Some(&self.base)
        }
        fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
            Some(&mut self.base)
        }
    }

    /// Move the pointer to world `(wx, wy)` in an 800×600 viewport (scale 1).
    fn hover(app: &mut App, wx: f64, wy: f64) {
        app.on_mouse_move(wx, 600.0 - wy);
    }

    /// (a) A `with_tooltip` widget shows its tip after the initial delay.
    #[test]
    fn tip_shows_after_initial_delay_when_hovered() {
        let _g = pin();
        let timings = tooltip_timings();
        let mut app = App::new(Box::new(Tipped::new("Bold")));
        app.layout(Size::new(800.0, 600.0));

        hover(&mut app, 400.0, 300.0);
        app.update_tooltips_for_test();
        assert!(!is_visible(), "no tip before the initial delay elapses");

        advance_tooltip_test_clock(timings.initial_delay);
        app.update_tooltips_for_test();
        assert!(is_visible(), "tip appears once the initial delay elapses");
        assert_eq!(visible_text().as_deref(), Some("Bold"));
    }

    /// (b) Moving between two tipped widgets uses the reshow delay and never
    /// shows more than one tip: only the second widget's tip is live.
    #[test]
    fn moving_between_tipped_widgets_reshows_single_tip() {
        let _g = pin();
        let timings = tooltip_timings();
        let anchor = Some(Point::new(100.0, 100.0));

        // Enter A and show its tip, warming the reshow window.
        drive(Some((vec![0], "A".into())), anchor);
        advance_tooltip_test_clock(timings.initial_delay);
        drive(Some((vec![0], "A".into())), anchor);
        assert_eq!(visible_text().as_deref(), Some("A"));

        // Leave A (tip hides, window stays warm), then a short move to B.
        drive(None, anchor);
        assert!(!is_visible());
        advance_tooltip_test_clock(timings.reshow_delay);
        drive(Some((vec![1], "B".into())), anchor);
        assert!(!is_visible(), "B not shown until the (short) reshow delay elapses");

        // After only the reshow delay — well short of a full initial delay — B's
        // tip appears, and it is the only tip.
        advance_tooltip_test_clock(timings.reshow_delay);
        drive(Some((vec![1], "B".into())), anchor);
        assert!(is_visible());
        assert_eq!(visible_text().as_deref(), Some("B"), "exactly one tip: B's");
    }

    /// (c) A visible tip re-renders with the CURRENT system font: swapping the
    /// crate-wide font mid-hover makes the very next paint submit the new face.
    /// Pins the "font read live every paint" contract the module docs promise —
    /// the controller must never cache the font it first painted with.
    #[test]
    fn visible_tip_follows_live_system_font_swap() {
        let _g = pin();
        let timings = tooltip_timings();
        let anchor = Some(Point::new(100.0, 100.0));

        // Two distinct fonts (same bytes) tell apart by Arc pointer identity.
        let font_a = Arc::new(Font::from_bytes(FONT_BYTES.to_vec()).expect("font a"));
        let font_b = Arc::new(Font::from_bytes(FONT_BYTES.to_vec()).expect("font b"));
        assert!(!Arc::ptr_eq(&font_a, &font_b));

        // Install A, hover, and let the tip appear: it submits with font A.
        crate::font_settings::set_system_font(Some(Arc::clone(&font_a)));
        drive(Some((vec![0], "Tip".into())), anchor);
        advance_tooltip_test_clock(timings.initial_delay);
        drive(Some((vec![0], "Tip".into())), anchor);
        assert!(is_visible(), "tip visible after the initial delay");
        let submitted_a =
            super::super::render::last_submitted_font().expect("tip submitted a request");
        assert!(
            Arc::ptr_eq(&submitted_a, &font_a),
            "tip first paints with the installed font A"
        );

        // Swap the system font live and paint again WITHOUT re-hovering: the
        // controller must re-read the font and submit B, not the cached A.
        crate::font_settings::set_system_font(Some(Arc::clone(&font_b)));
        drive(Some((vec![0], "Tip".into())), anchor);
        let submitted_b =
            super::super::render::last_submitted_font().expect("tip re-submitted after swap");
        assert!(
            Arc::ptr_eq(&submitted_b, &font_b),
            "still-visible tip re-renders with the NEW system font B"
        );
        assert!(
            !Arc::ptr_eq(&submitted_b, &font_a),
            "the swapped tip must not keep painting with the old font A"
        );
    }

    /// (d) A tipped widget hovered at the right viewport edge paints its tip
    /// fully inside the viewport safe area (edge clamping).
    #[test]
    fn tip_at_viewport_edge_stays_inside() {
        let _g = pin();
        let timings = tooltip_timings();
        let mut app = App::new(Box::new(Tipped::new("Increase indent")));
        let viewport = Size::new(800.0, 600.0);
        app.layout(viewport);

        // Hover hard against the right edge; the pointer-anchored panel would
        // overflow to the right if it were not clamped. The first pass arms the
        // hover timer; the tip shows on the pass after the initial delay.
        hover(&mut app, 798.0, 300.0);
        app.update_tooltips_for_test();
        advance_tooltip_test_clock(timings.initial_delay);
        app.update_tooltips_for_test();

        assert!(is_visible());
        let r = visible_rect().expect("visible tip has a placed rect");
        assert!(r.x >= 0.0, "left edge inside viewport: {r:?}");
        assert!(
            r.x + r.width <= viewport.width,
            "right edge inside viewport: {r:?}"
        );
        assert!(r.y >= 0.0 && r.y + r.height <= viewport.height, "vertically inside: {r:?}");
    }
}
