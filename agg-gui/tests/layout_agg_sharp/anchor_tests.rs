//! Port of agg-sharp `AnchorTests.cs`: the anchor layout rules of a plain
//! parent (agg-gui `AbsoluteLayout`, C# `GuiWidget` with
//! `LayoutEngineSimpleAlign`).
//!
//! C# builds each picture twice — once positioned by hand, once by anchors —
//! and compares the rendered back buffers. These ports do the same: a
//! "control" layout places the widget at the hand-computed origin and size, a
//! "test" layout anchors it, and both the child rectangles and the rendered
//! pixels must match. C#'s `BorderDouble(l, b, r, t)` is
//! `Insets::from_sides(l, r, t, b)`.
//!
//! C#'s `LocalBounds` can sit off the widget's origin (`Offset(-10, -10)`);
//! agg-gui bounds have no local origin, so the offset legs of
//! `AnchorLeftBottomTests` and `CenterBothOffsetBoundsTests` build the same
//! picture with an un-offset widget (the offset does not move the drawn
//! widget in C#, which is what those legs check).

use agg_gui::widgets::AbsoluteLayout;
use agg_gui::{Button, Framebuffer, HAnchor, Insets, Label, Rect, Size, VAnchor, Widget};

use super::support::{bounds_at, font, layout_at, render, sized};

/// C# `new BorderDouble(left, bottom, right, top)`.
fn bd(left: f64, bottom: f64, right: f64, top: f64) -> Insets {
    Insets::from_sides(left, right, top, bottom)
}

fn button(text: &str) -> Button {
    Button::new(text, font())
}

/// The natural size a widget reports in a `room`-sized slot.
fn natural(w: &mut dyn Widget, room: Size) -> Size {
    w.layout(room)
}

/// C# `Math.Round` (banker's), applied by `OriginRelativeParent`'s setter
/// when the widget enforces integer bounds.
fn round_even(v: f64) -> f64 {
    let r = v.round();
    if (v - v.trunc()).abs() == 0.5 {
        2.0 * (v / 2.0).round()
    } else {
        r
    }
}

/// Lay out and render both trees at `size`, then require the same child
/// rectangle and the same pixels.
fn assert_same_picture(control: &mut AbsoluteLayout, test: &mut AbsoluteLayout, size: Size) {
    layout_at(control, size);
    layout_at(test, size);
    let (a, b) = (bounds_at(control, &[0]), bounds_at(test, &[0]));
    assert_rect_eq(a, b);
    let (w, h) = (size.width as u32, size.height as u32);
    let (ca, cb): (Framebuffer, Framebuffer) = (render(control, w, h), render(test, w, h));
    let diff: Vec<usize> = (0..ca.pixels().len() / 4)
        .filter(|i| ca.pixels()[i * 4..i * 4 + 4] != cb.pixels()[i * 4..i * 4 + 4])
        .collect();
    assert!(
        diff.is_empty(),
        "The Anchored widget should be in the correct place: {} pixels differ, first at {:?}, rect {a:?}",
        diff.len(),
        diff.first().map(|i| (i % w as usize, i / w as usize))
    );
}

fn assert_rect_eq(a: Rect, b: Rect) {
    let close = |x: f64, y: f64| (x - y).abs() <= 1e-9;
    assert!(
        close(a.x, b.x) && close(a.y, b.y) && close(a.width, b.width) && close(a.height, b.height),
        "control {a:?} != anchored {b:?}"
    );
}

/// Set `w`'s margin and anchors through its `WidgetBase` (C# `Margin`,
/// `HAnchor`, `VAnchor`).
fn based<W: Widget>(mut w: W, margin: Insets, h: Option<HAnchor>, v: Option<VAnchor>) -> W {
    let base = w.widget_base_mut().expect("widget embeds a WidgetBase");
    base.margin = margin;
    if let Some(h) = h {
        base.h_anchor = h;
    }
    if let Some(v) = v {
        base.v_anchor = v;
    }
    w
}

/// A control child pinned to `origin` at a fixed `size`.
fn pinned<W: Widget + 'static>(w: W, origin: (f64, f64), size: Size) -> Box<dyn Widget> {
    let mut w = w.with_origin(origin.0, origin.1);
    if let Some(base) = w.widget_base_mut() {
        base.min_size = size;
        base.max_size = size;
    }
    Box::new(w)
}

