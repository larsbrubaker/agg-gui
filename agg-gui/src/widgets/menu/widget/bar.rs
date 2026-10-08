//! `MenuBar` — the visible strip of top-level menus.
//!
//! Holds a list of [`TopMenu`]s, lays their titles out horizontally (or
//! stacked for a sidebar), paints them, and opens the shared
//! [`PopupMenu`] under the title the user presses.  Each title also has an
//! invisible [`MenuTitle`] child (see `top_menu.rs`) so the tree exposes a
//! findable, named widget per title.  The popup machinery lives in
//! `widget/mod.rs` (`PopupMenu`) and `state.rs`; geometry constants in
//! `geometry.rs`.

use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, Modifiers, MouseButton};
use crate::font_settings;
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::{current_viewport, BackbufferCache, Widget};

use super::super::fit_width::MenuWidth;
use super::super::geometry::{contains, effective_metrics, item_at_path, DEFAULT_FONT_SIZE};
use super::super::paint::{bar_button_text_color, paint_menu_bar_button_bg, MenuStyle};
use super::super::state::{MenuAnchorKind, MenuResponse};
use super::labels::BarLabels;
use super::top_menu::{MenuTitle, TopMenu};
use super::{is_touch_synthesized, MenuOrientation, PopupMenu};

pub struct MenuBar {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    /// Explicit caller override from [`MenuBar::with_font_size`].  `None`
    /// means "auto": the size tracks [`effective_metrics`] so touch menus
    /// grow their text in lock-step with their rows.  An explicit override
    /// always wins (the caller knows best).
    font_size: Option<f64>,
    menus: Vec<TopMenu>,
    pub(super) open_index: Option<usize>,
    pub(super) hover_index: Option<usize>,
    pub(super) popup: PopupMenu,
    on_action: Box<dyn FnMut(&str)>,
    /// Top-menu index whose hover highlight is suppressed until cursor exit.
    pub(super) suppress_hover_for: Option<usize>,
    /// When `true`, [`Widget::layout`] returns the tight content width
    /// (sum of menu-button widths) instead of the full available width.
    /// Set via [`MenuBar::with_fit_width`] when the bar shares a FlexRow
    /// with right-aligned chrome (e.g. project title, About button) and
    /// shouldn't claim every spare pixel.
    fit_width: bool,
    orientation: MenuOrientation,
    /// CPU backbuffer cache for the mostly-static bar pixels.
    cache: BackbufferCache,
    /// Cached labels for each top-menu bar button.
    bar_labels: BarLabels,
    /// Explicit bar height from [`MenuBar::with_bar_height`]; `None` uses
    /// [`effective_metrics`]' `bar_h` (desktop [`super::super::MENU_BAR_H`]).
    bar_height: Option<f64>,
    /// `(id, label)` of each [`MenuTitle`] child, to rebuild them only when
    /// the titles change.
    title_keys: Vec<(String, String)>,
    /// Instance id from [`MenuBar::with_id`], reported by [`Widget::id`].
    id: Option<String>,
}

