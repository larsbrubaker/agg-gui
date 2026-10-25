//! Touch text selection for the text editors, the way Android and iOS do it:
//! selection handles, a floating edit toolbar, and the gestures that drive
//! them. Shared by [`TextField`](crate::widgets::TextField) and
//! [`TextArea`](crate::widgets::TextArea), which each embed a
//! [`TouchSelection`] and implement [`TouchSelectable`] over their own text
//! geometry and edit verbs.
//!
//! Only presses made by a finger
//! ([`last_press_was_touch`](crate::touch_emulation::last_press_was_touch))
//! come here; a mouse press hides whatever a finger left and takes the
//! desktop path unchanged. The gestures, as `App`'s touch emulation
//! ([`crate::touch_emulation`]) delivers them:
//!
//!   - **Long press** (a right press from a held finger): on text, selects the
//!     word under the finger (the character the finger is on, never the one
//!     after it; units in [`text_units`]), shows a handle at each end, and
//!     claims the
//!     finger ([`crate::touch_emulation::claim_long_press_drag`]) so sliding
//!     it extends the selection by whole words; the toolbar appears when the
//!     finger lifts. On empty space past the text it places the caret and
//!     shows the toolbar (Paste). On a handle it starts dragging the handle.
//!     In a password field it selects everything, and the toolbar never
//!     offers Cut or Copy there.
//!     The shell's long-press listener plays the haptic tick.
//!   - **Handle drag** (a touch drag that starts on a handle arrives as the
//!     middle press `App` offers ScrollViews; a handle takes it first): moves
//!     that end of the selection by character, keeping the finger's offset
//!     from the line, never past the other end. The toolbar hides while the
//!     handle moves and comes back on release. The host scrolls the moving
//!     end into view, which auto-scrolls a `TextArea` dragged past an edge;
//!     a finger held still past the edge keeps it scrolling, faster the
//!     further out it is ([`gestures::tick`]).
//!   - **Tap**: on the toolbar runs its button (on the release, so a swipe
//!     that starts on a button presses nothing); on the selection toggles the
//!     toolbar; on the caret handle toggles it too (Android's Paste popup);
//!     elsewhere the host places the caret as it always does, after which a
//!     collapsed caret gets Android's caret handle for [`CARET_HANDLE_TIME`]
//!     and a selection (a double tap) its handles and toolbar.
//!
//! Typing, a mouse press, losing focus and being disabled hide everything
//! (a mouse press on a leftover handle or toolbar is consumed, nothing
//! else). A drag that does not start on a handle stays the host's (or a
//! ScrollView's) as before.
//!
//! Handles and toolbar are drawn in the host's `paint_global_overlay`, in
//! its local coordinates, so they follow it when it scrolls or the
//! on-screen keyboard lifts the layout; the host's `hit_test_global_overlay`
//! routes presses on them back to it, even outside its bounds. A handle whose
//! caret lies outside the clip of the host's ancestors (a ScrollView) is
//! neither drawn nor hit. Placement is [`placement`], the gestures
//! [`gestures`], their looks [`handles`] and [`toolbar`]. iOS's loupe is not
//! drawn.

use std::cell::Cell;
use std::sync::Arc;
use std::time::Duration;

use web_time::Instant;

use crate::draw_ctx::DrawCtx;
use crate::geometry::{Point, Rect};
use crate::text::Font;
use crate::widgets::text_context_menu::TextMenuAction;

pub(crate) mod gestures;
pub(crate) mod handles;
pub(crate) mod placement;
pub(crate) mod text_units;
pub(crate) mod toolbar;

pub(crate) use gestures::{after_event, route, tick};
use handles::CaretLine;
pub(crate) use handles::HandleKind;
pub(crate) use placement::{hit_test, paint};

/// How long Android's caret handle stays after the tap that placed the
/// caret (its `DELAY_BEFORE_HANDLE_FADES_OUT`).
pub(crate) const CARET_HANDLE_TIME: Duration = Duration::from_millis(4000);