#[test]
fn bottom_and_top() {
    for (p, m) in [
        (0.0, 0.0),
        (0.0, 3.0),
        (2.0, 0.0),
        (2.2, 3.3),
        (0.0, 3.3),
        (2.2, 0.0),
        (2.2, 3.3),
    ] {
        bottom_and_top_text_control(p, m);
    }
    for (p, m) in [
        (0.0, 0.0),
        (0.0, 3.0),
        (2.0, 0.0),
        (2.2, 3.3),
        (0.0, 3.3),
        (2.2, 0.0),
        (2.2, 3.3),
    ] {
        bottom_and_top_button(p, m);
    }
}

fn bottom_and_top_text_control(control_padding: f64, button_margin: f64) {
    bottom_and_top_case(
        || Label::new("text1", font()),
        control_padding,
        button_margin,
        Size::new(200.0, 200.0),
    );
}

fn bottom_and_top_button(control_padding: f64, button_margin: f64) {
    bottom_and_top_case(
        || button("button1"),
        control_padding,
        button_margin,
        Size::new(200.0, 300.0),
    );
}

/// A BOTTOM|TOP child of a padded parent spans the parent's height less
/// padding and margin; `first` is the size the test parent is laid out at
/// before it takes the control's 200 x 300.
fn bottom_and_top_case<W: Widget + 'static>(
    make: impl Fn() -> W,
    control_padding: f64,
    button_margin: f64,
    first: Size,
) {
    let full = Size::new(200.0, 300.0);
    let inset = control_padding + button_margin;
    let margin = Insets::all(button_margin);
    let mut probe = based(make(), margin, None, None);
    let room = Size::new(full.width - 2.0 * inset, full.height - 2.0 * inset);
    let width = natural(&mut probe, room).width;
    let y = if probe.enforce_integer_bounds() {
        round_even(inset)
    } else {
        inset
    };
    let mut control = AbsoluteLayout::new()
        .with_padding(Insets::all(control_padding))
        .add(pinned(
            based(make(), margin, None, None),
            (0.0, y),
            Size::new(width, full.height - inset * 2.0),
        ));

    let mut test = AbsoluteLayout::new()
        .with_padding(Insets::all(control_padding))
        .add(Box::new(based(
            make(),
            margin,
            None,
            Some(VAnchor::BOTTOM | VAnchor::TOP),
        )));
    layout_at(&mut test, first);
    // now change its size
    assert_same_picture(&mut control, &mut test, full);
}

#[test]
fn bottom_and_top_set_anchor_befor_add_child_test() {
    // C# runs CenterBothTest here (not BottomAndTopSetAnchorBeforAddChild),
    // ported as written.
    for (p, m) in center_cases() {
        center_both_test(p, m);
    }
}

fn center_cases() -> [(Insets, Insets); 5] {
    [
        (Insets::ZERO, Insets::ZERO),
        (Insets::ZERO, Insets::all(3.0)),
        (Insets::all(2.0), Insets::all(0.0)),
        (Insets::all(2.0), Insets::all(3.0)),
        (bd(1.1, 1.2, 1.3, 1.4), bd(2.1, 2.2, 2.3, 2.4)),
    ]
}

/// One corner case: the control button sits at `origin(button size)`, the
/// test button is anchored with `margin` inside a parent padded by `padding`.
fn corner_case(
    origin: impl Fn(Size) -> (f64, f64),
    padding: Insets,
    margin: Insets,
    h: HAnchor,
    v: VAnchor,
) {
    let container = Size::new(300.0, 200.0);
    let size = natural(&mut button("button"), container);
    let mut control = AbsoluteLayout::new().add(pinned(button("button"), origin(size), size));
    let mut test = AbsoluteLayout::new()
        .with_padding(padding)
        .add(Box::new(based(button("button"), margin, Some(h), Some(v))));
    assert_same_picture(&mut control, &mut test, container);
}

