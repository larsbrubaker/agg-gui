//! The list's selection gestures beyond a single row click: the press and release that make a
//! click (`press`, `release_press`), Shift ranges from an anchor, the rubber band drawn over empty
//! space, and the keyboard's select all, clear and arrow moves. A plain press on one of several
//! selected items keeps them all, so dragging carries the whole selection; only a release without
//! a drag collapses the selection to that item. A Control (Command) press on a selected item takes
//! it out on release, for the same reason. The band is kept in the list content's own
//! coordinates, so it stays on the items it started over while the list scrolls under it. Part of
//! `state/mod.rs`, whose fields it reads; the list widget decides where the band is drawn and
//! which rows it covers.
use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::focus::FocusId;
use agg_gui::{Modifiers, Point, Rect};

use super::{BrowserEntry, BrowserState, ItemEvent};

/// How far a press on empty space must move before it draws a band, so a click that wobbles a
/// little still clears the selection rather than drawing a tiny band.
const BAND_THRESHOLD: f64 = 4.0;

/// What a press on an already selected item leaves for its release to do.
enum OnRelease {
    /// A plain press on one of several selected items: a release without a drag selects only it.
    SelectOnly(BrowserEntry),
    /// A Control (Command) press on a selected item: a release without a drag takes it out.
    Deselect(BrowserEntry),
}

/// A rubber band over the list, in the list content's coordinates (Y up).
struct Band {
    start: Point,
    end: Point,
    /// The selection the band adds to (empty unless Control or Command was held at the press).
    base: Vec<BrowserEntry>,
    additive: bool,
    /// The press has moved far enough to draw the band.
    drawn: bool,
}

/// The list's selection gesture state, kept in the shared state so it survives the list being
/// rebuilt under the pointer.
pub struct SelectionGesture {
    /// Where a Shift range starts: the item of the last plain or Control press.
    anchor: Option<BrowserEntry>,
    /// The item the arrow keys move from: the last one pressed or moved to.
    lead: Option<BrowserEntry>,
    on_release: Option<OnRelease>,
    band: Option<Band>,
    /// The list's keyboard focus id: a press in the list asks for it.
    focus_id: FocusId,
    /// The folder tree's: a press on a row asks for it. Kept here, with the
    /// browser, so it stays the same when the tab builds its tree again.
    tree_focus_id: FocusId,
}

impl Default for SelectionGesture {
    fn default() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        // Each browser its own id (a main browser, a popup and the dialogs' lists);
        // far from the ids other widgets count from.
        static NEXT: AtomicU64 = AtomicU64::new(0x11B5_0000_0000);
        static NEXT_TREE: AtomicU64 = AtomicU64::new(0x11B6_0000_0000);
        Self {
            anchor: None,
            lead: None,
            on_release: None,
            band: None,
            focus_id: NEXT.fetch_add(1, Ordering::Relaxed),
            tree_focus_id: NEXT_TREE.fetch_add(1, Ordering::Relaxed),
        }
    }
}

/// `a` and `b` spanned as a rectangle.
fn span(a: Point, b: Point) -> Rect {
    Rect::new(
        a.x.min(b.x),
        a.y.min(b.y),
        (a.x - b.x).abs(),
        (a.y - b.y).abs(),
    )
}

impl BrowserState {
    /// The list's keyboard focus id.
    pub fn list_focus_id(&self) -> FocusId {
        self.gesture.focus_id
    }

    /// The folder tree's keyboard focus id.
    pub fn tree_focus_id(&self) -> FocusId {
        self.gesture.tree_focus_id
    }

    /// The rows the list shows, in their order (what a range or select-all runs over).
    fn listing(&self) -> Vec<BrowserEntry> {
        self.listed.clone().unwrap_or_else(|| self.entries())
    }

    fn position_in(listing: &[BrowserEntry], entry: &BrowserEntry) -> Option<usize> {
        listing.iter().position(|e| e.same_row(entry))
    }

