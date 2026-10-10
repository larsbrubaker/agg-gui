//! `FlowLeftRightWithWrapping` — items laid left to right in rows that wrap
//! at the flow's width, a port of agg-sharp `Gui/FlowLeftRightWithWrapping.cs`.
//!
//! The flow's children are its rows ([`WrapRow`]); each row's children are the
//! items that fit on it, plus the spacers the flow inserts for
//! [`Proportional`](FlowLeftRightWithWrapping::proportional) spacing and right or
//! centred rows ([`RowSpacer`]). Items move between rows whenever the flow's
//! width changes (C#'s `DoWrappingLayout`, run from `layout` here): a row breaks
//! before an item that would run past the flow's width less the row's chrome
//! (its margin, border and padding and the flow's padding), and at every
//! [`HardBreak`]. A [`SkipIfFirstSpace`] that would open a row is left out of
//! it (kept for the next wrap). Rows after the first carry
//! [`row_border`](FlowLeftRightWithWrapping::row_border), so a top-only border
//! separates rows.
//!
//! Units are logical, as everywhere in agg-gui; C# measured the same layout in
//! device pixels. C# re-wraps from `OnBoundsChanged` and guards the re-entrancy
//! that causes; here layout is a function of the width it is given, so the
//! rows are rebuilt when the width (or the item list) changed and the flow's
//! height always encloses its rows.
//!
//! [`ResponsiveImageWidget`](crate::widgets::responsive_image_widget) is the
//! other agg-sharp widget MatterCAD's store page shows in these flows.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::spacers::Spacer;
use crate::widgets::Label;

/// C# `HardBreak` (`IHardBreak`): ends the row it is in; 1 x 1.
pub struct HardBreak {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl HardBreak {
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
        }
    }
}

impl Default for HardBreak {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for HardBreak {
    crate::widgets::widget_as_any!();
    fn type_name(&self) -> &'static str {
        "HardBreak"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(1.0, 1.0)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// C# `SkipIfFirstSpace` (`ISkipIfFirst`): a space between words that is left
/// out when it would open a row.
pub struct SkipIfFirstSpace {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl SkipIfFirstSpace {
    /// A " " label at `font_size` in `color`.
    pub fn new(font: Arc<Font>, font_size: f64, color: Color) -> Self {
        let label = Label::new(" ", font)
            .with_font_size(font_size)
            .with_color(color);
        Self {
            bounds: Rect::default(),
            children: vec![Box::new(label)],
        }
    }
}

impl Widget for SkipIfFirstSpace {
    crate::widgets::widget_as_any!();
    fn type_name(&self) -> &'static str {
        "SkipIfFirstSpace"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        let size = self.children[0].layout(available);
        self.children[0].set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
        size
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// A space the flow puts in a row (C#'s `new GuiWidget(extraMargin, 2)` and
/// the `HorizontalSpacer` carried to the next row): not an item, so it is
/// dropped when the rows are rebuilt.
pub struct RowSpacer {
    inner: Spacer,
}

impl RowSpacer {
    /// A fixed `width` x 2 spacer.
    fn fixed(width: f64) -> Self {
        let size = Size::new(width, 2.0);
        Self {
            inner: Spacer::new().with_min_size(size).with_max_size(size),
        }
    }

