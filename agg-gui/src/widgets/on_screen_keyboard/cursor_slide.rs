//! Moving the caret from the keyboard, as the phone's own keyboard does.
//!
//! - **Gboard (Android):** a finger that slides along the space bar moves the
//!   caret one character per [`SPACE_STEP`] of travel, with a haptic tick
//!   each step; once it has travelled [`ENGAGE_DISTANCE`] sideways the press
//!   is a slide and its lift types no space. A finger that slides left from
//!   Backspace selects whole words (one per [`DELETE_WORD_STEP`]), gives them
//!   back as it returns, and the lift deletes what is selected.
//! - **iPhone:** a space bar held still for [`TRACKPAD_HOLD`] turns the
//!   keyboard into a trackpad: the key labels fade, the finger moves the
//!   caret by character ([`TRACKPAD_CHAR_STEP`]) and by line
//!   ([`TRACKPAD_LINE_STEP`]) anywhere on screen, and the lift ends it.
//!
//! The caret moves as arrow keys (Shift with the platform's word modifier for
//! the Backspace slide), so every text widget follows without knowing the
//! keyboard is there; they go through the caret queue (`caret_ops.rs`), whose
//! answers say which steps moved the caret (a haptic tick each, and only
//! words actually selected count) and hold the selection the gesture began
//! from. Taking the Backspace slide back to its start puts that selection
//! back exactly (word-right is no inverse of word-left: it also skips the
//! spaces after a word); so do a cancelled touch and a second finger landing
//! during the Backspace slide or the trackpad, which delete nothing. A
//! running dictation ends the moment a gesture takes over, as for any key.
//!
//! "Never lose a tap to a drag": a press stays a key press, typing on its
//! lift as [`super::pointer`] decides, until the finger has clearly moved the
//! gesture's way; a press that moves the other way first (up to a letter) is
//! a key press for good. A held Backspace that has begun repeating stays a
//! repeat. [`super::pointer`] calls [`arm`], [`track`], [`release`] and
//! [`cancel`] from its press, move, lift and cancel handlers, and the paint
//! pass calls [`tick`] (the iPhone hold) and reads [`label_alpha`].

use std::time::Duration;

use web_time::Instant;

use crate::event::{Key, Modifiers};
use crate::geometry::Point;

use super::caret_ops::{self, notify_ticks, word_select_modifiers, CaretOp, StepKind};
use super::key::KeyAction;
use super::layouts::LayoutFamily;
use super::state::{with_state_mut, with_state_ref, KeyboardState};

// Every distance here is in CSS px (Android's dp, the iPhone's pt) on screen:
// the keyboard keeps the phone keyboard's own size whatever the app's UX zoom
// (`style.rs`), and so do its steps. Finger positions are converted with
// `on_screen`.

/// Sideways travel that turns a press into a slide: the app's touch tap
/// tolerance, so the keyboard and the rest of the UI agree on what a tap is.
pub(crate) const ENGAGE_DISTANCE: f64 = crate::touch_emulation::TOUCH_SCROLL_THRESHOLD;
/// Gboard: finger travel (logical points, i.e. dp) per character the caret
/// moves. About a quarter of a letter key, so a single character is easy to
/// hit (≈1.6 mm a step) and a slide across the space bar moves ~20.
pub(crate) const SPACE_STEP: f64 = 10.0;
/// Gboard: finger travel per word the Backspace slide selects; words are
/// longer than characters, so the step is twice the space bar's. The slide
/// starts at its first word: a shorter flick is still a Backspace press.
pub(crate) const DELETE_WORD_STEP: f64 = 20.0;
/// iPhone: how long the space bar is held still before the trackpad starts.
pub(crate) const TRACKPAD_HOLD: Duration = Duration::from_millis(500);
/// iPhone trackpad: finger travel per character, about one glyph of body text.
pub(crate) const TRACKPAD_CHAR_STEP: f64 = 8.0;
/// iPhone trackpad: finger travel per line, about one line of body text.
pub(crate) const TRACKPAD_LINE_STEP: f64 = 20.0;
/// How long the key labels take to fade when the trackpad starts.
const LABEL_FADE: Duration = Duration::from_millis(150);

