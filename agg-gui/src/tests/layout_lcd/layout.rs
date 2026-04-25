use super::*;

use crate::{
    device_scale, resolve_fit_or_stretch, set_device_scale, HAnchor, Insets, Padding, Spacer,
    VAnchor, WidgetBase,
};

// --- Insets arithmetic ------------------------------------------------------

#[test]
fn test_insets_all() {
    let i = Insets::all(5.0);
    assert_eq!(i.left, 5.0);
    assert_eq!(i.right, 5.0);
    assert_eq!(i.top, 5.0);
    assert_eq!(i.bottom, 5.0);
}

#[test]
fn test_insets_symmetric() {
    let i = Insets::symmetric(10.0, 4.0);
    assert_eq!(i.horizontal(), 20.0);
    assert_eq!(i.vertical(), 8.0);
}

#[test]
fn test_insets_scale() {
    let i = Insets::all(3.0).scale(2.0);
    assert_eq!(i.left, 6.0);
    assert_eq!(i.top, 6.0);
}

// --- HAnchor / VAnchor bitflag algebra --------------------------------------

#[test]
fn test_hanchor_stretch_contains_left_and_right() {
    assert!(HAnchor::STRETCH.contains(HAnchor::LEFT));
    assert!(HAnchor::STRETCH.contains(HAnchor::RIGHT));
    assert!(HAnchor::STRETCH.is_stretch());
}

#[test]
fn test_hanchor_left_not_stretch() {
    assert!(!HAnchor::LEFT.is_stretch());
}

#[test]
fn test_hanchor_max_fit_or_stretch_contains_stretch() {
    // MAX_FIT_OR_STRETCH = 13 = 8 | 1 | 4 = FIT | STRETCH
    assert!(HAnchor::MAX_FIT_OR_STRETCH.contains(HAnchor::LEFT));
    assert!(HAnchor::MAX_FIT_OR_STRETCH.contains(HAnchor::RIGHT));
    assert!(HAnchor::MAX_FIT_OR_STRETCH.contains(HAnchor::FIT));
}

#[test]
fn test_vanchor_stretch() {
    assert!(VAnchor::STRETCH.is_stretch());
    assert!(VAnchor::STRETCH.contains(VAnchor::BOTTOM));
    assert!(VAnchor::STRETCH.contains(VAnchor::TOP));
}

// --- resolve_fit_or_stretch -------------------------------------------------

#[test]
fn test_resolve_max_fit_or_stretch_prefers_larger() {
    // natural (fit) is bigger → keep it.
    assert_eq!(resolve_fit_or_stretch(100.0, 60.0, true), 100.0);
    // stretch is bigger → use stretch.
    assert_eq!(resolve_fit_or_stretch(40.0, 80.0, true), 80.0);
}

#[test]
fn test_resolve_min_fit_or_stretch_prefers_smaller() {
    assert_eq!(resolve_fit_or_stretch(100.0, 60.0, false), 60.0);
    assert_eq!(resolve_fit_or_stretch(40.0, 80.0, false), 40.0);
}

// --- WidgetBase clamp_size --------------------------------------------------

#[test]
fn test_widget_base_clamp_size() {
    let mut base = WidgetBase::new();
    base.min_size = Size::new(50.0, 30.0);
    base.max_size = Size::new(200.0, 100.0);

    let clamped = base.clamp_size(Size::new(10.0, 150.0));
    assert_eq!(clamped.width, 50.0, "below min should clamp to min_w");
    assert_eq!(clamped.height, 100.0, "above max should clamp to max_h");
}

// --- DeviceScale scaled_margin ----------------------------------------------
//
// DPI scaling is now applied once at the `App` boundary (via a paint-ctx
// transform plus logical/physical conversion of viewport + input coords) —
// widgets work in logical units end-to-end.  `scaled_margin()` is therefore
// a pass-through and no longer multiplies by `device_scale`.  The old 2×
// expectation was invariant under the previous broken-by-design approach
// where only margins scaled and fonts didn't; it's been removed rather than
// updated, since a pass-through-returns-logical test is redundant with
// `scaled_margin`'s trivial definition.

#[test]
fn test_device_scale_default_is_one() {
    set_device_scale(1.0);
    assert_eq!(device_scale(), 1.0);
}