    /// A spacer that stretches over the row's room (C# `HorizontalSpacer`).
    fn stretch() -> Self {
        Self {
            inner: Spacer::new()
                .with_h_anchor(HAnchor::STRETCH)
                .with_max_size(Size::new(f64::MAX, 2.0)),
        }
    }
}

impl Widget for RowSpacer {
    crate::widgets::widget_as_any!();
    fn type_name(&self) -> &'static str {
        "RowSpacer"
    }
    fn bounds(&self) -> Rect {
        self.inner.bounds()
    }
    fn set_bounds(&mut self, b: Rect) {
        self.inner.set_bounds(b);
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        self.inner.children()
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        self.inner.children_mut()
    }
    fn h_anchor(&self) -> HAnchor {
        self.inner.h_anchor()
    }
    fn min_size(&self) -> Size {
        self.inner.min_size()
    }
    fn max_size(&self) -> Size {
        self.inner.max_size()
    }
    fn layout(&mut self, available: Size) -> Size {
        self.inner.layout(available)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn is_generated(widget: &dyn Widget) -> bool {
    widget.as_any().is_some_and(|any| any.is::<RowSpacer>())
}

fn is_hard_break(widget: &dyn Widget) -> bool {
    widget.as_any().is_some_and(|any| any.is::<HardBreak>())
}

fn is_skip_if_first(widget: &dyn Widget) -> bool {
    widget
        .as_any()
        .is_some_and(|any| any.is::<SkipIfFirstSpace>())
}

fn is_stretch(widget: &dyn Widget) -> bool {
    widget.h_anchor() == HAnchor::STRETCH
}

/// C# `ItemWidth`: the room `child` takes in a row - its width and margin; a
/// stretching item counts at its minimum width.
fn item_width(child: &mut dyn Widget, available: f64) -> f64 {
    let width = if is_stretch(child) {
        child.min_size().width
    } else {
        child.layout(Size::new(available, f64::MAX)).width
    };
    let m = child.margin();
    width + m.left + m.right
}

pub use super::flow_wrap_row::WrapRow;

/// C# `FlowLeftRightWithWrapping`.
pub struct FlowLeftRightWithWrapping {
    bounds: Rect,
    /// The rows (C#'s row `FlowLayoutWidget`s).
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    /// Items added since the last wrap, in order.
    pending: Vec<Box<dyn Widget>>,
    /// Items the last wrap left out of every row (a `SkipIfFirstSpace` that
    /// would have opened one), with their place in the item sequence.
    parked: Vec<(usize, Box<dyn Widget>)>,
    /// C# `Padding`.
    pub padding: Insets,
    /// C# `RowMargin` (default 3 left and right).
    pub row_margin: Insets,
    /// C# `RowPadding` (default 3 all round).
    pub row_padding: Insets,
    /// C# `RowBorder`: the border of every row after the first.
    pub row_border: Insets,
    /// C# `RowBorderColor`.
    pub row_border_color: Color,
    /// C# `Proportional`: spread each row's spare width between its items.
    pub proportional: bool,
    /// C# `Center`: centre each row (overrides `content_h_anchor`).
    pub center: bool,
    content_h_anchor: HAnchor,
    /// The width when the flow is not stretched (C# `Width` with an absolute
    /// or left anchor).
    width: f64,
    /// C# `MaxLineWidth`: the widest row's items, after the last wrap.
    max_line_width: f64,
    /// C#'s `wrappedWidth`: the width the rows were last wrapped against.
    wrapped_width: Option<f64>,
}

impl Default for FlowLeftRightWithWrapping {
    fn default() -> Self {
        Self::new()
    }
}

impl FlowLeftRightWithWrapping {
    /// An empty flow that stretches across its parent (C# `HAnchor.Stretch`).
    pub fn new() -> Self {
        let mut base = WidgetBase::new();
        base.h_anchor = HAnchor::STRETCH;
        base.v_anchor = VAnchor::FIT;
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base,
            pending: Vec::new(),
            parked: Vec::new(),
            padding: Insets::ZERO,
            row_margin: Insets::symmetric(3.0, 0.0),
            row_padding: Insets::all(3.0),
            row_border: Insets::ZERO,
            row_border_color: Color::rgba(0.0, 0.0, 0.0, 0.0),
            proportional: false,
            center: false,
            content_h_anchor: HAnchor::LEFT,
            width: 0.0,
            max_line_width: 0.0,
            wrapped_width: None,
        }
    }

    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }

    /// C# `Width` for a flow that is not stretched.
    pub fn set_width(&mut self, width: f64) {
        self.width = width;
    }