impl MenuBar {
    pub fn new(
        font: Arc<Font>,
        menus: Vec<TopMenu>,
        on_action: impl FnMut(&str) + 'static,
    ) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            font_size: None,
            menus,
            open_index: None,
            hover_index: None,
            popup: PopupMenu::new(Vec::new()),
            on_action: Box::new(on_action),
            suppress_hover_for: None,
            fit_width: false,
            orientation: MenuOrientation::Horizontal,
            cache: BackbufferCache::new(),
            bar_labels: BarLabels::new(),
            bar_height: None,
            title_keys: Vec::new(),
            id: None,
        }
    }

    /// Give this bar a widget id ([`Widget::id`]) so an app or a test can
    /// find it by name (e.g. "Sheet Menu Bar") with
    /// [`find_widget_by_id`](crate::widget::find_widget_by_id).  A bar has
    /// no id by default.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Height of a horizontal bar (logical px) — e.g. a compact 20-px bar
    /// in a dense editor.  The default is the menu metrics' bar height.
    /// On a touch device the bar never drops below the touch-grown
    /// height, so titles stay tappable.  Vertical bars ignore this.
    pub fn with_bar_height(mut self, height: f64) -> Self {
        self.set_bar_height(height);
        self
    }

    /// Runtime form of [`Self::with_bar_height`].
    pub fn set_bar_height(&mut self, height: f64) {
        self.bar_height = Some(height);
        self.cache.invalidate();
    }

    /// The horizontal bar height this frame: the explicit height when set
    /// (floored at the touch-grown metrics height on touch), otherwise the
    /// metrics' bar height.
    pub fn bar_height(&self) -> f64 {
        let m = effective_metrics();
        match self.bar_height {
            Some(h) if crate::input_profile::touch_ui_active() => h.max(m.bar_h),
            Some(h) => h,
            None => m.bar_h,
        }
    }

    /// Keep one [`MenuTitle`] child per menu, named and positioned to
    /// match.  Rebuilt only when a title's id or label changes.
    fn sync_title_children(&mut self) {
        let keys: Vec<(String, String)> = self
            .menus
            .iter()
            .map(|menu| (menu.title_id(), menu.label.clone()))
            .collect();
        if keys != self.title_keys || self.children.len() != keys.len() {
            self.children = keys
                .iter()
                .map(|(id, label)| {
                    Box::new(MenuTitle::new(id.clone(), label.clone())) as Box<dyn Widget>
                })
                .collect();
            self.title_keys = keys;
        }
        for (child, menu) in self.children.iter_mut().zip(&self.menus) {
            child.set_bounds(menu.rect);
        }
    }

    /// Refresh the per-button label cache to match `self.menus`.
    fn sync_bar_labels(&mut self) {
        let labels: Vec<&str> = self.menus.iter().map(|m| m.label.as_str()).collect();
        let font_size = self.effective_font_size();
        self.bar_labels
            .sync_to(&self.active_font(), font_size, &labels);
    }

    /// Use a vertical layout — the bar stacks its menu buttons top-to-
    /// bottom (Y-up: highest local Y first) and opens popups to the
    /// RIGHT of each button. Intended for narrow, tall chrome strips
    /// such as a left-side mobile sidebar.
    pub fn with_orientation(mut self, orientation: MenuOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Opt into tight-width sizing — `Widget::layout` will report the
    /// summed menu-button width rather than the full available width.
    /// Use when the MenuBar is hosted inside a `FlexRow` with sibling
    /// chrome on the right (project title, status indicators, etc.)
    /// that needs to share the same row.
    pub fn with_fit_width(mut self, fit: bool) -> Self {
        self.fit_width = fit;
        self
    }

    pub fn with_font_size(mut self, font_size: f64) -> Self {
        self.font_size = Some(font_size);
        self
    }

    /// Width policy for the bar's popups (default [`MenuWidth::Fixed`]);
    /// [`MenuWidth::FitContent`] sizes each popup to its widest row.
    /// Unrelated to [`Self::with_fit_width`], which sizes the bar itself.
    pub fn with_menu_width(mut self, width: MenuWidth) -> Self {
        self.popup.set_width(width);
        self
    }

    /// Floor for the bar's fitted popups; see [`PopupMenu::with_min_width`].
    pub fn with_menu_min_width(mut self, min_width: f64) -> Self {
        self.popup.set_min_width(min_width);
        self
    }

    /// Keep the popup's fit-width measuring font in step with the font it
    /// paints with, so hit-testing matches the painted panels.
    fn sync_popup_measure(&mut self) {
        let (font, size) = (self.active_font(), self.effective_font_size());
        self.popup.set_measure_font(font, size);
    }

    /// Resolve the text size the bar / its popup should use this frame.
    /// An explicit [`with_font_size`](Self::with_font_size) override wins;
    /// otherwise the size comes from [`effective_metrics`], which grows it
    /// on touch so the label stays proportional to the (grown) row.
    fn effective_font_size(&self) -> f64 {
        self.font_size
            .unwrap_or_else(|| effective_metrics().default_font_size)
    }

    /// Override the popup's [`MenuStyle`] (row geometry, panel chrome,
    /// width policy, shortcut format) — see [`PopupMenu::set_style`].
    /// Without it the bar's popups use the thread's
    /// [`super::super::current_menu_style`].  Call before
    /// [`Self::with_menu_width`] / [`Self::with_menu_min_width`] if both are
    /// used, since the style carries its own width policy.
    pub fn with_menu_style(mut self, style: MenuStyle) -> Self {
        self.popup.set_style(style);
        self.cache.invalidate();
        self
    }

    /// Replace the bar's top-level menu list at runtime.  Used by callers
    /// that derive menu contents from app state (e.g. radio-style theme
    /// pickers) and need to refresh the items each frame so the popup's
    /// check/radio marks reflect the canonical state.  Invalidates the
    /// backbuffer cache so the next paint re-rasters bar labels.
    pub fn set_menus(&mut self, menus: Vec<TopMenu>) {
        self.menus = menus;
        self.cache.invalidate();
    }

    /// Read-only access to the configured top-level menus.  Mainly for
    /// tests that need to inspect labels / items without going through
    /// the popup state machine.
    pub fn menus(&self) -> &[TopMenu] {
        &self.menus
    }

    /// Resolve the font used for layout/paint.  Prefers the system-wide
    /// font override so the System window's font picker propagates live;
    /// falls back to the per-instance font otherwise.  Mirrors the
    /// `Label::active_font` pattern.
    fn active_font(&self) -> Arc<Font> {
        font_settings::current_system_font().unwrap_or_else(|| Arc::clone(&self.font))
    }

    fn menu_at(&self, pos: Point) -> Option<usize> {
        self.menus.iter().position(|menu| contains(menu.rect, pos))
    }

    pub(super) fn open_menu(&mut self, idx: usize) {
        let rect = self.menus[idx].rect;
        self.menus[idx].refresh_items();
        self.popup.items = self.menus[idx].items.clone();
        // Horizontal: anchor at the BAR'S bottom-left (rect.x, rect.y)
        // — popup opens straight DOWN under the bar item, allowed to
        // extend off-bar via the `Bar` kind's negative-y clamp.
        //
        // Vertical: anchor at the BUTTON'S top-right corner — popup
        // opens to the RIGHT of the button with its top aligned to the
        // button's top. `Context` kind clamps the popup inside the
        // viewport so we don't trail off the top.
        let (anchor, kind) = match self.orientation {
            MenuOrientation::Horizontal => (Point::new(rect.x, rect.y), MenuAnchorKind::Bar),
            // Anchor at the bar item's TOP edge so the popup
            // rises FROM it instead of hanging below.
            MenuOrientation::HorizontalBottom => (
                Point::new(rect.x, rect.y + rect.height),
                MenuAnchorKind::BottomBar,
            ),
            MenuOrientation::Vertical => (
                Point::new(rect.x + rect.width, rect.y + rect.height),
                MenuAnchorKind::Context,
            ),
        };
        self.sync_popup_measure();
        self.popup.state.open_at(anchor, kind);
        self.open_index = Some(idx);
        self.hover_index = Some(idx);
        self.cache.invalidate();
        crate::animation::request_draw();
    }

    fn open_menu_for_drag_release(&mut self, idx: usize) {
        self.open_menu(idx);
        self.popup.state.arm_mouse_up_activation();
    }

    fn switch_open_menu(&mut self, delta: isize) -> EventResult {
        let Some(current) = self.open_index else {
            return EventResult::Ignored;
        };
        if self.menus.is_empty() {
            return EventResult::Ignored;
        }
        let len = self.menus.len() as isize;
        let next = (current as isize + delta).rem_euclid(len) as usize;
        self.open_menu(next);
        EventResult::Consumed
    }

    fn should_switch_top_menu(&self, key: &Key) -> bool {
        match key {
            Key::ArrowLeft => self.popup.state.open_path.is_empty(),
            Key::ArrowRight => {
                if !self.popup.state.open_path.is_empty() {
                    return false;
                }
                self.popup
                    .state
                    .hover_path
                    .as_deref()
                    .and_then(|path| item_at_path(&self.popup.items, path))
                    .map_or(true, |item| !item.has_submenu())
            }
            _ => false,
        }
    }

    fn set_hover_index(&mut self, hover: Option<usize>) {
        // Touch devices have no real cursor; the synth-MouseMove fired
        // alongside a touchstart would otherwise paint a hover panel that
        // sticks after the tap (no MouseMove ever leaves the bar to clear
        // it).  Coerce hover to `None` for any input within the touch-synth
        // window so a tap-to-open / tap-to-close cycle leaves no residue.
        let hover = if is_touch_synthesized() { None } else { hover };
        if self.hover_index != hover {
            self.hover_index = hover;
            // `request_draw()` (NOT `_without_invalidation`) — the bar's
            // hover paint lives inside the parent Window's retained
            // backbuffer, so the cache must invalidate or the next paint
            // composites a stale bitmap.  The epoch bump in `request_draw`
            // is what `dispatch_event` reads to mark the ancestor path
            // dirty even when this MouseMove returns `Ignored`.
            crate::animation::request_draw();
            // The bar itself is backbuffered too — invalidate so the
            // next paint re-rasterises the hover-tinted bar item.
            self.cache.invalidate();
        }
        // Cursor moved to a different top-menu (or off any) — clear
        // the post-close hover suppression so the next genuine hover
        // re-enters with the usual highlight.
        if self.suppress_hover_for != hover {
            self.suppress_hover_for = None;
            self.cache.invalidate();
        }
    }
}

