//! One folder's row in the folder tree ([`TreeRow`]): its expand arrow (when the folder may have
//! folders of its own), its 20x20 icon (the look's art for the link's `micro_icon` key, tinted in
//! the accent) and its name, indented by its depth. A left press on the arrow opens or folds the
//! folder; a press anywhere else on the row (or where an arrow would be) shows the folder in the
//! list and opens it in the tree too, so its folders are one click away; an open row stays open,
//! so only the arrow folds a folder. A right press shows the folder and asks for its menu
//! (`menu.rs`), the press bubbling to the tree, which opens it. The "Loading..." row stands for no
//! folder: presses and menus pass it by.
//!
//! The row of the folder the list shows is drawn selected, its highlight's corners rounded by the
//! look's corner radius as the list's are. A name too long for its row is cut short with
//! `fit_name`, keeping a file's extension (an archive's ".zip"), and shows whole as the row's
//! tooltip.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::focus::FocusId;
use agg_gui::icon_image::IconImage;
use agg_gui::text::Font;
use agg_gui::{Color, DrawCtx, Event, EventResult, MouseButton, Rect, Size, Widget};

use crate::fitted_text::{centered_text_baseline, fill_text_within, fit_name};
use crate::list::tile_art::{paint_art, Art};
use crate::state::BrowserState;

use super::model::TreeModel;

/// One row's height: the 20 px image and its padding.
pub const TREE_ROW_HEIGHT: f64 = 24.0;
/// The tree's 5 px margin plus the root column's 10 px left margin.
pub(crate) const TREE_LEFT: f64 = 15.0;
/// The room above the first row.
pub(crate) const TREE_TOP: f64 = 5.0;
/// The indent of each level.
pub(crate) const INDENT: f64 = 16.0;
/// The arrow's box, then the image's (a 20x20 icon).
pub(crate) const CHEVRON_WIDTH: f64 = 22.0;
/// The side of a row's icon.
pub const TREE_ICON_SIZE: f64 = 20.0;

/// The colours and font a row draws with, from the look's style.
#[derive(Clone)]
pub(crate) struct TreeStyle {
    pub font: Arc<Font>,
    pub font_size: f64,
    pub text: Color,
    pub selected: Color,
    /// The corners of the selection highlight, as the list's items have.
    pub corner_radius: f64,
}

/// What a row is built from.
pub(crate) struct RowSpec {
    pub id: String,
    pub index: usize,
    pub name: String,
    pub depth: usize,
    pub expanded: bool,
    pub has_arrow: bool,
    pub placeholder: bool,
    pub selected: bool,
    pub icon: Option<IconImage>,
}