    /// The rows from the anchor (or `index` itself without one) to `index`, in listing order.
    fn range_to(&self, index: usize) -> Vec<BrowserEntry> {
        let listing = self.listing();
        if listing.is_empty() {
            return Vec::new();
        }
        let index = index.min(listing.len() - 1);
        let anchor = self
            .gesture
            .anchor
            .as_ref()
            .and_then(|a| Self::position_in(&listing, a))
            .unwrap_or(index);
        listing[anchor.min(index)..=anchor.max(index)].to_vec()
    }

    /// A left press on row `index`. Shift selects the range from the anchor to it (Control or
    /// Command with Shift adds the range to the selection). Control (Command on a Mac) adds a row
    /// that is not selected, and leaves taking a selected one out to the release. A plain press
    /// selects only the row, unless it is one of several selected rows: they stay selected so a
    /// drag carries them all, and the release decides. A press that is the second of a double
    /// click (agg-gui's `is_double_click`, with any keys held) also activates the row, its hooks
    /// running once `state`'s borrow has ended. True when it did: the double click has done its
    /// job, so its press ends there.
    pub fn press(state: &Rc<RefCell<Self>>, index: usize, modifiers: Modifiers) -> bool {
        let double = state.borrow_mut().press_in(index, modifiers);
        Self::run_hooks(state);
        double
    }

    /// `press`'s queueing half.
    pub(super) fn press_in(&mut self, index: usize, modifiers: Modifiers) -> bool {
        self.begin_tap(None);
        let Some(entry) = self.listed_entry(index) else {
            return false;
        };
        let toggle = modifiers.ctrl || modifiers.meta;
        // A list that activates on a tap, and a finger's tap on a folder that opens it, act on
        // the release (`state/activate.rs`), so the second press of a double tap does not
        // activate the row as well.
        let plain = !(toggle || modifiers.shift || modifiers.alt);
        self.begin_tap(plain.then_some(index));
        let double = agg_gui::event::is_double_click()
            && !self.tap_activates()
            && self.tap_pending.is_none();
        self.gesture.on_release = None;
        let is_selected = self.selected.iter().any(|s| s.same_row(&entry));
        if modifiers.shift {
            let range = self.range_to(index);
            if toggle {
                for row in range {
                    if !self.selected.iter().any(|s| s.same_row(&row)) {
                        self.selected.push(row);
                    }
                }
            } else {
                self.selected = range;
            }
        } else {
            if toggle && is_selected {
                self.gesture.on_release = Some(OnRelease::Deselect(entry.clone()));
            } else if toggle {
                self.selected.push(entry.clone());
            } else if is_selected && self.selected.len() > 1 {
                self.gesture.on_release = Some(OnRelease::SelectOnly(entry.clone()));
            } else {
                self.select_only(entry.clone());
            }
            self.gesture.anchor = Some(entry.clone());
        }
        self.gesture.lead = Some(entry);
        self.changed();
        if double {
            // The double click's second press acts on the row; a pending collapse onto a row a
            // folder's opening has replaced would select a row no longer shown. A Control double
            // click still takes the row its first press added back out.
            if matches!(self.gesture.on_release, Some(OnRelease::SelectOnly(_))) {
                self.gesture.on_release = None;
            }
            self.activate_in(index, crate::actions::Activation::DoubleClick);
        }
        double
    }

    /// Only `entry` selected, and a part's click told to a listening dialog (a folder's click,
    /// the first half of the double click that opens it, is not the dialog's to answer).
    fn select_only(&mut self, entry: BrowserEntry) {
        self.selected = vec![entry.clone()];
        if let (Some(events), false) = (self.item_events.as_mut(), entry.is_container()) {
            events.push(ItemEvent::Clicked(entry));
        }
    }

    /// The left press on a row ended; `dragged` when it became a drag, which keeps the selection
    /// it carried.
    pub fn release_press(&mut self, dragged: bool) {
        let Some(action) = self.gesture.on_release.take() else {
            return;
        };
        if dragged {
            return;
        }
        match action {
            OnRelease::SelectOnly(entry) => self.select_only(entry),
            OnRelease::Deselect(entry) => self.selected.retain(|s| !s.same_row(&entry)),
        }
        self.changed();
    }

