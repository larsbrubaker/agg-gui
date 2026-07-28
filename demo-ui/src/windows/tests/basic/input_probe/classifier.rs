//! Pure interaction-classification logic for the Input Test probes.
//!
//! This module is the testable heart of `input_probe`: it turns discrete
//! pointer callbacks (`on_down`/`on_move`/`on_up`) plus a monotonic millisecond
//! clock into higher-level [`Interaction`]s (clicks with double/triple counts,
//! drag start/move/stop, hover enter/leave).  It holds no widget or `Instant`
//! state, so the unit tests below drive it deterministically.  The widget shell
//! that renders these interactions lives in the sibling `mod.rs`.

use agg_gui::{MouseButton, Point};

// ---------------------------------------------------------------------------
// Classifier constants — deliberately match the Scene's click/drag convention
// (see `agg-gui/src/widgets/scene.rs`: `DBL_CLICK_MS`, `MAX_CLICK_DIST`).
// ---------------------------------------------------------------------------

/// Maximum gap between two clicks (ms) for the second to extend the sequence
/// into a double / triple click.  Matches the Scene's 400 ms window.
const DBL_CLICK_MS: f64 = 400.0;

/// Maximum pointer travel (logical px) for a press-release pair to still count
/// as a *click*, and for successive clicks to be considered "the same spot".
const MAX_CLICK_DIST: f64 = 6.0;

/// Pointer travel (logical px) past which a press becomes a *drag*.  Same
/// tolerance as [`MAX_CLICK_DIST`] so a gesture is exactly one of click/drag.
const DRAG_THRESHOLD: f64 = 6.0;

/// The consumption profile of a probe — the honest agg-gui counterpart of an
/// `egui::Sense`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeKind {
    /// Consumes nothing; only reports contains-pointer and hover enter/leave.
    Hover,
    /// Consumes press/release; classifies click / double / triple click.
    Click,
    /// Consumes the press (captures the pointer); reports drag start/move/stop.
    Drag,
    /// Consumes both — clicks *and* drags.
    ClickAndDrag,
}

impl ProbeKind {
    /// Short human title shown at the top of the probe.
    pub fn title(self) -> &'static str {
        match self {
            ProbeKind::Hover => "Hover",
            ProbeKind::Click => "Click",
            ProbeKind::Drag => "Drag",
            ProbeKind::ClickAndDrag => "Click + Drag",
        }
    }

    /// One-line description of what the widget does with events, phrased in
    /// agg-gui terms (`on_event` + `EventResult`), not egui's `Sense`.
    pub fn profile(self) -> &'static str {
        match self {
            ProbeKind::Hover => "on_event -> Ignored (never consumes)",
            ProbeKind::Click => "consumes press/release",
            ProbeKind::Drag => "consumes press; captures pointer",
            ProbeKind::ClickAndDrag => "consumes press/release + drag",
        }
    }
}

/// A single classified interaction produced by [`InteractionClassifier`].
///
/// `Dragged` carries the incremental delta since the previous move; everything
/// else is a discrete event.  Not `Eq` because of the `f64` deltas — tests use
/// exact `PartialEq`, which is fine for the integer inputs they feed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Interaction {
    HoverEnter,
    HoverLeave,
    /// `count` is 1 = click, 2 = double, 3 = triple (and up — see
    /// [`click_label`]).
    Click {
        button: MouseButton,
        count: u32,
    },
    DragStarted {
        button: MouseButton,
    },
    Dragged {
        button: MouseButton,
        dx: f64,
        dy: f64,
    },
    DragStopped {
        button: MouseButton,
    },
}

/// Tracks an in-progress press for drag classification.
#[derive(Clone, Copy, Debug)]
struct Press {
    button: MouseButton,
    origin: Point,
    last: Point,
    /// Set once the pointer has travelled past [`DRAG_THRESHOLD`].
    dragging: bool,
}

/// Remembers the most recent completed click so the next one can extend the
/// double/triple sequence.
#[derive(Clone, Copy, Debug)]
struct LastClick {
    button: MouseButton,
    time_ms: f64,
    pos: Point,
    count: u32,
}