#[test]
fn anchor_left_bottom_tests() {
    // bottom left. this is the easiest as there should be nothing to it.
    corner_case(
        |_| (0.0, 0.0),
        Insets::ZERO,
        Insets::ZERO,
        HAnchor::LEFT,
        VAnchor::BOTTOM,
    );
    // bottom left with some crazy localBounds (no local origin in agg-gui:
    // see the file header; the offset bounds draw where un-offset ones do).
    corner_case(
        |_| (0.0, 0.0),
        Insets::ZERO,
        Insets::ZERO,
        HAnchor::LEFT,
        VAnchor::BOTTOM,
    );
    // bottom left, respect margin.
    corner_case(
        |_| (5.0, 5.0),
        Insets::ZERO,
        Insets::all(5.0),
        HAnchor::LEFT,
        VAnchor::BOTTOM,
    );
    // bottom left, respect margin and padding.
    corner_case(
        |_| (8.0, 8.0),
        Insets::all(3.0),
        Insets::all(5.0),
        HAnchor::LEFT,
        VAnchor::BOTTOM,
    );
    // bottom left, respect margin. This time we set the Margin after the
    // AnchorFlags (the builder has no order: the same widget).
    corner_case(
        |_| (5.0, 5.0),
        Insets::ZERO,
        Insets::all(5.0),
        HAnchor::LEFT,
        VAnchor::BOTTOM,
    );
}

#[test]
fn anchor_right_bottom_tests() {
    // bottom right.
    corner_case(
        |s| (300.0 - s.width, 0.0),
        Insets::ZERO,
        Insets::ZERO,
        HAnchor::RIGHT,
        VAnchor::BOTTOM,
    );
    // bottom right, respect margin.
    corner_case(
        |s| (300.0 - s.width - 5.0, 5.0),
        Insets::ZERO,
        Insets::all(5.0),
        HAnchor::RIGHT,
        VAnchor::BOTTOM,
    );
    // bottom right, respect margin. This time we set the Margin after the AnchorFlags.
    corner_case(
        |s| (300.0 - s.width - 5.0, 5.0),
        Insets::ZERO,
        Insets::all(5.0),
        HAnchor::RIGHT,
        VAnchor::BOTTOM,
    );
}

#[test]
fn anchor_right_top_tests() {
    corner_case(
        |s| (300.0 - s.width, 200.0 - s.height),
        Insets::ZERO,
        Insets::ZERO,
        HAnchor::RIGHT,
        VAnchor::TOP,
    );
}

#[test]
fn center_both_tests() {
    for (p, m) in center_cases() {
        center_both_test(p, m);
    }
}

/// C#'s hand-computed centre position of a `size` widget with `margin` in a
/// 200 x 300 parent padded by `padding`.
fn centred_origin(padding: Insets, margin: Insets, size: Size) -> (f64, f64) {
    let (cw, ch) = (200.0, 300.0);
    let control_center_x = padding.left + (cw - padding.left - padding.right) / 2.0;
    let button_x = control_center_x - (size.width + margin.left + margin.right) / 2.0 + margin.left;
    let control_center_y =
        padding.bottom + (ch - padding.bottom - padding.top) / 2.0 + margin.bottom;
    let button_y = control_center_y - (size.height + margin.bottom + margin.top) / 2.0;
    (button_x, button_y)
}

fn rounded(w: &dyn Widget, (x, y): (f64, f64)) -> (f64, f64) {
    if w.enforce_integer_bounds() {
        (round_even(x), round_even(y))
    } else {
        (x, y)
    }
}

fn center_both_test(control_padding: Insets, button_margin: Insets) {
    let container = Size::new(200.0, 300.0);
    let room = Size::new(
        container.width - control_padding.horizontal() - button_margin.horizontal(),
        container.height - control_padding.vertical() - button_margin.vertical(),
    );
    let probe = button("button1");
    let size = natural(&mut button("button1"), room);
    let origin = rounded(&probe, centred_origin(control_padding, button_margin, size));
    let mut control = AbsoluteLayout::new()
        .with_padding(control_padding)
        .add(pinned(
            based(button("button1"), button_margin, None, None),
            origin,
            size,
        ));
    let mut test = AbsoluteLayout::new()
        .with_padding(control_padding)
        .add(Box::new(based(
            button("button1"),
            button_margin,
            Some(HAnchor::CENTER),
            Some(VAnchor::CENTER),
        )));
    assert_same_picture(&mut control, &mut test, container);
}

