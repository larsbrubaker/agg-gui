//! The formatting toolbar's widgets: `MarkdownFormatButton` (one icon button,
//! ports `MarkdownFormatButton.cs`), `MarkdownFormatGroup` (a run of buttons
//! drawn as one segmented control, ports `MarkdownFormatGroup.cs`), the size
//! list, the wrapping strip that holds them, and the toolbar's own `Widget`
//! implementation (the strip over the link row).  The toolbar's logic is in
//! `format_toolbar`.
//!
//! A button is agg-gui's [`Button`] in its flat mode (`with_flat`) with a Font
//! Awesome icon in its em box (`with_icon_em_box`, so every icon shares one
//! baseline), taking the clicks and the tooltip; the *group* paints the
//! cells (the accent tint of a pressed style, the faint shade of a mixed one,
//! hover and press) with the outline and dividers, in one device-pixel grid
//! (`format_toolbar_cells`), so the strokes are crisp at any scale and the end
//! cells follow the outline's rounded corners.  The strip reads each group's
//! and the size list's margin and anchor, so groups and list are spaced evenly
//! and stand on one baseline; the list is as tall as the groups.  Sizes are in
//! agg-gui's logical units, so C#'s `* DeviceScale` factors fall away (the
//! renderer applies the scale).

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::{Button, ComboBox, ComboBoxStyle, FlowLeftRightWithWrapping};

use super::format_toolbar::{
    MarkdownFormatButtonKind, MarkdownFormatToolbar, ToolbarRequest, ToolbarShared,
    DESIGN_EDGE_SPACE, ROW_DESIGN_PADDING,
};
use super::format_toolbar_cells::{icon_color, paint_group};

/// The button's width in design units.
pub const BUTTON_DESIGN_WIDTH: f64 = 30.0;
/// The button's height in design units.
pub const BUTTON_DESIGN_HEIGHT: f64 = 28.0;
/// The icon font's em, in design units.
pub const ICON_DESIGN_SIZE: f64 = 14.0;
/// The group outline's corner radius, in design units.
pub const GROUP_DESIGN_RADIUS: f64 = 7.0;
/// The room between two groups, in design units (half on each side).
pub const GROUP_DESIGN_SPACING: f64 = 6.0;

/// A group's outer height: a button plus the outline above and below it.
pub fn group_height() -> f64 {
    BUTTON_DESIGN_HEIGHT + 2.0
}

pub(super) fn toolbar_base() -> WidgetBase {
    let mut base = WidgetBase::new();
    base.h_anchor = HAnchor::STRETCH;
    base.v_anchor = VAnchor::FIT;
    base
}

/// The wrapping strip: the style group, the size list, then the other groups.
pub(super) fn build_strip(
    shared: &Rc<ToolbarShared>,
    font: &Arc<Font>,
    icon_font: &Arc<Font>,
    mac: bool,
) -> FlowLeftRightWithWrapping {
    let mut strip = FlowLeftRightWithWrapping::new().with_h_anchor(HAnchor::STRETCH);
    // One unit of each row's own padding (which also keeps wrapped rows apart); the rest of
    // the edge space is the strip's vertical padding, leaving its width unchanged.
    strip.row_padding = Insets::symmetric(0.0, ROW_DESIGN_PADDING);
    strip.row_margin = Insets::ZERO;
    strip.padding = Insets::symmetric(0.0, DESIGN_EDGE_SPACE - ROW_DESIGN_PADDING);
    for (i, kinds) in MarkdownFormatButtonKind::GROUPS.iter().enumerate() {
        let mut group = MarkdownFormatGroup::new(shared);
        for kind in kinds.iter() {
            group.add_button(MarkdownFormatButton::new(
                shared, *kind, icon_font, font, mac,
            ));
        }
        strip.add_child(Box::new(group));
        if i == 0 {
            strip.add_child(Box::new(SizeListWidget::new(shared, font)));
        }
    }
    strip
}

/// One formatting button: an icon [`Button`] whose enabled and pressed states
/// follow the toolbar, over the faint shade of a mixed selection.
pub struct MarkdownFormatButton {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
    kind: MarkdownFormatButtonKind,
}

