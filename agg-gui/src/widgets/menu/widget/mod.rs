//! Widget adapters for reusable menus.
//!
//! `ContextMenu` is a small controller that other widgets can embed, while
//! `MenuBar` (`bar.rs`) is a visible widget for top-level menus, built
//! from `TopMenu`s (`top_menu.rs`).

use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, Modifiers};
use crate::geometry::{Point, Size};
use crate::text::Font;

use super::fit_width::{FitMeasure, MenuWidth};
use super::model::MenuEntry;
use super::paint::{paint_panel, MenuStyle};
use super::row_widgets::RowWidgets;
use super::state::{MenuAnchorKind, MenuResponse, PopupMenuState};
use super::style::current_menu_style;

use labels::PopupLabels;

mod bar;
mod labels;
mod popup_local;
mod popup_paint;
mod top_menu;

pub use bar::MenuBar;
pub use top_menu::{MenuTitle, TopMenu};

/// Layout direction for `MenuBar`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuOrientation {
    Horizontal,
    HorizontalBottom,
    Vertical,
}

/// Mouse events synthesised from a touch tap arrive within a few
/// milliseconds of the corresponding `touchstart`/`touchend`.  Allow a
/// generous window (50 ms) so a busy frame doesn't accidentally
/// classify a synthesised event as a desktop click.
const TOUCH_SYNTH_WINDOW_MS: u128 = 50;

fn is_touch_synthesized() -> bool {
    crate::touch_state::last_touch_event_age()
        .map(|d| d.as_millis() < TOUCH_SYNTH_WINDOW_MS)
        .unwrap_or(false)
}

#[derive(Clone)]
pub struct PopupMenu {
    pub items: Vec<MenuEntry>,
    pub state: PopupMenuState,
    pub style: MenuStyle,
    /// Cached row labels for popup text and shortcuts.
    labels: PopupLabels,
    /// Widgets hosted by `MenuItem::widget_row` rows (`popup_local.rs`).
    row_widgets: RowWidgets,
    /// Set by `open_at_local`: the anchor in the host widget's local space,
    /// re-applied whenever the host's root origin changes.
    local_anchor: Option<Point>,
    /// The host widget's (0, 0) in root coordinates — see `popup_local.rs`.
    root_origin: Point,
}

impl PopupMenu {
    /// A closed popup over `items`, styled with this thread's
    /// [`current_menu_style`] ([`MenuStyle::default`] unless the host
    /// installed another with [`super::set_menu_style`]).
    pub fn new(items: Vec<MenuEntry>) -> Self {
        let mut menu = Self {
            items,
            state: PopupMenuState::default(),
            style: MenuStyle::default(),
            labels: PopupLabels::new(),
            row_widgets: RowWidgets::default(),
            local_anchor: None,
            root_origin: Point::ORIGIN,
        };
        menu.set_style(current_menu_style());
        menu
    }

    /// Restyle this popup.  Besides the paint values this applies the
    /// style's row height, width policy and fitted-width floor to the
    /// layout, so prefer it over assigning [`Self::style`] directly.
    /// A later [`Self::set_width`] / [`Self::set_min_width`] still wins.
    pub fn with_style(mut self, style: MenuStyle) -> Self {
        self.set_style(style);
        self
    }

    pub fn set_style(&mut self, style: MenuStyle) {
        self.state.set_width(style.width);
        self.state.set_min_width(style.min_width);
        self.state.set_row_height(style.row_h);
        self.style = style;
    }

    /// Choose the popup width policy — [`MenuWidth::FitContent`] sizes each
    /// panel to its widest row.  The default is [`MenuWidth::Fixed`].
    pub fn with_width(mut self, width: MenuWidth) -> Self {
        self.state.set_width(width);
        self
    }

    pub fn set_width(&mut self, width: MenuWidth) {
        self.state.set_width(width);
    }

    /// Narrowest a [`MenuWidth::FitContent`] panel may be (desktop logical
    /// px, touch-grown).  Defaults to [`super::fit_width::FIT_MIN_W`];
    /// `0.0` makes every panel exactly its widest row.  Applies to the root
    /// and all submenus.  Widget rows contribute no width of their own, so a
    /// popup of only widget rows needs a non-zero floor.
    pub fn with_min_width(mut self, min_width: f64) -> Self {
        self.state.set_min_width(min_width);
        self
    }