#[test]
fn center_both_offset_bounds_tests() {
    for (p, m) in center_cases() {
        center_both_offset_bounds_test(p, m);
    }
}

fn center_both_offset_bounds_test(control_padding: Insets, button_margin: Insets) {
    let container = Size::new(200.0, 300.0);
    let size = Size::new(100.0, 100.0);
    let red = || {
        agg_gui::Container::new()
            .with_background(agg_gui::Color::rgba(1.0, 0.0, 0.0, 1.0))
            .with_min_size(size)
            .with_max_size(size)
    };
    let origin = rounded(&red(), centred_origin(control_padding, button_margin, size));
    let mut control = AbsoluteLayout::new()
        .with_padding(control_padding)
        .add(pinned(
            based(red(), button_margin, None, None),
            origin,
            size,
        ));
    let mut test = AbsoluteLayout::new()
        .with_padding(control_padding)
        .add(Box::new(based(
            red(),
            button_margin,
            Some(HAnchor::CENTER),
            Some(VAnchor::CENTER),
        )));
    assert_same_picture(&mut control, &mut test, container);
}

#[test]
fn h_center_h_right_and_v_center_v_top_tests() {
    for (p, m) in center_cases() {
        h_center_h_right_and_v_center_v_top_test(p, m);
    }
}

fn h_center_h_right_and_v_center_v_top_test(control_padding: Insets, button_margin: Insets) {
    let (cw, ch) = (200.0, 300.0);
    let (p, m) = (control_padding, button_margin);
    let origin = (
        p.left + m.left + (cw - (p.left + p.right)) / 2.0,
        p.bottom + m.bottom + (ch - (p.bottom + p.top)) / 2.0,
    );
    let size = Size::new(
        cw / 2.0 - (p.left + p.right) / 2.0 - (m.left + m.right),
        ch / 2.0 - (p.bottom + p.top) / 2.0 - (m.bottom + m.top),
    );
    let origin = rounded(&button("button1"), origin);
    let mut control = AbsoluteLayout::new()
        .with_padding(control_padding)
        .add(pinned(
            based(button("button1"), m, None, None),
            origin,
            size,
        ));
    let mut test = AbsoluteLayout::new()
        .with_padding(control_padding)
        .add(Box::new(based(
            button("button1"),
            m,
            Some(HAnchor::CENTER | HAnchor::RIGHT),
            Some(VAnchor::CENTER | VAnchor::TOP),
        )));
    assert_same_picture(&mut control, &mut test, Size::new(cw, ch));
}

/// C# `new GuiWidget(w, h)` as a parent: an `AbsoluteLayout` of that size
/// whose construction size is also its minimum (C#'s default
/// `SizeLimitsToSet.Minimum`).
fn gui_widget(w: f64, h: f64, h_anchor: HAnchor, v_anchor: VAnchor) -> AbsoluteLayout {
    AbsoluteLayout::new()
        .with_h_anchor(h_anchor)
        .with_v_anchor(v_anchor)
        .with_min_size(Size::new(w, h))
}

/// Size of `w` laid out on its own at `own` (its current size), as a C#
/// widget with no parent to stretch to.
fn alone(w: &mut dyn Widget, own: Size) -> Size {
    layout_at(w, own)
}

