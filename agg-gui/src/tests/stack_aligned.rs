//! Stack `add_aligned` placement invariants.
//!
//! An aligned overlay child (e.g. a floating dialog / search panel) must:
//!   1. stay centred when its `max_size` caps its width on a wide viewport,
//!   2. keep its margins — never go full-bleed — when the viewport is
//!      narrower than `max_size` (the "dialog cut off at the screen edge"
//!      bug reported from Instant-Astronomer's search overlay), and
//!   3. treat margins as logical units end-to-end: DPI is applied once at
//!      the `App` paint boundary, so layout must NOT multiply margins by
//!      `device_scale` again (see `layout_props::WidgetBase::scaled_margin`).

use crate::{
    set_device_scale, FlexColumn, FlexRow, HAnchor, Insets, Size, SizedBox, Spacer, Stack, VAnchor,
    Widget,
};

/// Build an overlay positioner mirroring a floating dialog: fills available
/// width up to `max_w`, anchored top-centre, 10 px side margins, 6 px top.
fn dialog_positioner(max_w: f64) -> Box<dyn Widget> {
    Box::new(
        SizedBox::new()
            .with_height(44.0)
            .with_max_size(Size::new(max_w, f64::INFINITY))
            .with_margin(Insets {
                top: 6.0,
                bottom: 0.0,
                left: 10.0,
                right: 10.0,
            })
            .with_h_anchor(HAnchor::CENTER)
            .with_v_anchor(VAnchor::TOP),
    )
}

fn layout_stack_with_dialog(avail: Size) -> crate::Rect {
    let mut stack = Stack::new().add_aligned(dialog_positioner(460.0));
    stack.layout(avail);
    stack.children()[0].bounds()
}

/// Wide viewport: the dialog caps at `max_size.width` and is centred.
#[test]
fn aligned_center_child_is_centered_when_max_size_caps_width() {
    set_device_scale(1.0);
    let b = layout_stack_with_dialog(Size::new(1200.0, 800.0));
    assert_eq!(b.width, 460.0, "width should cap at max_size");
    assert_eq!(b.x, 370.0, "should be exactly centred: (1200-460)/2");
    assert_eq!(b.y, 800.0 - 6.0 - 44.0, "TOP anchor respects top margin");
}

/// Narrow viewport (phone): the dialog must keep its side margins instead
/// of going full-bleed against the screen edges.
#[test]
fn aligned_center_child_keeps_margins_on_narrow_viewport() {
    set_device_scale(1.0);
    let b = layout_stack_with_dialog(Size::new(380.0, 700.0));
    assert_eq!(
        b.width,
        380.0 - 20.0,
        "width should shrink to available minus both side margins"
    );
    assert_eq!(b.x, 10.0, "left edge should sit at the left margin");
    assert_eq!(
        b.x + b.width,
        370.0,
        "right edge should sit at available - right margin"
    );
}

/// Margins are logical units; a HiDPI device scale must not inflate them
/// during layout (DPI is applied once at the App paint transform).
#[test]
fn aligned_child_margins_are_not_scaled_by_device_scale() {
    set_device_scale(2.0);
    let b = layout_stack_with_dialog(Size::new(380.0, 700.0));
    set_device_scale(1.0);
    assert_eq!(b.width, 360.0, "margins must stay 10 logical px at 2x DPI");
    assert_eq!(b.x, 10.0, "left margin must stay 10 logical px at 2x DPI");
}

/// An aligned child taller than the viewport must clamp to the available
/// height minus vertical margins rather than hang off-screen.
#[test]
fn aligned_child_height_clamps_to_available_minus_margins() {
    set_device_scale(1.0);
    let mut stack = Stack::new().add_aligned(Box::new(
        SizedBox::new()
            .with_height(900.0)
            .with_margin(Insets {
                top: 6.0,
                bottom: 4.0,
                left: 0.0,
                right: 0.0,
            })
            .with_h_anchor(HAnchor::CENTER)
            .with_v_anchor(VAnchor::TOP),
    ));
    stack.layout(Size::new(400.0, 300.0));
    let b = stack.children()[0].bounds();
    assert_eq!(b.height, 300.0 - 10.0, "height caps at available - margins");
    assert_eq!(b.y, 4.0, "bottom lands on the bottom margin");
}

// ---------------------------------------------------------------------------
// Margin double-scaling regression coverage for the flex/container layouts.
// ---------------------------------------------------------------------------

/// FlexColumn: a child's left margin stays logical under a HiDPI scale.
#[test]
fn flex_column_child_margin_not_scaled_by_device_scale() {
    set_device_scale(2.0);
    let mut col = FlexColumn::new().add(Box::new(
        SizedBox::fixed(50.0, 20.0)
            .with_h_anchor(HAnchor::LEFT)
            .with_margin(Insets {
                left: 10.0,
                right: 0.0,
                top: 0.0,
                bottom: 0.0,
            }),
    ));
    col.layout(Size::new(200.0, 100.0));
    let b = col.children()[0].bounds();
    set_device_scale(1.0);
    assert_eq!(b.x, 10.0, "left margin must stay 10 logical px at 2x DPI");
}

/// FlexRow: main-axis margins stay logical under a HiDPI scale.
#[test]
fn flex_row_child_margin_not_scaled_by_device_scale() {
    set_device_scale(2.0);
    let mut row = FlexRow::new()
        .add(Box::new(SizedBox::fixed(50.0, 20.0).with_margin(Insets {
            left: 10.0,
            right: 0.0,
            top: 0.0,
            bottom: 0.0,
        })))
        .add(Box::new(Spacer::new()));
    row.layout(Size::new(200.0, 100.0));
    let b = row.children()[0].bounds();
    set_device_scale(1.0);
    assert_eq!(b.x, 10.0, "left margin must stay 10 logical px at 2x DPI");
}

/// Container: child margins stay logical under a HiDPI scale.
#[test]
fn container_child_margin_not_scaled_by_device_scale() {
    set_device_scale(2.0);
    let mut c =
        crate::Container::new().add(Box::new(SizedBox::fixed(50.0, 20.0).with_margin(Insets {
            left: 10.0,
            right: 0.0,
            top: 0.0,
            bottom: 0.0,
        })));
    c.layout(Size::new(200.0, 100.0));
    let b = c.children()[0].bounds();
    set_device_scale(1.0);
    assert_eq!(b.x, 10.0, "left margin must stay 10 logical px at 2x DPI");
}