/// One folder's row: arrow, icon, name (see the module header).
pub struct TreeRow {
    spec: RowSpec,
    /// The name as it fits in the row; fitted when the row is given its bounds.
    shown_name: String,
    model: Rc<RefCell<TreeModel>>,
    state: Rc<RefCell<BrowserState>>,
    /// The tree's focus id, which a press on the row asks for.
    focus_id: FocusId,
    style: TreeStyle,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl TreeRow {
    pub(crate) fn new(
        spec: RowSpec,
        model: Rc<RefCell<TreeModel>>,
        state: Rc<RefCell<BrowserState>>,
        focus_id: FocusId,
        style: TreeStyle,
    ) -> Self {
        Self {
            shown_name: spec.name.clone(),
            spec,
            model,
            state,
            focus_id,
            style,
            bounds: Rect::default(),
            children: Vec::new(),
        }
    }

    fn chevron_left(&self) -> f64 {
        TREE_LEFT + self.spec.depth as f64 * INDENT
    }

    /// Where the name starts: after the arrow, the icon and a 4 unit gap.
    fn text_left(&self) -> f64 {
        self.chevron_left() + CHEVRON_WIDTH + TREE_ICON_SIZE + 4.0
    }

    /// The name's room in a row `width` wide: to the selection fill's right end, 2 short of the
    /// row's edge.
    fn name_room(&self, width: f64) -> f64 {
        width - self.text_left() - 2.0
    }

    /// The list shows this folder, opened from the root down.
    fn show_in_list(&self) {
        let path = match self.model.borrow().rows().get(self.spec.index) {
            Some(row) => row.path().to_vec(),
            None => return,
        };
        crate::trace(|| format!("tree row {} shows its folder", self.spec.name));
        let mut state = self.state.borrow_mut();
        state.go_to(0);
        for link in &path {
            state.open(link.as_ref());
        }
    }

    /// Paints the arrow: right when folded, down when open.
    fn paint_arrow(&self, ctx: &mut dyn DrawCtx, h: f64) {
        let left = self.chevron_left();
        let (cx, cy) = (left + CHEVRON_WIDTH / 2.0 - 1.0, h / 2.0);
        ctx.set_stroke_color(self.style.text);
        ctx.set_line_width(1.2);
        ctx.begin_path();
        if self.spec.expanded {
            ctx.move_to(cx - 4.0, cy + 2.0);
            ctx.line_to(cx, cy - 2.0);
            ctx.line_to(cx + 4.0, cy + 2.0);
        } else {
            ctx.move_to(cx - 2.0, cy + 4.0);
            ctx.line_to(cx + 2.0, cy);
            ctx.line_to(cx - 2.0, cy - 4.0);
        }
        ctx.stroke();
    }
}

impl Widget for TreeRow {
    fn type_name(&self) -> &'static str {
        "ContainerTreeNode"
    }
    fn id(&self) -> Option<&str> {
        Some(&self.spec.id)
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    /// The name is fitted to the width the row is given (its parent may measure it at another).
    fn set_bounds(&mut self, bounds: Rect) {
        let refit = bounds.width != self.bounds.width;
        self.bounds = bounds;
        if !refit {
            return;
        }
        self.shown_name = fit_name(
            &self.style.font,
            &self.spec.name,
            self.style.font_size,
            self.name_room(bounds.width),
        );
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        available
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let h = self.bounds.height;
        let left = self.chevron_left();
        let text_left = self.text_left();
        if self.spec.selected {
            ctx.set_fill_color(self.style.selected);
            ctx.begin_path();
            ctx.rounded_rect(
                text_left - 2.0,
                1.0,
                self.bounds.width - text_left,
                h - 2.0,
                self.style.corner_radius,
            );
            ctx.fill();
        }
        // No arrow for a folder known to have no folders; its room is kept so the rows still
        // line up.
        if self.spec.has_arrow {
            self.paint_arrow(ctx, h);
        }
        if let Some(icon) = &self.spec.icon {
            let rect = Rect::new(
                left + CHEVRON_WIDTH,
                (h - TREE_ICON_SIZE) / 2.0,
                TREE_ICON_SIZE,
                TREE_ICON_SIZE,
            );
            paint_art(ctx, &Art::Image(icon.clone()), rect);
        }
        ctx.set_fill_color(self.style.text);
        ctx.set_font(Arc::clone(&self.style.font));
        ctx.set_font_size(self.style.font_size);
        fill_text_within(
            ctx,
            &self.shown_name,
            text_left,
            // The one-em line centred in the row.
            centered_text_baseline(&self.style.font, self.style.font_size, h),
            self.name_room(self.bounds.width),
            h,
        );
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                agg_gui::focus::request_focus(self.focus_id);
                if self.spec.placeholder {
                    return EventResult::Consumed;
                }
                let left = self.chevron_left();
                let index = self.spec.index;
                // Without an arrow there is nothing to open: the press shows the folder, as a
                // press anywhere else on the row does.
                if self.spec.has_arrow && pos.x >= left && pos.x < left + CHEVRON_WIDTH {
                    self.model.borrow_mut().toggle(index, true);
                } else {
                    self.show_in_list();
                    // Showing a folder opens it too; an open one stays open (only the arrow
                    // folds). Not as a press on the arrow: a folder that needs a sign-in first
                    // stays folded without prompting, since showing it already says what it
                    // needs.
                    if self.spec.has_arrow && !self.spec.expanded {
                        self.model.borrow_mut().toggle(index, false);
                    }
                }
                agg_gui::animation::request_layout();
                EventResult::Consumed
            }
            // The folder shows in the list and its menu is asked for; the press bubbles up to
            // the tree, which opens the menu where it was pressed.
            Event::MouseDown {
                button: MouseButton::Right,
                ..
            } => {
                agg_gui::focus::request_focus(self.focus_id);
                if self.spec.placeholder {
                    return EventResult::Ignored;
                }
                self.show_in_list();
                self.model.borrow_mut().request_menu(self.spec.index);
                agg_gui::animation::request_layout();
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }
    /// The full name while it is cut short, so the tooltip shows what the row hides.
    fn tooltip_text(&self) -> Option<&str> {
        (self.shown_name != self.spec.name).then_some(self.spec.name.as_str())
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("name", self.spec.name.clone()),
            ("shown_name", self.shown_name.clone()),
            ("expanded", self.spec.expanded.to_string()),
            ("has_arrow", self.spec.has_arrow.to_string()),
            ("selected", self.spec.selected.to_string()),
            ("has_icon", self.spec.icon.is_some().to_string()),
        ]
    }
}
