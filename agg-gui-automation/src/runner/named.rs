//! The runner's name lookup and the waits that poll it — the port of
//! `GetWidgetsByName`, `GetWidgetByName`, `GetRegionByName`, `NameExists`,
//! `NamedWidgetExists`, `ChildExists<T>`, `WaitForName`,
//! `WaitForWidgetDisappear`, `WaitForWidgetEnabled` and
//! `WidgetNotFoundMessage` (agg-sharp `GuiAutomation/AutomationRunner.cs`).
//!
//! A name is agg-gui's [`Widget::id`] (C# `Name`) or the name of a
//! non-widget target a widget reports through
//! [`Widget::find_named_targets`] (a drop-down or menu row: C#'s
//! `NamedObject`, found with `GetObjectByName`); the walk and the
//! rectangles are [`crate::tree_query`]'s; among same-named widgets the
//! choice is [`crate::pointer_reach`]'s. Search regions are Y-down window
//! pixels ([`SearchRegion`]); a widget is in one when its screen rectangle
//! overlaps it, a target when its own rectangle does. A target counts as on
//! show when its widget is actually visible on screen (as C# asks of the
//! widget behind a `NamedObject`) and its rectangle reaches into the window:
//! drop-down rows paint in the overlay pass, outside their ancestors' clips. The polling waits look once per pumped frame (C#'s
//! `WaitForPendingUiWork(50)`), and their answer is the last look, not the
//! clock, so a zero-second wait reports what is there now. The frame-level
//! waits they build on are in the sibling `waits.rs`.
//!
//! C# searches every open `SystemWindow`; a runner drives one window, so its
//! tree is the whole search.

use agg_gui::{clock, Rect, Widget};

use super::AutomationRunner;
use crate::driver::UiDriver;
use crate::pointer_reach;
use crate::search_region::{ScreenRectangle, SearchRegion};
use crate::tree_query::{self, NamedHit, WidgetHandle};

/// C# `DefaultWidgetWaitSeconds`: how long the name lookups wait for a name
/// by default.
pub const DEFAULT_WIDGET_WAIT_SECONDS: f64 = 2.0;

/// C# `WidgetPollWaitMilliseconds`: the ceiling of the pump wait between
/// looks at the tree.
const WIDGET_POLL_WAIT_MILLISECONDS: i32 = 50;

/// How a name lookup waits and where it looks: C#'s optional
/// `secondsToWait`, `searchRegion` and `onlyVisible` parameters.
#[derive(Clone, Copy)]
pub struct WaitOpts<'a> {
    /// How long to wait for the name to show up first; zero (or less) looks
    /// once without waiting (C# skips its `WaitForName`).
    pub secs_to_wait: f64,
    /// Only widgets whose screen rectangle overlaps this region.
    pub search_region: Option<&'a SearchRegion>,
    /// Only widgets that are actually visible on screen.
    pub only_visible: bool,
}

impl Default for WaitOpts<'_> {
    fn default() -> Self {
        Self {
            secs_to_wait: DEFAULT_WIDGET_WAIT_SECONDS,
            search_region: None,
            only_visible: true,
        }
    }
}

impl<'a> WaitOpts<'a> {
    /// The defaults, waiting `secs_to_wait` seconds.
    pub fn secs(secs_to_wait: f64) -> Self {
        Self {
            secs_to_wait,
            ..Self::default()
        }
    }

    /// The defaults, limited to `region`.
    pub fn in_region(region: &'a SearchRegion) -> Self {
        Self {
            search_region: Some(region),
            ..Self::default()
        }
    }
}

/// A predicate over a found widget (C#'s `Func<GuiWidget, bool>`).
pub type WidgetPredicate<'p> = &'p dyn Fn(&dyn Widget) -> bool;

impl AutomationRunner {
    /// C# `WidgetNotFoundMessage`: the message a named-widget failure is
    /// reported with. The application's last startup failure is appended
    /// once `StartupFailureLog` lands (design slice 29).
    pub fn widget_not_found_message(operation: &str, widget_name: &str) -> String {
        format!("{operation} Failed: Named GuiWidget not found [{widget_name}]")
    }