/// Which gesture a press may become.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureKind {
    /// Gboard's space bar slide.
    SpaceSlide,
    /// The iPhone's held space bar.
    Trackpad,
    /// Gboard's slide left from Backspace.
    DeleteSelect,
}

/// A press on a key that can move the caret, from touch-down to lift.
#[derive(Debug, Clone, Copy)]
pub struct CursorGesture {
    pub kind: GestureKind,
    /// The pressed key (index into `last_painted_keys`).
    pub key_index: usize,
    /// Where the finger came down (CSS px, Y-up).
    pub origin: Point,
    /// Where the finger is now (CSS px).
    pub last: Point,
    /// Where the next step on each axis is measured from (CSS px).
    pub anchor: Point,
    /// When the finger came down.
    pub pressed_at: Instant,
    /// When the press became the gesture; `None` while it is still a key
    /// press whose lift types.
    pub engaged_at: Option<Instant>,
    /// Words the Backspace slide's finger asks for.
    pub want: u32,
    /// Words the Backspace slide has selected (each step that moved the
    /// caret, as the App answered).
    pub selected: u32,
    /// A word step found nothing more to select (the start of the text):
    /// travel further left asks for no more words.
    pub capped: bool,
}

impl CursorGesture {
    fn engaged(&self) -> bool {
        self.engaged_at.is_some()
    }
}

/// A logical (UX-zoomed) point as CSS px on screen, the units of the steps.
fn on_screen(p: Point) -> Point {
    let zoom = crate::ux_scale::ux_scale().max(1e-6);
    Point::new(p.x * zoom, p.y * zoom)
}

/// On a press of painted key `hit` at `pos`: remember it if the key can
/// become a caret gesture on this keyboard.
pub(super) fn arm(s: &mut KeyboardState, hit: Option<usize>, pos: Point, family: LayoutFamily) {
    s.cursor = None;
    let pos = on_screen(pos);
    let Some(index) = hit else {
        return;
    };
    let Some(action) = s.last_painted_keys.get(index).map(|k| k.action) else {
        return;
    };
    let kind = match (action, family) {
        (KeyAction::Space, LayoutFamily::Gboard) => GestureKind::SpaceSlide,
        (KeyAction::Space, LayoutFamily::Ios) => GestureKind::Trackpad,
        (KeyAction::Backspace, LayoutFamily::Gboard) => GestureKind::DeleteSelect,
        _ => return,
    };
    if kind == GestureKind::Trackpad {
        crate::animation::request_draw_after(TRACKPAD_HOLD);
    }
    s.cursor = Some(CursorGesture {
        kind,
        key_index: index,
        origin: pos,
        last: pos,
        anchor: pos,
        pressed_at: crate::clock::now(),
        engaged_at: None,
        want: 0,
        selected: 0,
        capped: false,
    });
}

/// The finger moved to `pos`. `true` when a caret gesture owns the finger
/// (the move is the gesture's, not the keys'); the caret operations it
/// produced are on the caret queue.
pub(super) fn track(pos: Point) -> bool {
    let pos = on_screen(pos);
    let (ops, engaged_now) = with_state_mut(|s| {
        let Some(mut g) = s.cursor else {
            return (Vec::new(), false);
        };
        g.last = pos;
        let mut engaged_now = false;
        if !g.engaged() {
            match decide(s, &g, pos) {
                Decision::Wait => {
                    s.cursor = Some(g);
                    return (Vec::new(), false);
                }
                Decision::Drop => {
                    s.cursor = None;
                    return (Vec::new(), false);
                }
                Decision::Engage => {
                    engage(s, &mut g);
                    engaged_now = true;
                }
            }
        }
        let mut ops = Vec::new();
        if engaged_now && g.kind == GestureKind::DeleteSelect {
            ops.push(CaretOp::Snapshot);
        }
        ops.extend(steps(&mut g, pos));
        s.cursor = Some(g);
        (ops, engaged_now)
    });
    let owned = engaged_now || with_state_ref(|s| s.cursor.is_some_and(|g| g.engaged()));
    if engaged_now {
        super::voice_session::end_keeping_tail();
    }
    // Each give-back (a Restore here) moved the caret once.
    let given_back = ops
        .iter()
        .filter(|op| matches!(op, CaretOp::Restore { .. }))
        .count();
    for op in ops {
        caret_ops::push(op);
    }
    notify_ticks(given_back as u32);
    if owned {
        crate::animation::request_draw();
    }
    owned
}