/// What a host text widget gives [`TouchSelection`]: its edit state, its
/// text geometry (local, Y-up) and its edit verbs.
pub(crate) trait TouchSelectable {
    fn touch_selection(&self) -> &TouchSelection;
    fn touch_selection_mut(&mut self) -> &mut TouchSelection;
    /// The selection as `(anchor, cursor)` byte offsets.
    fn ts_anchor_cursor(&self) -> (usize, usize);
    /// Select from `anchor` to `cursor`; the host scrolls the cursor into
    /// view, restarts the caret blink and repaints.
    fn ts_select(&mut self, anchor: usize, cursor: usize);
    fn ts_text(&self) -> String;
    /// The caret offset nearest `local`.
    fn ts_offset_at(&self, local: Point) -> usize;
    /// Whether `local` is over text rather than empty space past it.
    fn ts_on_text(&self, local: Point) -> bool;
    /// The caret line at `offset`.
    fn ts_caret_line(&self, offset: usize) -> CaretLine;
    /// Where text shows; an end whose caret is outside has no handle.
    fn ts_text_rect(&self) -> Rect;
    fn ts_editable(&self) -> bool;
    /// A password: a long press selects everything and nothing is copied.
    fn ts_secret(&self) -> bool {
        false
    }
    /// Whether the toolbar may show (the host's context menu is enabled).
    fn ts_toolbar_allowed(&self) -> bool;
    /// Run a toolbar button through the host's own clipboard verbs.
    fn ts_apply(&mut self, action: TextMenuAction);
    fn ts_font(&self) -> Arc<Font>;
    /// Scroll the text by `dy` (positive shows later lines); `false` when it
    /// could not move. Hosts that do not scroll vertically keep the default.
    fn ts_autoscroll(&mut self, _dy: f64) -> bool {
        false
    }
}

/// A drag the controller owns. `lift` is the keyboard lift when it began:
/// the finger's position is read against it, so the layout sliding under a
/// still finger (the keyboard opening) does not move the selection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Drag {
    /// The finger that long pressed a word slides: whole words from `pivot`.
    Words { pivot: (usize, usize), lift: f64 },
    /// A handle follows the finger, `grab` from it to the handle's line.
    Handle {
        kind: HandleKind,
        grab: (f64, f64),
        lift: f64,
    },
}

/// What became of a finger's left press.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum Press {
    #[default]
    None,
    /// Handled here (handle, selection): its release is eaten too.
    Swallowed,
    /// Landed on this toolbar button, which runs if the press is released
    /// on it; a press that turns into a drag is cancelled.
    Button(TextMenuAction),
    /// Passed to the host to place the caret: settle on its release.
    Passed,
}

/// Touch selection state a text widget embeds.
#[derive(Default)]
pub(crate) struct TouchSelection {
    /// Handles at both ends of the selection.
    pub(crate) selection_handles: bool,
    /// The caret handle, until this moment.
    pub(crate) caret_handle_until: Option<Instant>,
    pub(crate) toolbar: bool,
    /// Whether the clipboard held text when the toolbar was shown.
    pub(crate) paste_available: bool,
    pub(crate) drag: Option<Drag>,
    pub(crate) press: Press,
    /// Where the finger driving a handle drag last was (host-local), and
    /// when the held-finger auto-scroll last stepped.
    pub(crate) finger: Option<Point>,
    pub(crate) scrolled_at: Option<Instant>,
    /// The host's local origin in root logical space at its last paint.
    pub(crate) origin: Cell<Point>,
    /// The clip of the host's ancestors at its last paint, in host-local
    /// coordinates; `None` when nothing clips it.
    pub(crate) clip: Cell<Option<Rect>>,
}

impl TouchSelection {
    /// Hide handles and toolbar and drop any drag.
    pub(crate) fn hide(&mut self) {
        self.selection_handles = false;
        self.caret_handle_until = None;
        self.toolbar = false;
        self.drag = None;
        self.press = Press::None;
        self.finger = None;
        self.scrolled_at = None;
    }

    pub(crate) fn is_shown(&self) -> bool {
        self.selection_handles || self.caret_handle_until.is_some() || self.toolbar
    }

    #[cfg(test)]
    pub(crate) fn selection_handles_shown(&self) -> bool {
        self.selection_handles
    }

    pub(crate) fn caret_handle_shown(&self) -> bool {
        self.caret_handle_until
            .is_some_and(|until| crate::clock::now() < until)
    }

    /// Whether the toolbar is up (it hides while a drag is in progress).
    pub(crate) fn toolbar_shown(&self) -> bool {
        self.toolbar && self.drag.is_none()
    }