    /// A window rectangle (logical, Y-up, as [`tree_query`] reports it) as
    /// a Y-down pixel [`ScreenRectangle`] — C#'s `SystemWindowToScreen`,
    /// truncating each edge to whole pixels the way C# does.
    pub fn window_rect_to_screen(&self, rect: Rect) -> ScreenRectangle {
        let scale = agg_gui::ux_scale::effective_scale();
        let (_, height_px) = self.driver.size_px();
        let height_px = height_px as i32;
        ScreenRectangle::new(
            (rect.left() * scale) as i32,
            height_px - (rect.top() * scale) as i32,
            (rect.right() * scale) as i32,
            height_px - (rect.bottom() * scale) as i32,
        )
    }

    /// The handle's screen rectangle and whether it is actually visible on
    /// screen; `None` once it is detached.
    fn screen_and_visibility(&self, handle: &WidgetHandle) -> Option<(ScreenRectangle, bool)> {
        let viewport = self.driver.logical_size();
        let placement = tree_query::placement(self.driver.root(), handle, viewport)?;
        let clipped = placement.clipped_rect;
        let visible = placement.visible_chain && clipped.width > 0.0 && clipped.height > 0.0;
        Some((self.window_rect_to_screen(placement.screen_rect), visible))
    }

    /// A hit's screen rectangle (the target's for a target) and whether it
    /// is on show; `None` once its widget is detached.
    fn hit_screen_and_visibility(&self, hit: &NamedHit) -> Option<(ScreenRectangle, bool)> {
        if hit.target.is_none() {
            return self.screen_and_visibility(&hit.handle);
        }
        let (_, widget_visible) = self.screen_and_visibility(&hit.handle)?;
        let rect = self.hit_window_rect(hit)?;
        let viewport = self.driver.logical_size();
        let on_window =
            tree_query::intersect(Rect::new(0.0, 0.0, viewport.width, viewport.height), rect);
        let visible = widget_visible && on_window.width > 0.0 && on_window.height > 0.0;
        Some((self.window_rect_to_screen(rect), visible))
    }

    /// A hit's rectangle in window space (logical, Y-up): the target's for a
    /// target, else the widget's.
    fn hit_window_rect(&self, hit: &NamedHit) -> Option<Rect> {
        let root = self.driver.root();
        match &hit.target {
            Some(target) => tree_query::local_rect_to_screen(root, &hit.handle, target.rect_local),
            None => tree_query::screen_rect(root, &hit.handle),
        }
    }

    /// The area of a hit left on show: a widget's after its ancestors' clips
    /// and the viewport, a target's after the viewport.
    fn hit_visible_area(&self, hit: &NamedHit) -> f64 {
        let root = self.driver.root();
        let viewport = self.driver.logical_size();
        let clipped = match &hit.target {
            Some(_) => self.hit_window_rect(hit).map(|r| {
                tree_query::intersect(Rect::new(0.0, 0.0, viewport.width, viewport.height), r)
            }),
            None => tree_query::clipped_rect(root, &hit.handle, viewport),
        };
        clipped.map_or(0.0, |c| c.width * c.height)
    }

    fn in_region(screen: ScreenRectangle, region: Option<&SearchRegion>) -> bool {
        region.is_none_or(|r| ScreenRectangle::intersection(r.screen_rect, screen).is_some())
    }

    /// C# `GetWidgetsByName`: every widget named `widget_name` that passes
    /// `opts`, in paint order. Waits up to `opts.secs_to_wait` for the name
    /// first and finds nothing if it never shows (C# returns `null`; here
    /// the list is empty).
    pub fn get_widgets_by_name(&mut self, widget_name: &str, opts: &WaitOpts) -> Vec<NamedHit> {
        self.check_not_timed_out();
        if opts.secs_to_wait > 0.0
            && !self.wait_for_name_with(widget_name, opts.secs_to_wait, opts.only_visible, None)
        {
            return Vec::new();
        }

        let root = self.driver.root();
        let mut named_widgets_in_region = Vec::new();
        for hit in tree_query::find_named_hits(root, widget_name) {
            let Some((screen, visible)) = self.hit_screen_and_visibility(&hit) else {
                continue;
            };
            if (!opts.only_visible || visible) && Self::in_region(screen, opts.search_region) {
                named_widgets_in_region.push(hit);
            }
        }
        named_widgets_in_region
    }