enum Decision {
    /// Still a key press; keep watching.
    Wait,
    /// A key press for good.
    Drop,
    /// The press is now the gesture.
    Engage,
}

fn decide(s: &KeyboardState, g: &CursorGesture, pos: Point) -> Decision {
    let (dx, dy) = (pos.x - g.origin.x, pos.y - g.origin.y);
    let vertical_first = dy.abs() >= ENGAGE_DISTANCE && dy.abs() > dx.abs();
    match g.kind {
        // The iPhone's trackpad starts from a still finger (`tick`); one
        // that moves first is typing.
        GestureKind::Trackpad => {
            if dx.hypot(dy) >= ENGAGE_DISTANCE {
                Decision::Drop
            } else {
                Decision::Wait
            }
        }
        GestureKind::SpaceSlide if vertical_first => Decision::Drop,
        GestureKind::SpaceSlide if dx.abs() >= ENGAGE_DISTANCE => Decision::Engage,
        GestureKind::DeleteSelect => {
            let repeating = s.key_repeat.is_some_and(|r| r.last_fired_at.is_some());
            if repeating || vertical_first {
                Decision::Drop
            } else if dx <= -DELETE_WORD_STEP {
                Decision::Engage
            } else {
                Decision::Wait
            }
        }
        GestureKind::SpaceSlide => Decision::Wait,
    }
}

/// The press becomes the gesture: the key stays lit (or, for the trackpad,
/// unlit) whatever the finger crosses, a long press or repeat stops, and
/// steps count from where the finger came down.
fn engage(s: &mut KeyboardState, g: &mut CursorGesture) {
    g.engaged_at = Some(crate::clock::now());
    g.anchor = match g.kind {
        GestureKind::Trackpad => g.last,
        _ => g.origin,
    };
    s.pressed_key_index = Some(g.key_index);
    s.hold = None;
    s.key_repeat = None;
    s.linger = None;
}

/// The caret operations for the finger reaching `pos`, moving each anchor
/// on by the whole steps taken.
fn steps(g: &mut CursorGesture, pos: Point) -> Vec<CaretOp> {
    let mut ops = Vec::new();
    let plain = Modifiers::default();
    match g.kind {
        GestureKind::SpaceSlide => {
            let n = take_steps(&mut g.anchor.x, pos.x, SPACE_STEP);
            push_n(
                &mut ops,
                n,
                [Key::ArrowRight, Key::ArrowLeft],
                plain,
                StepKind::Char,
            );
        }
        GestureKind::Trackpad => {
            let n = take_steps(&mut g.anchor.x, pos.x, TRACKPAD_CHAR_STEP);
            push_n(
                &mut ops,
                n,
                [Key::ArrowRight, Key::ArrowLeft],
                plain,
                StepKind::Char,
            );
            // Y-up: a finger moving up moves the caret up a line.
            let m = take_steps(&mut g.anchor.y, pos.y, TRACKPAD_LINE_STEP);
            push_n(
                &mut ops,
                m,
                [Key::ArrowUp, Key::ArrowDown],
                plain,
                StepKind::Line,
            );
        }
        GestureKind::DeleteSelect => {
            let n = take_steps(&mut g.anchor.x, pos.x, DELETE_WORD_STEP);
            if n < 0 && !g.capped {
                g.want += n.unsigned_abs() as u32;
            } else if n > 0 {
                g.want = g.want.saturating_sub(n as u32);
            }
            // Past the start of the slide the zero point follows the finger.
            if g.want == 0 && pos.x > g.anchor.x {
                g.anchor.x = pos.x;
            }
            select_words(g, &mut ops);
        }
    }
    ops
}