    pub fn set_min_width(&mut self, min_width: f64) {
        self.state.set_min_width(min_width);
    }

    /// Give a [`MenuWidth::FitContent`] popup the font and size it will be
    /// painted with, so events routed before its first paint hit-test the
    /// fitted panel.  [`Self::paint`] refreshes it every frame.
    pub fn set_measure_font(&mut self, font: Arc<Font>, font_size: f64) {
        self.state
            .set_fit_measure(FitMeasure::new(font, font_size, &self.style));
    }

    pub fn open_at(&mut self, pos: Point) {
        self.local_anchor = None;
        self.row_widgets.reset_interaction();
        self.state.open_at(pos, MenuAnchorKind::Context);
    }

    pub fn close(&mut self) {
        self.row_widgets.reset_interaction();
        self.state.close();
    }

    pub fn is_open(&self) -> bool {
        self.state.open
    }

    pub fn take_suppress_mouse_up(&mut self) -> bool {
        self.state.take_suppress_mouse_up()
    }

    pub fn handle_event(&mut self, event: &Event, viewport: Size) -> (EventResult, MenuResponse) {
        if self.state.open && !self.row_widgets.is_empty() {
            let layouts = self.state.layouts(&self.items, viewport);
            if let Some(result) = self.row_widgets.route(&self.items, &layouts, event) {
                return (result, MenuResponse::None);
            }
        }
        self.state.handle_event(&mut self.items, event, viewport)
    }

    /// Return `true` if `pos` falls inside any of the popup's currently
    /// laid-out panels (the open menu plus any nested submenus).  Used
    /// by `MenuBar` to detect a mouse-up in "neutral space" — outside
    /// both the menu bar AND the popup body — so the bar can dismiss
    /// the popup without waiting for a follow-up event.
    pub fn body_contains(&self, pos: Point, viewport: Size) -> bool {
        self.state
            .layouts(&self.items, viewport)
            .iter()
            .any(|layout| {
                pos.x >= layout.rect.x
                    && pos.x <= layout.rect.x + layout.rect.width
                    && pos.y >= layout.rect.y
                    && pos.y <= layout.rect.y + layout.rect.height
            })
    }

    pub fn handle_shortcut(&mut self, key: &Key, modifiers: Modifiers) -> MenuResponse {
        self.state.handle_shortcut(&mut self.items, key, modifiers)
    }

    pub fn paint(
        &mut self,
        ctx: &mut dyn DrawCtx,
        font: Arc<Font>,
        font_size: f64,
        viewport: Size,
    ) {
        self.set_measure_font(Arc::clone(&font), font_size);
        let layouts = self.state.layouts(&self.items, viewport);
        // Refresh the per-row `Label` cache against the current open
        // tree.  Cheap when nothing changed — `sync_to` only mutates
        // entries whose text or font differs from the cached state.
        self.labels.sync_to(
            &font,
            font_size,
            &self.items,
            &layouts,
            self.style.shortcut_format,
        );

        // Set the popup's default font / size on the ctx for the
        // inline glyphs we still paint directly (icons, check / radio
        // marks, submenu chevron).  Label widgets push their own font
        // through their internal `set_font` call so this only affects
        // the inline glyphs.
        ctx.set_font(Arc::clone(&font));
        ctx.set_font_size(font_size);

        for (level_idx, layout) in layouts.iter().enumerate() {
            paint_panel(ctx, layout.rect, &self.style);
            popup_paint::paint_popup_level(
                ctx,
                level_idx,
                layout,
                &self.items,
                &self.state,
                &self.style,
                &mut self.labels,
            );
            self.row_widgets.paint_level(ctx, &self.items, layout);
        }
        popup_paint::offer_hovered_row_tooltip(&self.items, &self.state, &layouts);
    }
}

#[cfg(test)]
mod tests_1;
#[cfg(test)]
mod tests_2;
#[cfg(test)]
mod tests_bar_children;
#[cfg(test)]
mod tests_fit;
#[cfg(test)]
mod tests_image_icon;
#[cfg(test)]
mod tests_rows;
#[cfg(test)]
mod tests_style;
#[cfg(test)]
mod tests_tooltip;