// --- Padding layout ---------------------------------------------------------

/// `Padding::new(Insets, child)` with asymmetric insets must place the child
/// at (left, bottom) and report the correct outer size.
#[test]
fn test_padding_asymmetric_layout() {
    // Use a Spacer as the child: it returns whatever size it's given.
    let child = Box::new(Spacer::new());
    let mut w = Padding::new(
        Insets::from_sides(10.0, 20.0, 5.0, 15.0), // left, right, top, bottom
        child,
    );

    let outer = w.layout(Size::new(100.0, 80.0));
    // Inner available: (100-10-20) × (80-5-15) = 70 × 60.
    // Spacer returns its full inner size, so content = 70 × 60.
    // Outer = 70+30 × 60+20 = 100 × 80.
    assert_eq!(
        outer.width, 100.0,
        "outer width should equal available.width"
    );
    assert_eq!(
        outer.height, 80.0,
        "outer height should equal available.height"
    );

    // Child bounds (in Padding-local Y-up coords): x=left=10, y=bottom=15.
    let cb = w.children()[0].bounds();
    assert_eq!(cb.x, 10.0, "child x should be left inset");
    assert_eq!(cb.y, 15.0, "child y should be bottom inset (Y-up)");
    assert_eq!(cb.width, 70.0, "child width = available.width - h_insets");
    assert_eq!(
        cb.height, 60.0,
        "child height = available.height - v_insets"
    );
}

/// `Padding::uniform` is a convenience alias.
#[test]
fn test_padding_uniform_alias() {
    let mut w = Padding::uniform(8.0, Box::new(Spacer::new()));
    let outer = w.layout(Size::new(50.0, 40.0));
    assert_eq!(outer.width, 50.0);
    assert_eq!(outer.height, 40.0);
    let cb = w.children()[0].bounds();
    assert_eq!(cb.x, 8.0);
    assert_eq!(cb.y, 8.0);
}

// --- SizedBox anchor-aware child placement ----------------------------------

/// Child with `h_anchor = RIGHT` should be placed at the right edge of the box.
#[test]
fn test_sized_box_child_right_anchor() {
    let child = Box::new(SizedBox::fixed(30.0, 20.0).with_h_anchor(HAnchor::RIGHT));
    let mut outer = SizedBox::new()
        .with_width(100.0)
        .with_height(50.0)
        .with_child(child);

    outer.layout(Size::new(100.0, 50.0));
    let cb = outer.children()[0].bounds();
    // Right-aligned 30-wide child inside 100-wide box: x = 100 - 30 = 70.
    assert_eq!(cb.x, 70.0, "right-anchor child x should be box_w - child_w");
    assert_eq!(cb.width, 30.0);
}

/// Child with `v_anchor = TOP` should be placed at the top (high Y) of the box.
#[test]
fn test_sized_box_child_top_anchor() {
    let child = Box::new(SizedBox::fixed(20.0, 15.0).with_v_anchor(VAnchor::TOP));
    let mut outer = SizedBox::new()
        .with_width(50.0)
        .with_height(60.0)
        .with_child(child);

    outer.layout(Size::new(50.0, 60.0));
    let cb = outer.children()[0].bounds();
    // Top-aligned 15-tall child inside 60-tall box: y = 60 - 15 = 45.
    assert_eq!(
        cb.y, 45.0,
        "top-anchor child y should be box_h - child_h (Y-up)"
    );
    assert_eq!(cb.height, 15.0);
}

/// Child with `h_anchor = CENTER` should be horizontally centered.
#[test]
fn test_sized_box_child_center_h_anchor() {
    let child = Box::new(SizedBox::fixed(20.0, 10.0).with_h_anchor(HAnchor::CENTER));
    let mut outer = SizedBox::new()
        .with_width(100.0)
        .with_height(50.0)
        .with_child(child);

    outer.layout(Size::new(100.0, 50.0));
    let cb = outer.children()[0].bounds();
    // Centered: x = (100 - 20) / 2 = 40.
    assert_eq!(
        cb.x, 40.0,
        "center-h child x should be (box_w - child_w) / 2"
    );
}