/// Pure classification logic shared by every probe.  It is driven by discrete
/// pointer callbacks and a monotonic millisecond clock, so it can be unit-tested
/// deterministically with no `Instant` or widget in the loop.
pub struct InteractionClassifier {
    kind: ProbeKind,
    press: Option<Press>,
    last_click: Option<LastClick>,
    /// Whether the pointer is currently inside the probe (hover state).
    inside: bool,
}

impl InteractionClassifier {
    pub fn new(kind: ProbeKind) -> Self {
        Self {
            kind,
            press: None,
            last_click: None,
            inside: false,
        }
    }

    /// `true` while a button is held (i.e. a gesture is in progress). The widget
    /// uses this to keep consuming captured drag moves.
    pub fn is_pressed(&self) -> bool {
        self.press.is_some()
    }

    /// `true` for any kind except `Hover`, which never records a press.
    fn tracks_press(&self) -> bool {
        !matches!(self.kind, ProbeKind::Hover)
    }

    /// A button was pressed inside the probe. Records the press for
    /// click/drag kinds; hover ignores it. Produces no interaction on its own.
    pub fn on_down(&mut self, button: MouseButton, pos: Point, _now_ms: f64) -> Vec<Interaction> {
        if self.tracks_press() {
            self.press = Some(Press {
                button,
                origin: pos,
                last: pos,
                dragging: false,
            });
        }
        Vec::new()
    }

    /// The pointer moved to `pos`; `inside` reports whether that point lies
    /// within the probe (the widget computes it from its bounds). Emits hover
    /// enter/leave when idle and drag start/move while a drag gesture is live.
    pub fn on_move(&mut self, pos: Point, inside: bool, _now_ms: f64) -> Vec<Interaction> {
        let mut out = Vec::new();

        // Hover transitions only while no button is held — a captured drag that
        // leaves the bounds must not spam enter/leave.
        if self.press.is_none() {
            if inside && !self.inside {
                self.inside = true;
                out.push(Interaction::HoverEnter);
            } else if !inside && self.inside {
                self.inside = false;
                out.push(Interaction::HoverLeave);
            }
        }

        // `take`/re-insert avoids a borrow conflict between `self.press` and
        // `self.last_click` when a drag start clears the click sequence.
        if let Some(mut press) = self.press.take() {
            if matches!(self.kind, ProbeKind::Drag | ProbeKind::ClickAndDrag) {
                let tdx = pos.x - press.origin.x;
                let tdy = pos.y - press.origin.y;
                if !press.dragging && (tdx * tdx + tdy * tdy).sqrt() > DRAG_THRESHOLD {
                    press.dragging = true;
                    self.last_click = None; // a drag breaks any click sequence
                    out.push(Interaction::DragStarted {
                        button: press.button,
                    });
                }
                if press.dragging {
                    out.push(Interaction::Dragged {
                        button: press.button,
                        dx: pos.x - press.last.x,
                        dy: pos.y - press.last.y,
                    });
                }
            }
            press.last = pos;
            self.press = Some(press);
        }
        out
    }

    /// A button was released at `pos`. Completes a click (with double/triple
    /// detection) or a drag depending on the kind and whether the gesture moved.
    pub fn on_up(&mut self, button: MouseButton, pos: Point, now_ms: f64) -> Vec<Interaction> {
        let mut out = Vec::new();
        let Some(press) = self.press.take() else {
            return out;
        };
        // A release of a different button than the one held leaves the press
        // intact (we only track a single primary gesture).
        if press.button != button {
            self.press = Some(press);
            return out;
        }

        match self.kind {
            ProbeKind::Hover => {}
            ProbeKind::Drag => {
                if press.dragging {
                    out.push(Interaction::DragStopped { button });
                }
            }
            ProbeKind::ClickAndDrag => {
                if press.dragging {
                    out.push(Interaction::DragStopped { button });
                } else {
                    out.push(self.classify_click(button, pos, now_ms));
                }
            }
            ProbeKind::Click => {
                // Only a press-release that stayed within tolerance is a click.
                if dist(press.origin, pos) <= MAX_CLICK_DIST {
                    out.push(self.classify_click(button, pos, now_ms));
                }
            }
        }
        out
    }