    /// When the caret handle goes, for the host's `next_draw_deadline`.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.caret_handle_until
    }

    /// The caret handle's time is up but it was last drawn: one more frame.
    pub(crate) fn needs_draw(&self) -> bool {
        self.caret_handle_until
            .is_some_and(|until| crate::clock::now() >= until)
    }

    /// Record where the host is (call from a paint inside the tree pass):
    /// its root origin, and its ancestors' clip in its own coordinates.
    /// Forgets an expired caret handle, which this frame no longer draws.
    pub(crate) fn note_paint(&mut self, ctx: &dyn DrawCtx) {
        let t = crate::widget::logical_root_transform(ctx);
        self.origin.set(Point::new(t.tx, t.ty));
        let clip = crate::widget::current_paint_clip().map(|c| {
            let root = ctx.root_transform();
            let mut corners = [(c.x, c.y), (c.x + c.width, c.y + c.height)];
            for (x, y) in &mut corners {
                root.inverse_transform(x, y);
            }
            let (x0, x1) = (
                corners[0].0.min(corners[1].0),
                corners[0].0.max(corners[1].0),
            );
            let (y0, y1) = (
                corners[0].1.min(corners[1].1),
                corners[0].1.max(corners[1].1),
            );
            Rect::new(x0, y0, x1 - x0, y1 - y0)
        });
        self.clip.set(clip);
        if self.needs_draw() {
            self.caret_handle_until = None;
        }
    }

    /// Show both handles, and the toolbar when `toolbar`.
    pub(crate) fn show_selection(&mut self, toolbar: bool) {
        self.selection_handles = true;
        self.caret_handle_until = None;
        self.set_toolbar(toolbar);
    }

    /// Show the caret, with Android's handle when `handle`.
    pub(crate) fn show_caret(&mut self, handle: bool, toolbar: bool) {
        self.selection_handles = false;
        self.caret_handle_until = handle.then(|| crate::clock::now() + CARET_HANDLE_TIME);
        self.set_toolbar(toolbar);
    }

    /// Show or hide the toolbar, noting whether Paste has anything to paste
    /// (read once here, not every frame).
    pub(crate) fn set_toolbar(&mut self, shown: bool) {
        if shown && !self.toolbar {
            // A browser reads the system clipboard only when Paste is
            // tapped, so there it is always offered.
            self.paste_available = crate::clipboard::system_paste::reads_asynchronously()
                || crate::clipboard::get_text().is_some_and(|t| !t.is_empty());
        }
        self.toolbar = shown;
    }
}

/// Test hooks: where things are, in host-local coordinates.
#[cfg(test)]
pub(crate) mod probe {
    use super::placement::{toolbar_layout, visible_handles};
    use super::*;
    use handles::HandleStyle;

    /// The middle of a visible handle's grab area.
    pub(crate) fn handle_center<W: TouchSelectable>(w: &W, kind: HandleKind) -> Option<Point> {
        let style = HandleStyle::current();
        visible_handles(w)
            .into_iter()
            .find(|(k, _)| *k == kind)
            .map(|(k, line)| {
                let r = handles::hit_rect(style, k, line);
                Point::new(r.x + r.width * 0.5, r.y + r.height * 0.5)
            })
    }

    /// The middle of a toolbar button, while the toolbar shows.
    pub(crate) fn button_center<W: TouchSelectable>(
        w: &W,
        action: TextMenuAction,
    ) -> Option<Point> {
        let layout = toolbar_layout(w)?;
        layout
            .buttons
            .iter()
            .find(|(a, _)| *a == action)
            .map(|(_, r)| Point::new(r.x + r.width * 0.5, r.y + r.height * 0.5))
    }

    /// The toolbar's buttons, while it shows.
    pub(crate) fn buttons<W: TouchSelectable>(w: &W) -> Vec<TextMenuAction> {
        toolbar_layout(w)
            .map(|l| l.buttons.iter().map(|(a, _)| *a).collect())
            .unwrap_or_default()
    }

    /// The toolbar's rect, while it shows.
    pub(crate) fn toolbar_rect<W: TouchSelectable>(w: &W) -> Option<Rect> {
        toolbar_layout(w).map(|l| l.rect)
    }
}