/// Child with `h_anchor = STRETCH` should fill the box width.
#[test]
fn test_sized_box_child_stretch() {
    let child = Box::new(SizedBox::fixed(20.0, 10.0).with_h_anchor(HAnchor::STRETCH));
    let mut outer = SizedBox::new()
        .with_width(100.0)
        .with_height(50.0)
        .with_child(child);

    outer.layout(Size::new(100.0, 50.0));
    let cb = outer.children()[0].bounds();
    assert_eq!(cb.x, 0.0, "stretched child should start at x=0");
    assert_eq!(cb.width, 100.0, "stretched child should fill box width");
}

// --- FlexColumn cross-axis anchoring ----------------------------------------

/// Children with LEFT / CENTER / RIGHT h_anchor must be placed correctly.
#[test]
fn test_flex_column_cross_axis_anchors() {
    let left_child = Box::new(SizedBox::fixed(30.0, 10.0).with_h_anchor(HAnchor::LEFT));
    let center_child = Box::new(SizedBox::fixed(30.0, 10.0).with_h_anchor(HAnchor::CENTER));
    let right_child = Box::new(SizedBox::fixed(30.0, 10.0).with_h_anchor(HAnchor::RIGHT));
    let stretch_child = Box::new(SizedBox::fixed(30.0, 10.0).with_h_anchor(HAnchor::STRETCH));

    let mut col = FlexColumn::new()
        .with_gap(0.0)
        .add(left_child)
        .add(center_child)
        .add(right_child)
        .add(stretch_child);

    col.layout(Size::new(100.0, 80.0));
    let children = col.children();

    // LEFT: x = 0
    assert_eq!(children[0].bounds().x, 0.0, "LEFT child x");
    // CENTER: x = (100 - 30) / 2 = 35
    let center_x = children[1].bounds().x;
    assert!(
        (center_x - 35.0).abs() < 0.5,
        "CENTER child x ≈ 35, got {center_x}"
    );
    // RIGHT: x = 100 - 30 = 70
    assert_eq!(children[2].bounds().x, 70.0, "RIGHT child x");
    // STRETCH: x = 0, width = 100
    assert_eq!(children[3].bounds().x, 0.0, "STRETCH child x");
    assert_eq!(children[3].bounds().width, 100.0, "STRETCH child width");
}

// --- FlexColumn main-axis margin spacing ------------------------------------

/// A child with bottom margin pushes the next sibling down.
#[test]
fn test_flex_column_child_margin_spacing() {
    set_device_scale(1.0);
    // Two 10-tall children; first has margin.bottom = 5, second has margin.top = 3.
    // Gap = 0.  Total spacing between them = 5 + 3 = 8.
    let top_child = Box::new(
        SizedBox::fixed(50.0, 10.0).with_margin(Insets::from_sides(0.0, 0.0, 0.0, 5.0)), // bottom=5
    );
    let bot_child = Box::new(
        SizedBox::fixed(50.0, 10.0).with_margin(Insets::from_sides(0.0, 0.0, 3.0, 0.0)), // top=3
    );

    let mut col = FlexColumn::new()
        .with_gap(0.0)
        .add(top_child)
        .add(bot_child);

    // Give enough height: (10+5) + (3+10) = 28 total main-axis.
    col.layout(Size::new(100.0, 100.0));
    let children = col.children();

    let top_bounds = children[0].bounds();
    let bot_bounds = children[1].bounds();

    // Top child is placed first (high Y in Y-up), bottom child below it.
    // Gap between bottom of top_child and top of bot_child should be 5+3=8.
    let gap_between = top_bounds.y - (bot_bounds.y + bot_bounds.height);
    assert!(
        (gap_between - 8.0).abs() < 0.5,
        "gap between children should equal 5+3=8 (additive margins), got {gap_between}"
    );
}

// --- FlexRow cross-axis VAnchor ---------------------------------------------