    /// Decide the click count (1/2/3…) for a completed click and remember it so
    /// a follow-up click within the window can extend the sequence.
    fn classify_click(&mut self, button: MouseButton, pos: Point, now_ms: f64) -> Interaction {
        let count = match &self.last_click {
            Some(lc)
                if lc.button == button
                    && now_ms - lc.time_ms < DBL_CLICK_MS
                    && dist(lc.pos, pos) <= MAX_CLICK_DIST =>
            {
                lc.count + 1
            }
            _ => 1,
        };
        self.last_click = Some(LastClick {
            button,
            time_ms: now_ms,
            pos,
            count,
        });
        Interaction::Click { button, count }
    }
}

/// Euclidean distance between two points.
fn dist(a: Point, b: Point) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

/// Label for a click of the given count: 1 → "Clicked", 2 → "Double-clicked",
/// 3+ → "Triple-clicked" (matching egui's wording).
fn click_label(count: u32) -> &'static str {
    match count {
        0 | 1 => "Clicked",
        2 => "Double-clicked",
        _ => "Triple-clicked",
    }
}

/// egui-style " by {button} button" suffix; empty for the primary (Left) button
/// to reduce clutter in the common case.
fn button_suffix(button: MouseButton) -> String {
    match button {
        MouseButton::Left => String::new(),
        MouseButton::Middle => " by Middle button".to_string(),
        MouseButton::Right => " by Right button".to_string(),
        MouseButton::Other(n) => format!(" by Other({n}) button"),
    }
}