    /// Selects every row the list shows (Control+A, Command+A on a Mac).
    pub fn select_all(&mut self) {
        self.selected = self.listing();
        self.changed();
    }

    /// Empties the selection (Escape); false when it was already empty.
    pub fn clear_selection(&mut self) -> bool {
        self.gesture.on_release = None;
        if self.selected.is_empty() {
            return false;
        }
        self.selected.clear();
        self.changed();
        true
    }

    /// The listing index the arrow keys move from: the last row pressed or moved to while it is
    /// still selected, else the last selected row.
    pub fn lead_index(&self) -> Option<usize> {
        let listing = self.listing();
        let lead = self
            .gesture
            .lead
            .as_ref()
            .filter(|lead| self.selected.iter().any(|s| s.same_row(lead)));
        lead.or(self.selected.last())
            .and_then(|entry| Self::position_in(&listing, entry))
    }

    /// An arrow key moved the selection to row `index`; `extend` (Shift) selects the range from
    /// the anchor to it instead of only it.
    pub fn move_selection_to(&mut self, index: usize, extend: bool) {
        let Some(entry) = self.listed_entry(index) else {
            return;
        };
        if extend {
            self.selected = self.range_to(index);
        } else {
            self.selected = vec![entry.clone()];
            self.gesture.anchor = Some(entry.clone());
        }
        self.gesture.lead = Some(entry);
        self.changed();
    }

    /// A press on the list's empty space at `at` (content coordinates) starts a band;
    /// `additive` (Control or Command held) keeps the selection the band then adds to.
    pub fn begin_band(&mut self, at: Point, additive: bool) {
        self.gesture.on_release = None;
        self.gesture.band = Some(Band {
            start: at,
            end: at,
            base: if additive {
                self.selected.clone()
            } else {
                Vec::new()
            },
            additive,
            drawn: false,
        });
    }

    /// Whether a press on empty space is being held (a band, drawn or not yet).
    pub fn is_banding(&self) -> bool {
        self.gesture.band.is_some()
    }

    /// The band's rectangle in content coordinates once it is drawn.
    pub fn band_rect(&self) -> Option<Rect> {
        let band = self.gesture.band.as_ref().filter(|b| b.drawn)?;
        Some(span(band.start, band.end))
    }

    /// The pointer moved to `to` (content coordinates) while the band is held: the band's
    /// rectangle once it has moved far enough to draw, which the caller turns into the rows it
    /// covers (`select_band`).
    pub fn drag_band(&mut self, to: Point) -> Option<Rect> {
        let band = self.gesture.band.as_mut()?;
        band.end = to;
        let (dx, dy) = (to.x - band.start.x, to.y - band.start.y);
        if !band.drawn && (dx * dx + dy * dy).sqrt() <= BAND_THRESHOLD {
            return None;
        }
        band.drawn = true;
        agg_gui::animation::request_draw();
        Some(span(band.start, band.end))
    }

    /// The band covers the rows at `indices` (listing indices, in listing order): the selection
    /// becomes the band's base and those rows. Nothing is rebuilt while the set is unchanged.
    pub fn select_band(&mut self, indices: &[usize]) {
        let Some(band) = self.gesture.band.as_ref() else {
            return;
        };
        let mut selection = band.base.clone();
        for entry in indices.iter().filter_map(|&i| self.listed_entry(i)) {
            if !selection.iter().any(|s| s.same_row(&entry)) {
                selection.push(entry);
            }
        }
        let same = selection.len() == self.selected.len()
            && selection
                .iter()
                .zip(&self.selected)
                .all(|(a, b)| a.same_row(b));
        if !same {
            self.selected = selection;
            self.changed();
        }
    }

    /// The press on empty space ended. A band that was drawn keeps what it selected (true); a
    /// click that never moved far enough empties the selection when `clear_on_click` and no
    /// Control or Command was held (false).
    pub fn end_band(&mut self, clear_on_click: bool) -> bool {
        let Some(band) = self.gesture.band.take() else {
            return false;
        };
        if band.drawn {
            agg_gui::animation::request_draw();
            return true;
        }
        if clear_on_click && !band.additive {
            self.clear_selection();
        }
        false
    }
}
