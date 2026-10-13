//! Shared builders for the `FlowLayoutTests` port: C#'s widgets and
//! `BorderDouble` expressed with agg-gui's public types, plus tree helpers
//! (lay out a root at a size, read a child's bounds by path, snapshot every
//! bound for the resize-and-back legs).

use std::cell::Cell;
use std::rc::Rc;

use agg_gui::widgets::AbsoluteLayout;
use agg_gui::{
    Conditional, FlexColumn, FlexRow, HAnchor, Insets, Rect, Size, SizedBox, VAnchor, Widget,
};

/// C# `new BorderDouble(left, bottom, right, top)`.
pub fn border(left: f64, bottom: f64, right: f64, top: f64) -> Insets {
    Insets::from_sides(left, right, top, bottom)
}

/// The five padding/margin pairs every C# `*Tests` driver runs (the first
/// four of each driver are shared; the fifth differs per driver).
pub fn standard_pairs(fifth: (Insets, Insets)) -> Vec<(Insets, Insets)> {
    vec![
        (Insets::ZERO, Insets::ZERO),
        (Insets::ZERO, Insets::all(3.0)),
        (Insets::all(2.0), Insets::ZERO),
        (Insets::all(2.0), Insets::all(3.0)),
        fifth,
    ]
}

/// The fifth pair of the drivers that use `(1.1, 1.2, 1.3, 1.4)` /
/// `(2.1, 2.2, 2.3, 2.4)`.
pub fn fractional_pair() -> (Insets, Insets) {
    (border(1.1, 1.2, 1.3, 1.4), border(2.1, 2.2, 2.3, 2.4))
}

/// The fifth pair of the drivers that use `(2, 4, 6, 8)` / `(1, 3, 5, 7)`.
pub fn uneven_pair() -> (Insets, Insets) {
    (border(2.0, 4.0, 6.0, 8.0), border(1.0, 3.0, 5.0, 7.0))
}

/// C# `new GuiWidget(w, h)`: a fixed-size box whose minimum size is its
/// construction size (agg-sharp's `GuiWidget(width, height)` sets
/// `MinimumSize` to it).
pub fn gui_widget(w: f64, h: f64) -> SizedBox {
    SizedBox::fixed(w, h).with_min_size(Size::new(w, h))
}

/// A stand-in for a C# `Button`: a fixed box of the button's size.  The C#
/// tests read only a button's size, never its text or look.
pub fn button(w: f64, h: f64) -> SizedBox {
    gui_widget(w, h)
}

/// The sizes the C# buttons take: `"buttonLeft"`, `"buttonRight"`, ...
pub const BUTTON_H: f64 = 24.0;

/// C# `new FlowLayoutWidget(FlowDirection.TopToBottom)`: no gap, and a
/// FIT width like the C# default `HAnchor.Fit`.
pub fn top_to_bottom() -> FlexColumn {
    FlexColumn::new().with_gap(0.0).with_fit_width(true)
}

/// C# `new FlowLayoutWidget()` / `FlowDirection.LeftToRight`: no gap, FIT
/// width (C#'s default `HAnchor.Fit`).
pub fn left_to_right() -> FlexRow {
    FlexRow::new().with_gap(0.0).with_fit_width(true)
}

/// C# `new FlowLayoutWidget(FlowDirection.RightToLeft)`: no gap, FIT width.
pub fn right_to_left() -> FlexRow {
    left_to_right().with_right_to_left(true)
}

/// C# `new FlowLayoutWidget(FlowDirection.BottomToTop)`: no gap, FIT width.
pub fn bottom_to_top() -> FlexColumn {
    top_to_bottom().with_bottom_to_top(true)
}

/// C# `flow.AnchorAll()` on a top-to-bottom flow.
pub fn anchor_all_column(c: FlexColumn) -> FlexColumn {
    c.with_fit_width(false)
        .with_h_anchor(HAnchor::STRETCH)
        .with_v_anchor(VAnchor::STRETCH)
}

/// C# `flow.AnchorAll()` on a left-to-right flow.
pub fn anchor_all_row(r: FlexRow) -> FlexRow {
    r.with_fit_width(false)
        .with_h_anchor(HAnchor::STRETCH)
        .with_v_anchor(VAnchor::STRETCH)
}

/// C# `new GuiWidget(w, h)` used as a parent: an `AbsoluteLayout`.
pub fn container() -> AbsoluteLayout {
    AbsoluteLayout::new()
}

/// A child whose `Visible` the test toggles (C# `widget.Visible = false`).
pub fn hideable(child: Box<dyn Widget>) -> (Rc<Cell<bool>>, Box<dyn Widget>) {
    let visible = Rc::new(Cell::new(true));
    let w = Conditional::new(Rc::clone(&visible), child);
    (visible, Box::new(w))
}

/// Lay `root` out as a `w` x `h` top-level widget at the origin.
pub fn lay(root: &mut dyn Widget, w: f64, h: f64) {
    root.layout(Size::new(w, h));
    root.set_bounds(Rect::new(0.0, 0.0, w, h));
}

/// Lay a parentless widget out at its own natural size (C# reads a flow's
/// size before it has a parent): measured with no room offered, then laid
/// out again in the box it measured, as a parent would.
pub fn lay_unparented(root: &mut dyn Widget) -> Size {
    let s = root.layout(Size::new(0.0, 0.0));
    let s = root.layout(s);
    root.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
    s
}

/// The widget at `path` below `root`.
pub fn at<'a>(root: &'a dyn Widget, path: &[usize]) -> &'a dyn Widget {
    path.iter().fold(root, |w, &i| w.children()[i].as_ref())
}

/// The widget at `path` below `root`, mutably.
pub fn at_mut<'a>(root: &'a mut dyn Widget, path: &[usize]) -> &'a mut dyn Widget {
    let mut w = root;
    for &i in path {
        w = w.children_mut()[i].as_mut();
    }
    w
}

/// C# `BoundsRelativeToParent` of the widget at `path`.
pub fn bounds(root: &dyn Widget, path: &[usize]) -> Rect {
    at(root, path).bounds()
}

/// The bounds of the widget at `path` in `root`'s coordinates (C#
/// `TransformToScreenSpace` of the widget's box, for a top-level `root`).
pub fn rect_in_root(root: &dyn Widget, path: &[usize]) -> Rect {
    let mut w = root;
    let (mut x, mut y) = (0.0, 0.0);
    for &i in path {
        w = w.children()[i].as_ref();
        let b = w.bounds();
        x += b.x;
        y += b.y;
    }
    let b = w.bounds();
    Rect::new(x, y, b.width, b.height)
}

/// Every widget's bounds, depth first (the picture a C# back buffer holds).
pub fn snapshot(root: &dyn Widget) -> Vec<Rect> {
    let mut out = vec![root.bounds()];
    for c in root.children() {
        out.extend(snapshot(c.as_ref()));
    }
    out
}

/// Set the width of the `SizedBox` at `path` (C# `widget.Width = w`).
pub fn set_box_width(root: &mut dyn Widget, path: &[usize], w: f64) {
    let b = at_mut(root, path)
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<SizedBox>())
        .expect("a SizedBox");
    b.width = Some(w);
}