/// Bring the Backspace slide's selection to the words its finger asks for:
/// select more words leftward, or put the starting selection back and select
/// fewer from there, so taking words back returns exactly to where the
/// slide began.
fn select_words(g: &mut CursorGesture, ops: &mut Vec<CaretOp>) {
    if g.want < g.selected && caret_ops::snapshot().is_none() {
        // The widget can't put its selection back: give the words back with
        // the inverse key (which may leave the spaces after a word selected).
        let back = g.selected - g.want;
        push_give_back(ops, back);
        g.selected = g.want;
        g.capped = false;
        return;
    }
    // Giving words back moves the caret once, however many words are then
    // selected again from the start: one tick (`track` gives it), not one
    // per word.
    let kind = if g.want < g.selected {
        ops.push(CaretOp::Restore { give_back: 0 });
        g.selected = 0;
        g.capped = false;
        StepKind::ReselectWord
    } else {
        StepKind::SelectWord
    };
    let more = g.want.saturating_sub(g.selected);
    push_n(
        ops,
        -i64::from(more),
        [Key::ArrowRight, Key::ArrowLeft],
        word_select_modifiers(),
        kind,
    );
}

/// `count` inverse word steps (Shift + word modifier + Right).
fn push_give_back(ops: &mut Vec<CaretOp>, count: u32) {
    push_n(
        ops,
        i64::from(count),
        [Key::ArrowRight, Key::ArrowLeft],
        word_select_modifiers(),
        StepKind::GiveBackWord,
    );
}

/// The App ran a gesture step of `kind`: `moved` says whether it moved the
/// caret. A move gives a haptic tick; a word that couldn't be selected caps
/// the Backspace slide where it is.
pub(super) fn step_result(kind: StepKind, moved: bool) {
    let words = matches!(kind, StepKind::SelectWord | StepKind::ReselectWord);
    if words {
        with_state_mut(|s| {
            let Some(g) = s.cursor.as_mut() else {
                return;
            };
            if moved {
                g.selected += 1;
            } else {
                g.capped = true;
                g.want = g.selected;
            }
        });
    }
    if moved && kind != StepKind::ReselectWord {
        notify_ticks(1);
    }
}

/// Whole `step`s from `anchor` to `to` (signed), moving `anchor` by them.
fn take_steps(anchor: &mut f64, to: f64, step: f64) -> i64 {
    let n = ((to - *anchor) / step).trunc() as i64;
    *anchor += n as f64 * step;
    n
}

/// `|n|` steps of `kind`: `keys[0]` for a positive `n`, `keys[1]` for a
/// negative one.
fn push_n(ops: &mut Vec<CaretOp>, n: i64, keys: [Key; 2], mods: Modifiers, kind: StepKind) {
    let [pos, neg] = keys;
    let key = if n > 0 { pos } else { neg };
    for _ in 0..n.unsigned_abs() {
        ops.push(CaretOp::Step {
            key: key.clone(),
            mods,
            kind,
        });
    }
}

/// The finger lifted. `true` when a caret gesture had taken the press: it
/// ends here (the Backspace slide deletes its selection) and the key types
/// nothing. `false` leaves the lift to the key press.
pub(super) fn release() -> bool {
    let finished = with_state_mut(|s| {
        let g = s.cursor.take()?;
        if !g.engaged() {
            return None;
        }
        s.pressed_key_index = None;
        s.captured_pointer = false;
        s.key_repeat = None;
        s.hold = None;
        Some(g)
    });
    let Some(g) = finished else {
        return false;
    };
    if g.kind == GestureKind::DeleteSelect {
        caret_ops::push(if g.selected > 0 {
            CaretOp::DeleteSelection
        } else {
            CaretOp::Restore { give_back: 0 }
        });
    }
    caret_ops::push(CaretOp::EndGesture);
    crate::animation::request_draw();
    true
}