    /// C# `ContentHAnchor`: where a row's items sit when they leave room
    /// (left, centre or right). Changing it re-wraps at the next layout.
    pub fn content_h_anchor(&self) -> HAnchor {
        self.content_h_anchor
    }

    pub fn set_content_h_anchor(&mut self, anchor: HAnchor) {
        if self.content_h_anchor != anchor {
            self.content_h_anchor = anchor;
            self.wrapped_width = None;
        }
    }

    /// C# `MaxLineWidth`.
    pub fn max_line_width(&self) -> f64 {
        self.max_line_width
    }

    /// C# `AddChild`: appends an item. It joins a row at the next layout.
    pub fn add_child(&mut self, child: Box<dyn Widget>) {
        self.pending.push(child);
        self.wrapped_width = None;
    }

    /// C# `AddChild` of an `IWrapChildrenSeparatly` widget: its children wrap
    /// as items of their own.
    pub fn add_children_separately(&mut self, mut container: Box<dyn Widget>) {
        for child in container.children_mut().drain(..) {
            self.add_child(child);
        }
    }

    /// C# `AddText`: each word a label, a [`SkipIfFirstSpace`] between words
    /// and a [`HardBreak`] at each line break.
    pub fn add_text(&mut self, text: &str, font: Arc<Font>, color: Color, font_size: f64) {
        for (line_index, line) in text.split('\n').enumerate() {
            if line_index > 0 {
                self.add_child(Box::new(HardBreak::new()));
            }
            for (word_index, word) in line.split(' ').enumerate() {
                if word_index > 0 {
                    self.add_child(Box::new(SkipIfFirstSpace::new(
                        Arc::clone(&font),
                        font_size,
                        color,
                    )));
                }
                if word != " " {
                    let label = Label::new(word, Arc::clone(&font))
                        .with_font_size(font_size)
                        .with_color(color);
                    self.add_child(Box::new(label));
                }
            }
        }
    }

    /// C# `ContentWidth`: how wide one row holding every item would be - each
    /// item's width and margin end to end, without the row's chrome. Known
    /// before the flow has ever wrapped.
    pub fn content_width(&mut self) -> f64 {
        let mut items = self.take_items();
        let total = items
            .iter_mut()
            .map(|i| item_width(i.as_mut(), f64::MAX))
            .sum();
        self.pending = items;
        self.wrapped_width = None;
        total
    }

    /// Every item, in order, out of the rows, the parked list and the pending
    /// list; the rows themselves are dropped (C# closes them).
    fn take_items(&mut self) -> Vec<Box<dyn Widget>> {
        let mut items: Vec<Box<dyn Widget>> = Vec::new();
        for mut row in self.children.drain(..) {
            for child in row.children_mut().drain(..) {
                if !is_generated(child.as_ref()) {
                    items.push(child);
                }
            }
        }
        for (index, item) in self.parked.drain(..) {
            let at = index.min(items.len());
            items.insert(at, item);
        }
        items.append(&mut self.pending);
        items
    }

    fn new_row(&self, bordered: bool) -> WrapRow {
        let border = if bordered {
            self.row_border
        } else {
            Insets::ZERO
        };
        WrapRow::new(
            self.row_margin,
            self.row_padding,
            border,
            self.row_border_color,
        )
    }

    /// C# `RowChromeWidth`: the width a row's items cannot use.
    fn row_chrome_width(&self, row: &WrapRow) -> f64 {
        let m = row.margin();
        let (b, p) = (row.border(), row.padding());
        m.left
            + m.right
            + b.left
            + b.right
            + p.left
            + p.right
            + self.padding.left
            + self.padding.right
    }