    /// C# `GetWidgetByName` with its `offsetHint`: the widget named
    /// `widget_name` a click should go to. When several share the name it
    /// prefers one a press can reach ([`pointer_reach`]), then the one with
    /// the largest clipped visible area — most likely the interactive one.
    ///
    /// C# also flashes the found widget's bounds (`DebugShowBounds`);
    /// agg-gui has no bounds overlay, so nothing is flashed.
    pub fn get_widget_hit_by_name(
        &mut self,
        widget_name: &str,
        opts: &WaitOpts,
    ) -> Option<NamedHit> {
        let get_results = self.get_widgets_by_name(widget_name, opts);
        if get_results.is_empty() {
            return None;
        }
        let get_results = pointer_reach::prefer_reachable(self.driver.root(), get_results);
        let mut best = get_results[0].clone();
        if get_results.len() > 1 {
            let mut best_area = 0.0;
            for result in &get_results {
                let area = self.hit_visible_area(result);
                if area > best_area {
                    best_area = area;
                    best = result.clone();
                }
            }
        }
        Some(best)
    }

    /// C# `GetWidgetByName`: [`get_widget_hit_by_name`](Self::get_widget_hit_by_name)'s widget.
    pub fn get_widget_by_name(
        &mut self,
        widget_name: &str,
        opts: &WaitOpts,
    ) -> Option<WidgetHandle> {
        self.get_widget_hit_by_name(widget_name, opts)
            .map(|hit| hit.handle)
    }

    /// C# `GetRegionByName`: the screen region the named widget (or named
    /// target) covers, to limit later searches to it.
    pub fn get_region_by_name(
        &mut self,
        widget_name: &str,
        opts: &WaitOpts,
    ) -> Option<SearchRegion> {
        let named_widget = self.get_widget_hit_by_name(widget_name, opts)?;
        let child_bounds = self.hit_window_rect(&named_widget)?;
        Some(SearchRegion::new(self.window_rect_to_screen(child_bounds)))
    }

    /// C# `GetObjectByName`: the first thing named `widget_name` that passes
    /// `opts` — a widget, or a non-widget target with its object (C#'s
    /// `NamedObject`) in [`NamedHit::target`] — with where a click on it
    /// lands. Unlike [`get_widget_hit_by_name`](Self::get_widget_hit_by_name)
    /// it takes the first hit, as C# does.
    pub fn get_object_by_name(&mut self, widget_name: &str, opts: &WaitOpts) -> Option<NamedHit> {
        self.get_widgets_by_name(widget_name, opts)
            .into_iter()
            .next()
    }

    /// C# `NameExists`: [`wait_for_name_with`](Self::wait_for_name_with) by
    /// another name.
    pub fn name_exists(
        &mut self,
        widget_name: &str,
        secs_to_wait: f64,
        only_visible: bool,
    ) -> bool {
        self.wait_for_name_with(widget_name, secs_to_wait, only_visible, None)
    }

    /// C# `NamedWidgetExists`: one look, no waiting. With `only_visible`
    /// a widget counts when it overlaps `search_region` (if given), is
    /// actually visible on screen and passes `predicate` (if given); without
    /// it any widget of that name counts, wherever it is (as in C#, the
    /// region and predicate then go unasked).
    pub fn named_widget_exists(
        &self,
        widget_name: &str,
        search_region: Option<&SearchRegion>,
        only_visible: bool,
        predicate: Option<WidgetPredicate<'_>>,
    ) -> bool {
        self.check_not_timed_out();
        let root = self.driver.root();
        for found_child in tree_query::find_named_hits(root, widget_name) {
            if !only_visible {
                return true;
            }
            let Some((screen, visible)) = self.hit_screen_and_visibility(&found_child) else {
                continue;
            };
            if Self::in_region(screen, search_region)
                && visible
                && predicate.is_none_or(|p| found_child.handle.widget(root).is_some_and(p))
            {
                return true;
            }
        }
        false
    }

