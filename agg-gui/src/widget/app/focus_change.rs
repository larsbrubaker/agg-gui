//! Focus changes on [`App`]: moving focus between widgets, and keeping it on
//! the right widget when the app rebuilds the focused part of the tree.
//!
//! `App` addresses the focused widget by child-index path, anchored to the
//! widgets' identities so it follows reorders (`path_anchor.rs`). Apps also
//! replace the focused widget with a freshly built one at the same place (a
//! list rebuilt after a selection change) and rely on keys still reaching
//! whatever stands there. That replacement has never been sent
//! `FocusGained`: a text field would show no caret and ignore typing while
//! `App` routes keys to it, and the on-screen keyboard would describe the old
//! widget. So after each resolve of the tracked paths, focus is checked
//! against the widget that was last told it has focus:
//!
//! - still at the focus path: nothing to do;
//! - moved elsewhere in the tree (re-parented): focus follows it;
//! - gone, with a focusable widget now at the focus path: that widget takes
//!   focus and is sent `FocusGained`, and the keyboard layer is re-told (it
//!   stays up for a text field and lowers for anything else);
//! - gone, with nothing focusable at the focus path (including a path that
//!   indexes past the tree): focus clears, as a click on an unfocusable
//!   widget clears it, and the keyboard lowers.
//!
//! No `FocusLost` goes to the widget that was replaced: it is out of the
//! tree.
//!
//! "Was told" is decided by a [`FocusStamp`] the `App` writes into the
//! widget's `WidgetBase` with each `FocusGained`, because heap addresses get
//! reused by rebuilt widgets. A focused widget without a `WidgetBase` falls
//! back to its address: a replacement of the same kind that reuses the
//! address of such a widget is taken for the original (keys still reach it;
//! it gets no `FocusGained`).

use super::path_anchor::identity;
use super::tree_paths::{widget_at_path, widget_at_path_ref};
use crate::event::Event;
use crate::focus::FocusStamp;
use crate::geometry::Point;
use crate::widget::tree::dispatch_event;
use crate::widget::{App, Widget};

impl App {
    /// Set focus to `new_path`, sending `FocusLost` / `FocusGained` as needed.
    ///
    /// When the app replaced the focused widget during the event being
    /// dispatched (before the next resolve tells the replacement), the stored
    /// path names a widget that never had focus: focusing that same path
    /// again sends it `FocusGained`, and no `FocusLost` goes to it when focus
    /// moves on.
    pub(super) fn set_focus(&mut self, new_path: Option<Vec<usize>>) {
        let told = self.focused_widget_was_told();
        if self.focus == new_path && (new_path.is_none() || told) {
            return;
        }
        // A dictation into the field losing focus ends there, before it
        // hears `FocusLost` (which drops any composition it shows).
        self.end_voice_input_keeping_tail();
        if let Some(old) = self.focus.take() {
            if told {
                dispatch_event(&mut self.root, &old, &Event::FocusLost, Point::ORIGIN);
            }
        }
        self.enter_focus(new_path);
    }

    /// Store `new_path` as focus, stamp and tell the widget there, and tell
    /// the keyboard layer which widget (if any) now has focus.
    fn enter_focus(&mut self, new_path: Option<Vec<usize>>) {
        self.store_focus(new_path.clone());
        self.focus_stamp = None;
        if let Some(new) = new_path.as_deref() {
            let base = widget_at_path(&mut self.root, new).and_then(|w| w.widget_base_mut());
            if let Some(base) = base {
                let stamp = FocusStamp::fresh();
                self.focus_stamp = Some(stamp.duplicate());
                base.focus_stamp = stamp;
            }
            dispatch_event(&mut self.root, new, &Event::FocusGained, Point::ORIGIN);
        }
        super::super::keyboard_scroll::notify_focus_change(
            new_path.as_deref(),
            self.viewport_size.width,
            self.root.as_mut(),
        );
    }

    /// Whether the widget at the focus path is the one last sent
    /// `FocusGained`: it carries the stamp written then, or, for a widget
    /// without a `WidgetBase`, the path still names the same widgets.
    pub(super) fn focused_widget_was_told(&self) -> bool {
        let Some(path) = self.focus.as_deref() else {
            return false;
        };
        let Some(widget) = widget_at_path_ref(self.root.as_ref(), path) else {
            return false;
        };
        match &self.focus_stamp {
            Some(stamp) => widget
                .widget_base()
                .is_some_and(|b| b.focus_stamp == *stamp),
            // The told widget had no `WidgetBase`; one that has a base now is
            // a different widget.
            None => widget.widget_base().is_none() && self.focus_is_anchored(),
        }
    }

    /// Keep focus on the widget that holds it after the tracked paths were
    /// resolved (see the module docs). Call at the end of
    /// [`resolve_tracked_paths`](Self::resolve_tracked_paths).
    pub(super) fn follow_focused_widget(&mut self) {
        let Some(path) = self.focus.clone() else {
            return;
        };
        if self.focused_widget_was_told() {
            // An ancestor may have been swapped for one holding the same
            // widget: re-anchor so later resolves follow the new ancestors.
            if !self.focus_is_anchored() {
                self.store_focus(Some(path));
            }
            return;
        }
        if let Some(moved) = self.find_told_widget() {
            self.store_focus(Some(moved));
            return;
        }
        let focusable =
            widget_at_path_ref(self.root.as_ref(), &path).is_some_and(|w| w.is_focusable());
        self.enter_focus(focusable.then_some(path.clone()));
        // A dictation goes on into the same field rebuilt, once it has been
        // told it has focus; for any other widget (or none) it ends, its
        // words kept from the newcomer.
        self.follow_voice_into_replacement(&path, focusable);
    }

    /// The path to the widget last told it has focus, wherever it now is in
    /// the tree: found by its stamp, or by its address when it has no
    /// `WidgetBase`. `None` when it left the tree.
    fn find_told_widget(&self) -> Option<Vec<usize>> {
        let root = self.root.as_ref();
        match &self.focus_stamp {
            Some(stamp) => find_path(root, &|w| {
                w.widget_base().is_some_and(|b| b.focus_stamp == *stamp)
            }),
            None => {
                let leaf = *self.anchors_focus_leaf()?;
                find_path(root, &|w| w.widget_base().is_none() && identity(w) == leaf)
            }
        }
    }
}

/// The path to the first widget below `root` (depth first, in child order)
/// that `matches`. Only used when focus cannot be followed level by level,
/// so the full walk is rare.
fn find_path(root: &dyn Widget, matches: &dyn Fn(&dyn Widget) -> bool) -> Option<Vec<usize>> {
    fn walk(
        node: &dyn Widget,
        matches: &dyn Fn(&dyn Widget) -> bool,
        path: &mut Vec<usize>,
    ) -> bool {
        for (i, child) in node.children().iter().enumerate() {
            path.push(i);
            if matches(child.as_ref()) || walk(child.as_ref(), matches, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut path = Vec::new();
    walk(root, matches, &mut path).then_some(path)
}