/// FlexRow children with BOTTOM / CENTER / TOP v_anchor are placed correctly.
#[test]
fn test_flex_row_cross_axis_anchors() {
    let bot_child = Box::new(SizedBox::fixed(20.0, 15.0).with_v_anchor(VAnchor::BOTTOM));
    let center_child = Box::new(SizedBox::fixed(20.0, 15.0).with_v_anchor(VAnchor::CENTER));
    let top_child = Box::new(SizedBox::fixed(20.0, 15.0).with_v_anchor(VAnchor::TOP));

    let mut row = FlexRow::new()
        .with_gap(0.0)
        .add(bot_child)
        .add(center_child)
        .add(top_child);

    row.layout(Size::new(200.0, 60.0));
    let children = row.children();

    // BOTTOM (Y-up): y = 0 (pad_b = 0, margin_b = 0)
    assert_eq!(children[0].bounds().y, 0.0, "BOTTOM child y");
    // CENTER: y = (60 - 15) / 2 = 22.5, rounded to integer → 23
    let cy = children[1].bounds().y;
    assert_eq!(cy, 23.0, "CENTER child y rounded to integer, got {cy}");
    // TOP: y = 60 - 15 = 45
    assert_eq!(children[2].bounds().y, 45.0, "TOP child y (Y-up)");
}

// --- min_size / max_size clamping in FlexColumn -----------------------------

#[test]
fn test_flex_column_respects_child_min_size() {
    // Child reports natural height 5, but min_size.height = 20.
    // The column must allocate at least 20 px.
    let tiny = Box::new(SizedBox::fixed(50.0, 5.0).with_min_size(Size::new(50.0, 20.0)));
    let mut col = FlexColumn::new().add(tiny);
    col.layout(Size::new(100.0, 200.0));
    assert_eq!(
        col.children()[0].bounds().height,
        20.0,
        "fixed child height must respect min_size"
    );
}

#[test]
fn test_flex_column_respects_child_max_size() {
    // Child is flex(1) in a 200-tall column, but max_size.height = 30.
    let big = Box::new(SizedBox::fixed(50.0, 50.0).with_max_size(Size::new(50.0, 30.0)));
    let mut col = FlexColumn::new().add_flex(big, 1.0);
    col.layout(Size::new(100.0, 200.0));
    assert_eq!(
        col.children()[0].bounds().height,
        30.0,
        "flex child height must respect max_size"
    );
}

// --- MIN_FIT_OR_STRETCH and MAX_FIT_OR_STRETCH in FlexColumn ----------------

/// MIN_FIT_OR_STRETCH: child smaller than slot → use natural width (fit wins).
#[test]
fn test_min_fit_or_stretch_uses_fit_when_smaller() {
    // Column is 100 wide, child natural width is 40 → min(40, 100) = 40.
    let child = Box::new(SizedBox::fixed(40.0, 10.0).with_h_anchor(HAnchor::MIN_FIT_OR_STRETCH));
    let mut col = FlexColumn::new().add(child);
    col.layout(Size::new(100.0, 50.0));
    assert_eq!(
        col.children()[0].bounds().width,
        40.0,
        "MIN_FIT_OR_STRETCH should use fit (40) when fit < stretch (100)"
    );
}

/// MAX_FIT_OR_STRETCH: child smaller than slot → use slot width (stretch wins).
#[test]
fn test_max_fit_or_stretch_uses_stretch_when_larger() {
    let child = Box::new(SizedBox::fixed(40.0, 10.0).with_h_anchor(HAnchor::MAX_FIT_OR_STRETCH));
    let mut col = FlexColumn::new().add(child);
    col.layout(Size::new(100.0, 50.0));
    assert_eq!(
        col.children()[0].bounds().width,
        100.0,
        "MAX_FIT_OR_STRETCH should use stretch (100) when stretch > fit (40)"
    );
}

// ---------------------------------------------------------------------------
// LCD subpixel placement — pixel-snap invariant
// ---------------------------------------------------------------------------
//
// LCD coverage masks encode a per-channel (R,G,B) phase offset at 1:1
// texel-to-pixel resolution.  If the mask is composited at a fractional
// destination position, the subpixel phasing shifts across pixel
// boundaries and text reads as blurry/fringed.  Both the CPU
// (`GfxCtx::draw_lcd_mask`) and GL (`demo-gl::draw_lcd_quad`) paths
// must snap the destination origin to integer pixels.  The CPU test
// below guards that invariant; the GL path follows the same contract
// but requires a live GL context to test directly.