    /// C# `ChildExists<T>`: whether a direct child of the window's root is
    /// a `T`, overlaps `search_region` (if given) and is actually visible.
    pub fn child_exists<T: 'static>(&self, search_region: Option<&SearchRegion>) -> bool {
        self.check_not_timed_out();
        let root = self.driver.root();
        let Some(window) = WidgetHandle::new(root, &[]) else {
            return false;
        };
        tree_query::children_of_type::<T>(root, &window)
            .iter()
            .any(|found_child| {
                self.screen_and_visibility(found_child)
                    .is_some_and(|(screen, visible)| {
                        Self::in_region(screen, search_region) && visible
                    })
            })
    }

    /// C# `WaitForName` with its defaults: wait up to `secs_to_wait` for a
    /// visible widget named `widget_name`.
    pub fn wait_for_name(&mut self, widget_name: &str, secs_to_wait: f64) -> bool {
        self.wait_for_name_with(widget_name, secs_to_wait, true, None)
    }

    /// C# `WaitForName`: look for the widget; while it is missing and less
    /// than `secs_to_wait` of UI time has passed, let the UI run a frame and
    /// look again. The answer is the last look, not the clock: judged by
    /// elapsed time, a zero-second wait reported "not found" even with the
    /// widget on screen.
    pub fn wait_for_name_with(
        &mut self,
        widget_name: &str,
        secs_to_wait: f64,
        only_visible: bool,
        predicate: Option<WidgetPredicate<'_>>,
    ) -> bool {
        self.check_not_timed_out();
        let time_waited = clock::now();
        loop {
            let found = self.named_widget_exists(widget_name, None, only_visible, predicate);
            if found || clock::since(time_waited).as_secs_f64() >= secs_to_wait {
                return found;
            }
            // The widget tree only changes when the UI runs, so asking again
            // before it has run again can only give the same answer.
            self.wait_for_pending_ui_work(WIDGET_POLL_WAIT_MILLISECONDS);
        }
    }

    /// C# `WaitForWidgetDisappear`: wait up to `secs_to_wait` for no
    /// visible widget to be named `widget_name`. As in
    /// [`wait_for_name_with`](Self::wait_for_name_with), the answer is the
    /// last look rather than the clock.
    pub fn wait_for_widget_disappear(&mut self, widget_name: &str, secs_to_wait: f64) -> bool {
        self.check_not_timed_out();
        let time_waited = clock::now();
        loop {
            let still_there = self.named_widget_exists(widget_name, None, true, None);
            if !still_there || clock::since(time_waited).as_secs_f64() >= secs_to_wait {
                return !still_there;
            }
            // The widget can only go away as a result of UI work.
            self.wait_for_pending_ui_work(WIDGET_POLL_WAIT_MILLISECONDS);
        }
    }

    /// C# `WaitForWidgetEnabled`: wait up to `secs_to_wait` for the named
    /// widget to be visible and enabled (it and every ancestor,
    /// [`tree_query::actually_enabled`]). Panics with
    /// [`widget_not_found_message`](Self::widget_not_found_message) when no
    /// such widget turns up, and with C#'s "not visible and enabled" message
    /// when it never becomes both.
    pub fn wait_for_widget_enabled(&mut self, widget_name: &str, secs_to_wait: f64) -> &mut Self {
        // This can be called after a reload. Draw first in the hope that the
        // UI sorts itself out, so the lookup below does not pick up a widget
        // that is on its way out. (C# looks the widget up with its default
        // wait here only to find its window; the one window is known.)
        self.get_widget_by_name(widget_name, &WaitOpts::default());
        self.wait_for_draw();

        let time_waited = clock::now();
        while !self.named_widget_exists(widget_name, None, true, None)
            && clock::since(time_waited).as_secs_f64() < secs_to_wait
        {
            self.wait_for_pending_ui_work(WIDGET_POLL_WAIT_MILLISECONDS);
        }

        let Some(widget) = self.get_widget_by_name(widget_name, &WaitOpts::default()) else {
            panic!(
                "{}",
                Self::widget_not_found_message("WaitForWidgetEnabled", widget_name)
            );
        };

        // Decided by the widget's state, not the clock.
        let remaining = (secs_to_wait - clock::since(time_waited).as_secs_f64()).max(0.0);
        let visible_and_enabled = self.wait_until(
            |runner| {
                let root = runner.driver.root();
                let viewport = runner.driver.logical_size();
                tree_query::actually_visible_on_screen(root, &widget, viewport)
                    && tree_query::actually_enabled(root, &widget)
            },
            remaining,
            super::waits::DEFAULT_CHECK_INTERVAL_MILLISECONDS,
        );
        if !visible_and_enabled {
            panic!(
                "WaitForWidgetEnabled Failed: [{widget_name}] not visible and enabled after [{secs_to_wait}] seconds"
            );
        }
        self
    }
}