impl Widget for MenuBar {
    crate::widgets::widget_as_any!();
    fn type_name(&self) -> &'static str {
        "MenuBar"
    }

    fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_bounds(&mut self, bounds: Rect) {
        if (bounds.width - self.bounds.width).abs() > 0.5
            || (bounds.height - self.bounds.height).abs() > 0.5
        {
            self.cache.invalidate();
        }
        self.bounds = bounds;
    }

    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn backbuffer_cache_mut(&mut self) -> Option<&mut BackbufferCache> {
        Some(&mut self.cache)
    }

    fn backbuffer_mode(&self) -> crate::widget::BackbufferMode {
        // Mirror Label: when the global LCD toggle is on (which the
        // default rule wires to "scale ≤ 1.25"), use the per-channel LCD
        // coverage cache so text on the bar is subpixel-rendered exactly
        // like Label text.  Falls back to RGBA at HiDPI where LCD gains
        // nothing.  MenuBar paints `top_bar_bg` as an opaque full-width
        // fill before any other content, satisfying LcdCoverage's
        // "widget must cover its bounds with opaque content" contract.
        if crate::font_settings::lcd_enabled() {
            crate::widget::BackbufferMode::LcdCoverage
        } else {
            crate::widget::BackbufferMode::Rgba
        }
    }

    fn layout(&mut self, available: Size) -> Size {
        // Keep the bar's Label cache in lock-step with `self.menus`
        // before measuring — handles dynamic menu lists.
        self.sync_bar_labels();
        let m = effective_metrics();
        match self.orientation {
            MenuOrientation::Horizontal | MenuOrientation::HorizontalBottom => {
                // Scale each button's width by the same factor the text
                // grew, so the label keeps the same slack on touch as on
                // desktop and the button hit-rect always contains its text.
                let wscale = self.effective_font_size() / DEFAULT_FONT_SIZE;
                let bar_h = self.bar_height();
                let mut x = 0.0;
                for menu in &mut self.menus {
                    let width = (menu.label.chars().count() as f64 * 8.0 + 22.0).max(52.0) * wscale;
                    menu.rect = Rect::new(x, 0.0, width, bar_h);
                    x += width;
                }
                self.sync_title_children();
                // Fit-width mode leaves room for sibling chrome.
                let report_w = if self.fit_width { x } else { available.width };
                Size::new(report_w, bar_h)
            }
            MenuOrientation::Vertical => {
                // Stack top-to-bottom in Y-up coordinates.
                let mut y = available.height;
                for menu in &mut self.menus {
                    y -= m.vertical_row_h;
                    menu.rect = Rect::new(0.0, y, available.width, m.vertical_row_h);
                }
                self.sync_title_children();
                let used_h = self.menus.len() as f64 * m.vertical_row_h;
                Size::new(available.width, used_h)
            }
        }
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        // Re-sync in case paint runs without a preceding layout.
        self.sync_bar_labels();
        ctx.set_font(self.active_font());
        ctx.set_font_size(self.effective_font_size());
        let v = ctx.visuals();
        ctx.set_fill_color(v.top_bar_bg);
        ctx.begin_path();
        // Fill the laid-out bounds, never a height re-read from
        // `effective_metrics()`: the touch latch / input profile can flip
        // between layout and paint, and a re-read would fill a strip of the
        // wrong height for a frame.  Covering the bounds is also what the
        // `LcdCoverage` backbuffer mode requires.
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
        // First pass: button chrome under the text.
        for (idx, menu) in self.menus.iter().enumerate() {
            // After a click-to-close-toggle, the cursor is still over
            // the bar item so `hover_index` still points at it —
            // suppress the hover highlight until the cursor moves off
            // and back on, so the closed menu doesn't read as "still
            // selected".
            let hovered = self.hover_index == Some(idx) && self.suppress_hover_for != Some(idx);
            let open = self.open_index == Some(idx);
            paint_menu_bar_button_bg(ctx, menu.rect, open, hovered);
        }
        // Second pass: paint each button's `Label` through
        // `paint_subtree` so glyphs flow through Label's backbuffer +
        // LCD path.  Done after backgrounds so the text composites on
        // top of the hover fill.
        let menu_rects: Vec<(Rect, bool)> = self
            .menus
            .iter()
            .enumerate()
            .map(|(idx, menu)| (menu.rect, self.open_index == Some(idx)))
            .collect();
        for (idx, (rect, open)) in menu_rects.into_iter().enumerate() {
            let color = bar_button_text_color(ctx, open);
            self.bar_labels.paint_in(ctx, idx, rect, color);
        }
    }

    fn hit_test_global_overlay(&self, _local_pos: Point) -> bool {
        self.popup.is_open()
    }

    fn has_active_modal(&self) -> bool {
        self.popup.is_open()
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Event::MouseMove { pos } = event {
            let hovered = self.menu_at(*pos);
            self.set_hover_index(hovered);
            // Hover-switch a different open menu while a popup is open.
            // Suppressed when this MouseMove was synthesised by the
            // touch shell from a touchstart — on mobile the synth move
            // arrives at the tap position immediately followed by a
            // synth MouseDown at the same point; switching the open
            // menu here would make that MouseDown look like a click on
            // the currently-open menu and toggle-close the popup the
            // user just tapped to open.  On desktop the
            // `last_touch_event_age` is `None` (or very large), so
            // hover-switch works as before.
            let from_touch = is_touch_synthesized();
            if self.popup.is_open() && !from_touch {
                if let Some(idx) = hovered {
                    if self.open_index != Some(idx) {
                        let activate_on_release = self.popup.state.is_mouse_up_activation_armed();
                        self.open_menu(idx);
                        if activate_on_release {
                            self.popup.state.arm_mouse_up_activation();
                        }
                    }
                    return EventResult::Consumed;
                }
            }
        }
        if self.popup.is_open() {
            self.sync_popup_measure();
            if let Event::KeyDown { key, .. } = event {
                if self.should_switch_top_menu(key) {
                    return match key {
                        Key::ArrowLeft => self.switch_open_menu(-1),
                        Key::ArrowRight => self.switch_open_menu(1),
                        _ => EventResult::Ignored,
                    };
                }
            }
            // Tap-to-switch: when one menu is already open and a
            // MouseDown lands on a DIFFERENT top menu's bar, switch
            // directly.  Without this, the popup handler would see the
            // MouseDown as outside-the-popup-body and close the menu,
            // leaving the user staring at an empty bar.  Clicking the
            // currently-open menu falls through to the popup so it can
            // close (toggle, the desktop convention).
            if let Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } = event
            {
                if let Some(idx) = self.menu_at(*pos) {
                    if self.open_index != Some(idx) {
                        self.open_menu(idx);
                        return EventResult::Consumed;
                    }
                }
            }
            // Drag-release in neutral space cancels.  The user pressed
            // a top menu, dragged off both the bar and the popup body,
            // and let go — the standard menu convention is to dismiss.
            // The popup state's drag-release handler treats outside-
            // popup-body as a no-op (so a mouse-up still on the bar
            // doesn't close), so the bar enforces the cancel here
            // since only the bar knows where its own top-menu rects
            // live.
            if let Event::MouseUp {
                pos,
                button: MouseButton::Left,
                ..
            } = event
            {
                if self.popup.state.is_mouse_up_activation_armed()
                    && self.menu_at(*pos).is_none()
                    && !self.popup.body_contains(*pos, current_viewport())
                {
                    self.popup.close();
                    self.open_index = None;
                    self.cache.invalidate();
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
            }
            let (result, response) = self.popup.handle_event(event, current_viewport());
            if let MenuResponse::Action(action) = response {
                if let Some(idx) = self.open_index {
                    self.menus[idx].items = self.popup.items.clone();
                }
                (self.on_action)(&action);
                if !self.popup.is_open() {
                    self.open_index = None;
                    self.cache.invalidate();
                }
            } else if matches!(response, MenuResponse::Closed) {
                self.open_index = None;
                // Suppress the hover highlight on the menu the cursor
                // is still over — without this, click-to-close-toggle
                // leaves the bar item painted in the hover tint and
                // reads as "still selected".  Cleared once the cursor
                // moves to a different top-menu (or off the bar).
                self.suppress_hover_for = self.hover_index;
                self.cache.invalidate();
            }
            if result.is_consumed() {
                return result;
            }
        }
        match event {
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(idx) = self.menu_at(*pos) {
                    self.open_menu_for_drag_release(idx);
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseMove { .. } => EventResult::Ignored,
            _ => EventResult::Ignored,
        }
    }

    fn on_unconsumed_key(&mut self, key: &Key, modifiers: Modifiers) -> EventResult {
        let response = if self.popup.is_open() {
            self.popup.handle_shortcut(key, modifiers)
        } else {
            self.menus
                .iter_mut()
                .find_map(|menu| {
                    menu.refresh_items();
                    let mut popup = PopupMenu::new(menu.items.clone());
                    match popup.handle_shortcut(key, modifiers) {
                        MenuResponse::Action(action) => {
                            menu.items = popup.items;
                            Some(action)
                        }
                        MenuResponse::None | MenuResponse::Closed => None,
                    }
                })
                .map(MenuResponse::Action)
                .unwrap_or(MenuResponse::None)
        };
        if let MenuResponse::Action(action) = response {
            if let Some(idx) = self.open_index {
                self.menus[idx].items = self.popup.items.clone();
            }
            (self.on_action)(&action);
            if !self.popup.is_open() {
                self.open_index = None;
                self.cache.invalidate();
            }
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        let font = self.active_font();
        let font_size = self.effective_font_size();
        self.popup.paint(ctx, font, font_size, current_viewport());
    }
}