/// Render an [`Interaction`] into `(summary, full, is_hover)`.
///
/// `summary` is the dedup key (so repeated `Dragged`s coalesce with an ×N
/// counter); `full` carries extra detail (the drag delta); `is_hover` gates the
/// entry behind the "Include hover events" checkbox.
pub fn describe(i: Interaction) -> (String, String, bool) {
    match i {
        Interaction::HoverEnter => ("Hover enter".to_string(), "Hover enter".to_string(), true),
        Interaction::HoverLeave => ("Hover leave".to_string(), "Hover leave".to_string(), true),
        Interaction::Click { button, count } => {
            let s = format!("{}{}", click_label(count), button_suffix(button));
            (s.clone(), s, false)
        }
        Interaction::DragStarted { button } => {
            let s = format!("Drag started{}", button_suffix(button));
            (s.clone(), s, false)
        }
        Interaction::Dragged { button, dx, dy } => {
            let s = format!("Dragged{}", button_suffix(button));
            let full = format!("{s} (\u{0394}{dx:.0}, {dy:.0})");
            (s, full, false)
        }
        Interaction::DragStopped { button } => {
            let s = format!("Drag stopped{}", button_suffix(button));
            (s.clone(), s, false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    /// A press-release that stays put is a single click.
    #[test]
    fn single_click() {
        let mut c = InteractionClassifier::new(ProbeKind::Click);
        assert!(c.on_down(MouseButton::Left, p(10.0, 10.0), 0.0).is_empty());
        let out = c.on_up(MouseButton::Left, p(10.0, 10.0), 5.0);
        assert_eq!(
            out,
            vec![Interaction::Click {
                button: MouseButton::Left,
                count: 1
            }]
        );
    }

    /// Two clicks within the window escalate to a double-click; a third to a
    /// triple.
    #[test]
    fn double_then_triple_click() {
        let mut c = InteractionClassifier::new(ProbeKind::Click);

        c.on_down(MouseButton::Left, p(10.0, 10.0), 0.0);
        assert_eq!(
            c.on_up(MouseButton::Left, p(10.0, 10.0), 10.0),
            vec![Interaction::Click {
                button: MouseButton::Left,
                count: 1
            }]
        );

        c.on_down(MouseButton::Left, p(11.0, 11.0), 100.0);
        assert_eq!(
            c.on_up(MouseButton::Left, p(11.0, 11.0), 110.0),
            vec![Interaction::Click {
                button: MouseButton::Left,
                count: 2
            }]
        );

        c.on_down(MouseButton::Left, p(10.0, 12.0), 200.0);
        assert_eq!(
            c.on_up(MouseButton::Left, p(10.0, 12.0), 210.0),
            vec![Interaction::Click {
                button: MouseButton::Left,
                count: 3
            }]
        );
    }

    /// A second click after the double-click window resets the sequence.
    #[test]
    fn slow_second_click_resets_count() {
        let mut c = InteractionClassifier::new(ProbeKind::Click);
        c.on_down(MouseButton::Left, p(10.0, 10.0), 0.0);
        c.on_up(MouseButton::Left, p(10.0, 10.0), 10.0);

        // 500 ms later — well past DBL_CLICK_MS (400).
        c.on_down(MouseButton::Left, p(10.0, 10.0), 500.0);
        assert_eq!(
            c.on_up(MouseButton::Left, p(10.0, 10.0), 510.0),
            vec![Interaction::Click {
                button: MouseButton::Left,
                count: 1
            }]
        );
    }

    /// A click too far from the previous one restarts the sequence even within
    /// the time window.
    #[test]
    fn far_second_click_resets_count() {
        let mut c = InteractionClassifier::new(ProbeKind::Click);
        c.on_down(MouseButton::Left, p(10.0, 10.0), 0.0);
        c.on_up(MouseButton::Left, p(10.0, 10.0), 10.0);

        c.on_down(MouseButton::Left, p(100.0, 100.0), 100.0);
        assert_eq!(
            c.on_up(MouseButton::Left, p(100.0, 100.0), 110.0),
            vec![Interaction::Click {
                button: MouseButton::Left,
                count: 1
            }]
        );
    }

    /// A left click followed by a right click are independent sequences.
    #[test]
    fn per_button_separation() {
        let mut c = InteractionClassifier::new(ProbeKind::Click);
        c.on_down(MouseButton::Left, p(10.0, 10.0), 0.0);
        assert_eq!(
            c.on_up(MouseButton::Left, p(10.0, 10.0), 10.0),
            vec![Interaction::Click {
                button: MouseButton::Left,
                count: 1
            }]
        );
        // Right click right after — must NOT be a double-click.
        c.on_down(MouseButton::Right, p(10.0, 10.0), 50.0);
        assert_eq!(
            c.on_up(MouseButton::Right, p(10.0, 10.0), 60.0),
            vec![Interaction::Click {
                button: MouseButton::Right,
                count: 1
            }]
        );
    }

    /// On a Click-only probe, moving past the tolerance cancels the click.
    #[test]
    fn click_probe_ignores_dragged_release() {
        let mut c = InteractionClassifier::new(ProbeKind::Click);
        c.on_down(MouseButton::Left, p(10.0, 10.0), 0.0);
        // A Click probe does not track drags, so on_move yields nothing; the
        // release lands far from the press and is not classified as a click.
        assert!(c.on_move(p(40.0, 40.0), true, 20.0).is_empty());
        assert!(c.on_up(MouseButton::Left, p(40.0, 40.0), 30.0).is_empty());
    }

    /// A Drag probe reports start → dragged → stopped once past the threshold.
    #[test]
    fn drag_lifecycle() {
        let mut c = InteractionClassifier::new(ProbeKind::Drag);
        assert!(c.on_down(MouseButton::Left, p(0.0, 0.0), 0.0).is_empty());

        // Sub-threshold move: nothing yet.
        assert!(c.on_move(p(3.0, 0.0), true, 5.0).is_empty());

        // Cross the threshold: drag starts and reports the incremental delta.
        let out = c.on_move(p(10.0, 0.0), true, 10.0);
        assert_eq!(
            out,
            vec![
                Interaction::DragStarted {
                    button: MouseButton::Left
                },
                Interaction::Dragged {
                    button: MouseButton::Left,
                    dx: 7.0,
                    dy: 0.0
                },
            ]
        );

        // Continued drag: only the incremental delta.
        assert_eq!(
            c.on_move(p(15.0, 4.0), true, 15.0),
            vec![Interaction::Dragged {
                button: MouseButton::Left,
                dx: 5.0,
                dy: 4.0
            }]
        );

        // Release ends the drag.
        assert_eq!(
            c.on_up(MouseButton::Left, p(15.0, 4.0), 20.0),
            vec![Interaction::DragStopped {
                button: MouseButton::Left
            }]
        );
    }

    /// A Drag probe that never crosses the threshold is neither a drag nor a
    /// click.
    #[test]
    fn tiny_drag_gesture_is_nothing() {
        let mut c = InteractionClassifier::new(ProbeKind::Drag);
        c.on_down(MouseButton::Left, p(0.0, 0.0), 0.0);
        assert!(c.on_move(p(2.0, 2.0), true, 5.0).is_empty());
        assert!(c.on_up(MouseButton::Left, p(2.0, 2.0), 10.0).is_empty());
    }

    /// Click+Drag classifies a still gesture as a click and a moved one as a
    /// drag.
    #[test]
    fn click_and_drag_distinguishes() {
        // Still gesture → click.
        let mut c = InteractionClassifier::new(ProbeKind::ClickAndDrag);
        c.on_down(MouseButton::Left, p(0.0, 0.0), 0.0);
        assert_eq!(
            c.on_up(MouseButton::Left, p(1.0, 1.0), 10.0),
            vec![Interaction::Click {
                button: MouseButton::Left,
                count: 1
            }]
        );

        // Moved gesture → drag, no click.
        let mut c = InteractionClassifier::new(ProbeKind::ClickAndDrag);
        c.on_down(MouseButton::Left, p(0.0, 0.0), 0.0);
        let started = c.on_move(p(20.0, 0.0), true, 5.0);
        assert!(started.contains(&Interaction::DragStarted {
            button: MouseButton::Left
        }));
        let up = c.on_up(MouseButton::Left, p(20.0, 0.0), 10.0);
        assert_eq!(
            up,
            vec![Interaction::DragStopped {
                button: MouseButton::Left
            }]
        );
    }

    /// Hover enter/leave fire on move transitions and only while idle.
    #[test]
    fn hover_enter_and_leave() {
        let mut c = InteractionClassifier::new(ProbeKind::Hover);
        assert_eq!(
            c.on_move(p(5.0, 5.0), true, 0.0),
            vec![Interaction::HoverEnter]
        );
        // Still inside — no repeat.
        assert!(c.on_move(p(6.0, 6.0), true, 5.0).is_empty());
        // Leave.
        assert_eq!(
            c.on_move(p(-1.0, -1.0), false, 10.0),
            vec![Interaction::HoverLeave]
        );
    }

    /// A Hover probe never records a press, so it emits hover events only —
    /// never a click or a drag, no matter how the pointer moves while "held".
    #[test]
    fn hover_probe_never_clicks_or_drags() {
        let is_click_or_drag = |i: &Interaction| {
            matches!(
                i,
                Interaction::Click { .. }
                    | Interaction::DragStarted { .. }
                    | Interaction::Dragged { .. }
                    | Interaction::DragStopped { .. }
            )
        };
        let mut c = InteractionClassifier::new(ProbeKind::Hover);
        assert!(c.on_down(MouseButton::Left, p(5.0, 5.0), 0.0).is_empty());
        // Entering emits HoverEnter (a hover event), but never click/drag.
        assert!(!c
            .on_move(p(40.0, 40.0), true, 5.0)
            .iter()
            .any(is_click_or_drag));
        assert!(!c
            .on_up(MouseButton::Left, p(40.0, 40.0), 10.0)
            .iter()
            .any(is_click_or_drag));
    }
}