#[test]
fn parent_stretch_and_fit_to_children() {
    // Make sure normal nested layouts works as expected. First inner added then outer
    {
        let fixed_w = |l: AbsoluteLayout| l.with_max_size(Size::new(31.0, f64::MAX));
        let mut child_outer = fixed_w(gui_widget(
            31.0,
            32.0,
            HAnchor::ABSOLUTE,
            VAnchor::FIT | VAnchor::STRETCH,
        ));
        assert_eq!(
            alone(&mut child_outer, Size::new(31.0, 32.0)),
            Size::new(31.0, 32.0)
        );

        child_outer = child_outer.add(Box::new(sized(41.0, 42.0)));
        assert_eq!(
            alone(&mut child_outer, Size::new(31.0, 32.0)),
            Size::new(31.0, 42.0)
        );

        let mut parent = AbsoluteLayout::new().add(Box::new(child_outer));
        layout_at(&mut parent, Size::new(100.0, 200.0));
        assert_rect_eq(bounds_at(&parent, &[0]), Rect::new(0.0, 0.0, 31.0, 200.0));
    }

    // Make sure normal nested layouts works as expected. First outer than inner added
    {
        let child_outer = gui_widget(
            31.0,
            32.0,
            HAnchor::ABSOLUTE,
            VAnchor::FIT | VAnchor::STRETCH,
        )
        .with_max_size(Size::new(31.0, f64::MAX));
        let mut parent = AbsoluteLayout::new().add(Box::new(child_outer));
        layout_at(&mut parent, Size::new(100.0, 200.0));
        assert_rect_eq(bounds_at(&parent, &[0]), Rect::new(0.0, 0.0, 31.0, 200.0));

        parent.children_mut()[0]
            .children_mut()
            .push(Box::new(sized(41.0, 42.0)));
        layout_at(&mut parent, Size::new(100.0, 200.0));
        assert_rect_eq(bounds_at(&parent, &[0]), Rect::new(0.0, 0.0, 31.0, 200.0));
    }
}

/// Set the fixed height (or width) of the `SizedBox` at `path`.
fn resize_box(root: &mut dyn Widget, path: &[usize], width: Option<f64>, height: Option<f64>) {
    let mut w = root;
    for &i in path {
        w = w.children_mut()[i].as_mut();
    }
    let b = w
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<agg_gui::SizedBox>())
        .expect("a SizedBox");
    if width.is_some() {
        b.width = width;
    }
    if height.is_some() {
        b.height = height;
    }
}

#[test]
fn v_anchor_fit_ignores_children_with_v_anchor_stretch() {
    let child_a = AbsoluteLayout::new()
        .with_h_anchor(HAnchor::ABSOLUTE)
        .with_v_anchor(VAnchor::FIT)
        .with_min_size(Size::new(100.0, 0.0))
        .with_max_size(Size::new(100.0, f64::MAX))
        .add(Box::new(sized(100.0, 100.0)))
        .add(Box::new(
            sized(100.0, 100.0).with_v_anchor(VAnchor::STRETCH),
        ));
    let mut container = AbsoluteLayout::new().add(Box::new(child_a));
    let size = Size::new(200.0, 300.0);
    layout_at(&mut container, size);
    assert_eq!(bounds_at(&container, &[0]).height, 100.0);

    // expand B
    resize_box(&mut container, &[0, 0], None, Some(120.0));
    layout_at(&mut container, size);
    assert_eq!(bounds_at(&container, &[0]).height, 120.0);

    // compact B
    resize_box(&mut container, &[0, 0], None, Some(80.0));
    layout_at(&mut container, size);
    assert_eq!(bounds_at(&container, &[0]).height, 80.0);
}

#[test]
fn h_anchor_fit_ignores_children_with_h_anchor_stretch() {
    let child_a = AbsoluteLayout::new()
        .with_h_anchor(HAnchor::FIT)
        .with_v_anchor(VAnchor::ABSOLUTE)
        .with_min_size(Size::new(0.0, 100.0))
        .with_max_size(Size::new(f64::MAX, 100.0))
        .add(Box::new(sized(100.0, 100.0)))
        .add(Box::new(
            sized(100.0, 100.0).with_h_anchor(HAnchor::STRETCH),
        ));
    let mut container = AbsoluteLayout::new().add(Box::new(child_a));
    let size = Size::new(300.0, 200.0);
    layout_at(&mut container, size);
    assert_eq!(bounds_at(&container, &[0]).width, 100.0);

    // expand B
    resize_box(&mut container, &[0, 0], Some(120.0), None);
    layout_at(&mut container, size);
    assert_eq!(bounds_at(&container, &[0]).width, 120.0);

    // compact B
    resize_box(&mut container, &[0, 0], Some(80.0), None);
    layout_at(&mut container, size);
    assert_eq!(bounds_at(&container, &[0]).width, 80.0);
}