    /// C# `DoWrappingLayout`: the items into rows at `width`.
    fn do_wrapping_layout(&mut self, width: f64) {
        self.wrapped_width = Some(width);
        let items = self.take_items();
        let mut rows: Vec<WrapRow> = vec![self.new_row(false)];
        let mut row_chrome = self.row_chrome_width(&rows[0]);
        let mut running_size = 0.0;
        self.max_line_width = 0.0;
        for (index, mut child) in items.into_iter().enumerate() {
            let child_width = item_width(child.as_mut(), width);
            if running_size + child_width > width - row_chrome || is_hard_break(child.as_ref()) {
                self.max_line_width = self.max_line_width.max(running_size);
                running_size = 0.0;
                let carry_spacer = rows
                    .last()
                    .and_then(|row| row.children().last())
                    .is_some_and(|last| {
                        // C# `HorizontalSpacer`: a spacer stretching over the row's room.
                        matches!(last.type_name(), "Spacer" | "RowSpacer")
                            && is_stretch(last.as_ref())
                    });
                let mut row = self.new_row(true);
                if carry_spacer {
                    row.push(Box::new(RowSpacer::stretch()));
                }
                row_chrome = self.row_chrome_width(&row);
                rows.push(row);
            }
            if running_size > 0.0 || !is_skip_if_first(child.as_ref()) {
                if let Some(row) = rows.last_mut() {
                    row.push(child);
                }
                running_size += child_width;
                self.max_line_width = self.max_line_width.max(running_size);
            } else {
                self.parked.push((index, child));
            }
        }
        // C# `MakeProportionalIfRequired` then `AlignRowsIfRequired`.
        if self.proportional {
            for row in &mut rows {
                let extra = width - row.content_width() - self.row_chrome_width(row);
                let count = row.children().len();
                if extra > count as f64 {
                    let extra_margin = extra / (count as f64 + 1.0);
                    for i in (0..=count).rev() {
                        row.insert(i, Box::new(RowSpacer::fixed(extra_margin)));
                    }
                }
            }
        }
        let anchor = if self.center {
            HAnchor::CENTER
        } else {
            self.content_h_anchor
        };
        if anchor.bits() & (HAnchor::CENTER.bits() | HAnchor::RIGHT.bits()) != 0 {
            for row in &mut rows {
                let extra = width - row.content_width() - self.row_chrome_width(row);
                let count = row.children().len();
                if extra > count as f64 {
                    let leading = if anchor.bits() & HAnchor::RIGHT.bits() == HAnchor::RIGHT.bits()
                    {
                        extra
                    } else {
                        extra / 2.0
                    };
                    row.insert(0, Box::new(RowSpacer::fixed(leading)));
                }
            }
        }
        self.children = rows
            .into_iter()
            .map(|row| Box::new(row) as Box<dyn Widget>)
            .collect();
    }
}

impl Widget for FlowLeftRightWithWrapping {
    crate::widgets::widget_as_any!();
    fn type_name(&self) -> &'static str {
        "FlowLeftRightWithWrapping"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn margin(&self) -> Insets {
        self.base.margin
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn h_anchor(&self) -> HAnchor {
        self.base.h_anchor
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }
    fn min_size(&self) -> Size {
        self.base.min_size
    }
    fn max_size(&self) -> Size {
        self.base.max_size
    }

    fn layout(&mut self, available: Size) -> Size {
        let width = if self.base.h_anchor == HAnchor::STRETCH {
            available.width
        } else {
            self.width
        };
        if self.wrapped_width != Some(width) || !self.pending.is_empty() {
            self.do_wrapping_layout(width);
        }
        // Rows top to bottom (Y-up: the first row at the top).
        let pad = self.padding;
        let mut heights = Vec::with_capacity(self.children.len());
        let mut total = pad.top + pad.bottom;
        for row in &mut self.children {
            let m = row.margin();
            let row_width = (width - pad.left - pad.right - m.left - m.right).max(0.0);
            let h = row.layout(Size::new(row_width, f64::MAX)).height;
            heights.push((row_width, h));
            total += h + m.top + m.bottom;
        }
        let mut y = total - pad.top;
        for (row, (row_width, h)) in self.children.iter_mut().zip(heights) {
            let m = row.margin();
            y -= m.top + h;
            row.set_bounds(Rect::new(pad.left + m.left, y, row_width, h));
            y -= m.bottom;
        }
        Size::new(width, total)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}