impl MarkdownFormatButton {
    fn new(
        shared: &Rc<ToolbarShared>,
        kind: MarkdownFormatButtonKind,
        icon_font: &Arc<Font>,
        font: &Arc<Font>,
        mac: bool,
    ) -> Self {
        let (enabled, click) = (Rc::clone(shared), Rc::clone(shared));
        let index = kind.index();
        let colors = Rc::clone(shared);
        let button = Button::new("", Arc::clone(font))
            .with_icon_sized(kind.icon(), Arc::clone(icon_font), ICON_DESIGN_SIZE)
            .with_icon_em_box()
            // The group paints the cells (tint, hover, press) so the end cells take its rounded
            // corners; the button draws the icon and takes the clicks.
            .with_flat()
            .with_compact()
            .with_focus_on_click(false)
            .with_enabled_fn(move || enabled.model.borrow().buttons[index].enabled)
            .with_icon_color_fn(move |v| {
                let state = colors.model.borrow().buttons[index];
                Some(icon_color(v, state.enabled, state.is_pressed()))
            })
            .on_click(move || click.push(ToolbarRequest::Click(kind)))
            .with_min_size(Size::new(BUTTON_DESIGN_WIDTH, group_height() - 2.0))
            .with_max_size(Size::new(BUTTON_DESIGN_WIDTH, group_height() - 2.0))
            .with_tooltip(kind.tool_tip(mac));
        Self {
            bounds: Rect::default(),
            base: WidgetBase::new(),
            children: vec![Box::new(button)],
            kind,
        }
    }

    /// The button this widget shows.
    pub fn kind(&self) -> MarkdownFormatButtonKind {
        self.kind
    }
}

impl Widget for MarkdownFormatButton {
    crate::widgets::widget_as_any!();

    fn type_name(&self) -> &'static str {
        "MarkdownFormatButton"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }

    fn layout(&mut self, _available: Size) -> Size {
        let size = Size::new(BUTTON_DESIGN_WIDTH, group_height() - 2.0);
        if let Some(button) = self.children.first_mut() {
            button.layout(size);
            button.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
        }
        size
    }

    // The group paints the cell behind the icon (see `MarkdownFormatGroup::paint`).
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// A run of related buttons drawn as one segmented control: a rounded
/// outline around them all and a one-unit divider between each pair.  The
/// strip wraps a group whole.
pub struct MarkdownFormatGroup {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
    shared: Rc<ToolbarShared>,
}

impl MarkdownFormatGroup {
    fn new(shared: &Rc<ToolbarShared>) -> Self {
        let mut base = WidgetBase::new();
        base.margin = Insets::symmetric(GROUP_DESIGN_SPACING / 2.0, 0.0);
        base.v_anchor = VAnchor::CENTER;
        Self {
            bounds: Rect::default(),
            base,
            children: Vec::new(),
            shared: Rc::clone(shared),
        }
    }

    fn add_button(&mut self, button: MarkdownFormatButton) {
        self.children.push(Box::new(button));
    }

    /// The group's buttons, left to right.
    pub fn buttons(&self) -> Vec<MarkdownFormatButtonKind> {
        self.children
            .iter()
            .filter_map(|c| c.as_any()?.downcast_ref::<MarkdownFormatButton>())
            .map(MarkdownFormatButton::kind)
            .collect()
    }
}

impl Widget for MarkdownFormatGroup {
    crate::widgets::widget_as_any!();

    fn type_name(&self) -> &'static str {
        "MarkdownFormatGroup"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    // The strip reads these from the widget, not its base: without them the groups and the size
    // list ran together with no gap between them and sat on the row's bottom edge.
    fn margin(&self) -> Insets {
        self.base.margin
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }

    // One unit of padding inside the outline and one between buttons, where the dividers go.
    fn layout(&mut self, available: Size) -> Size {
        let mut x = 1.0;
        for (i, child) in self.children.iter_mut().enumerate() {
            if i > 0 {
                x += 1.0;
            }
            let size = child.layout(available);
            child.set_bounds(Rect::new(x, 1.0, size.width, size.height));
            x += size.width;
        }
        Size::new(x + 1.0, group_height())
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        paint_group(ctx, self.bounds, &self.children, &self.shared);
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// The size list: Normal, Heading 1-3 and, while the selection is on one, a
/// deeper heading.  Blank over mixed sizes.  Rebuilt when its entries change.
struct SizeListWidget {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
    shared: Rc<ToolbarShared>,
    font: Arc<Font>,
    built_items: Vec<String>,
    shown: Rc<Cell<Option<usize>>>,
}

impl SizeListWidget {
    fn new(shared: &Rc<ToolbarShared>, font: &Arc<Font>) -> Self {
        let mut base = WidgetBase::new();
        base.margin = Insets::symmetric(GROUP_DESIGN_SPACING / 2.0, 0.0);
        base.v_anchor = VAnchor::CENTER;
        let mut list = Self {
            bounds: Rect::default(),
            base,
            children: Vec::new(),
            shared: Rc::clone(shared),
            font: Arc::clone(font),
            built_items: Vec::new(),
            shown: Rc::new(Cell::new(None)),
        };
        list.rebuild();
        list
    }

    fn rebuild(&mut self) {
        let items = self.shared.model.borrow().size_items.clone();
        let (enabled, change, shown) = (
            Rc::clone(&self.shared),
            Rc::clone(&self.shared),
            Rc::clone(&self.shown),
        );
        let combo = ComboBox::new(items.clone(), 0, Arc::clone(&self.font))
            .with_no_selection("")
            .with_fit_width(true)
            // As tall as the groups, with their corner radius, so the row reads as one set.
            .with_style(ComboBoxStyle {
                height: Some(group_height()),
                radius: Some(GROUP_DESIGN_RADIUS),
                ..ComboBoxStyle::default()
            })
            .with_enabled_fn(move || enabled.model.borrow().size_enabled)
            .on_change(move |index| {
                // A pick of the entry already shown changes nothing (C#'s `refreshing` guard).
                if shown.get() != Some(index) {
                    change.push(ToolbarRequest::Size(index));
                }
            })
            .with_min_size(Size::new(100.0, group_height()))
            .with_tooltip("Text size");
        self.children = vec![Box::new(combo)];
        self.built_items = items;
        self.shown.set(None);
    }

    fn combo(&mut self) -> Option<&mut ComboBox> {
        self.children
            .first_mut()?
            .as_any_mut()?
            .downcast_mut::<ComboBox>()
    }
}

impl Widget for SizeListWidget {
    crate::widgets::widget_as_any!();

    fn type_name(&self) -> &'static str {
        "MarkdownSizeList"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    // The strip reads these from the widget, not its base: without them the groups and the size
    // list ran together with no gap between them and sat on the row's bottom edge.
    fn margin(&self) -> Insets {
        self.base.margin
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }

    fn layout(&mut self, available: Size) -> Size {
        if self.shared.model.borrow().size_items != self.built_items {
            self.rebuild();
        }
        let index = self.shared.model.borrow().size_index;
        if self.shown.get() != index {
            self.shown.set(index);
            if let Some(combo) = self.combo() {
                combo.set_selected_index(index);
            }
        }
        let Some(combo) = self.children.first_mut() else {
            return Size::new(0.0, 0.0);
        };
        let size = combo.layout(available);
        let size = Size::new(size.width.max(100.0), group_height());
        combo.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
        size
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

impl Widget for MarkdownFormatToolbar {
    crate::widgets::widget_as_any!();

    fn type_name(&self) -> &'static str {
        "MarkdownFormatToolbar"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }

    // The strip on top (its padding holds the edge space), the link row under it.
    fn layout(&mut self, available: Size) -> Size {
        let width = available.width;
        let mut row_height = 0.0;
        if let Some(row) = self.children.get_mut(1) {
            if row.is_visible() {
                row_height = row.layout(Size::new(width, available.height)).height;
                row.set_bounds(Rect::new(0.0, 0.0, width, row_height));
            } else {
                row.set_bounds(Rect::new(0.0, 0.0, width, 0.0));
            }
        }
        let mut strip_height = 0.0;
        if let Some(strip) = self.children.first_mut() {
            strip_height = strip.layout(Size::new(width, available.height)).height;
            strip.set_bounds(Rect::new(0.0, row_height, width, strip_height));
        }
        Size::new(width, row_height + strip_height)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}