/// The press ends without its lift (the platform cancelled the touch, or a
/// second finger landed): the Backspace slide and the trackpad put the
/// selection back and delete nothing; a space-bar slide stays where it got.
///
/// A widget that can't put its selection back gets the Backspace slide's
/// words back with the inverse keys instead, and the trackpad's caret stays
/// where it got to.
pub(super) fn cancel(s: &mut KeyboardState) {
    let Some(g) = s.cursor.take() else {
        return;
    };
    if cancel_restores(g) {
        let give_back = if g.kind == GestureKind::DeleteSelect {
            g.selected
        } else {
            0
        };
        caret_ops::push(CaretOp::Restore { give_back });
    }
    if g.engaged() {
        caret_ops::push(CaretOp::EndGesture);
    }
}

/// Whether ending `g` without its lift puts the selection back.
fn cancel_restores(g: CursorGesture) -> bool {
    g.engaged() && matches!(g.kind, GestureKind::DeleteSelect | GestureKind::Trackpad)
}

/// Whether a second finger landing on the keyboard cancels the press under
/// the first (see [`cancel`]) rather than letting it finish as a lift.
pub(super) fn overlap_cancels(s: &KeyboardState) -> bool {
    s.cursor.is_some_and(cancel_restores)
}

/// Start the iPhone trackpad once the space bar has been held still for
/// [`TRACKPAD_HOLD`], and keep frames coming while the labels fade. Called
/// once per paint.
pub(super) fn tick() {
    let now = crate::clock::now();
    let (started, fading) = with_state_mut(|s| {
        let Some(mut g) = s.cursor else {
            return (false, false);
        };
        if !s.captured_pointer {
            s.cursor = None;
            return (false, false);
        }
        let mut started = false;
        if g.kind == GestureKind::Trackpad
            && !g.engaged()
            && now.duration_since(g.pressed_at) >= TRACKPAD_HOLD
        {
            engage(s, &mut g);
            caret_ops::push(CaretOp::Snapshot);
            started = true;
        }
        s.cursor = Some(g);
        let fading = g.kind == GestureKind::Trackpad
            && g.engaged_at
                .is_some_and(|t| now.duration_since(t) < LABEL_FADE);
        (started, fading)
    });
    if started {
        super::voice_session::end_keeping_tail();
        // iOS plays a tick as the trackpad starts.
        notify_ticks(1);
    }
    if started || fading {
        crate::animation::request_draw();
    }
}

/// When the keyboard next needs a frame for a gesture: the iPhone hold's end.
pub(super) fn due(s: &KeyboardState) -> Option<Instant> {
    let g = s.cursor?;
    (g.kind == GestureKind::Trackpad && !g.engaged()).then(|| g.pressed_at + TRACKPAD_HOLD)
}

/// Whether the iPhone trackpad is on.
pub(super) fn trackpad_active(s: &KeyboardState) -> bool {
    s.cursor
        .is_some_and(|g| g.kind == GestureKind::Trackpad && g.engaged())
}

/// How opaque the key labels draw: fading to nothing as the trackpad starts,
/// fully drawn otherwise.
pub(super) fn label_alpha(s: &KeyboardState) -> f64 {
    match s.cursor {
        Some(g) if g.kind == GestureKind::Trackpad => match g.engaged_at {
            Some(t) => {
                let faded = crate::clock::since(t).as_secs_f64() / LABEL_FADE.as_secs_f64();
                (1.0 - faded).clamp(0.0, 1.0)
            }
            None => 1.0,
        },
        _ => 1.0,
    }
}
