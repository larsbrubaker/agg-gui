//! agg-sharp's `Tests/Agg.Tests/Agg.UI/ToolTipPlacementTests.cs`: under the
//! [`TooltipPlacement::Widget`] policy a tooltip is centred on the widget the
//! mouse is over, slides back under the cursor on a wide widget, hangs
//! [`CURSOR_CLEARANCE`] below the pointer, and is clamped [`EDGE_INSET`] in
//! from the window edges, the left edge winning.
//!
//! The C# test swaps `ToolTipManager.CreateToolTip` for a fixed-size body and
//! calls the private `DoShowToolTip` by reflection. Here a content factory
//! ([`set_content_factory`]) supplies the fixed body, and the tip is shown
//! through the real path: `App::on_mouse_move`, the App's tooltip pass, and
//! the virtual clock past the hover delay. [`show_tool_tip`] is shared with
//! `tool_tip_device_scale.rs`.

use crate::geometry::Point;
use crate::layout_props::WidgetBase;
use crate::widgets::tooltip::{
    controller, reset_tooltip_test_state, set_content_factory, set_tooltip_placement,
    tooltip_timings, TooltipPlacement,
};
use crate::{App, DrawCtx, Event, EventResult, Rect, Size, Widget};

const TOOL_TIP_WIDTH: f64 = 120.0;
const TOOL_TIP_HEIGHT: f64 = 40.0;
const WINDOW_WIDTH: f64 = 800.0;
const WINDOW_HEIGHT: f64 = 600.0;

/// The screen edge gap ToolTipManager keeps, in design pixels.
const EDGE_INSET: f64 = 3.0;

/// The gap ToolTipManager leaves above itself so the tooltip clears the cursor.
const CURSOR_CLEARANCE: f64 = 23.0;

/// A plain widget; the root lays out its children at their preset bounds.
pub(super) struct Fixed {
    pub bounds: Rect,
    pub children: Vec<Box<dyn Widget>>,
    pub size: Option<Size>,
    pub base: WidgetBase,
}

impl Widget for Fixed {
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
        match self.size {
            // A tooltip body: its fixed size whatever it is offered.
            Some(size) => size,
            // The window: fills the viewport, children keep their bounds.
            None => {
                self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
                available
            }
        }
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
}

/// Restores the tooltip subsystem's process-wide state on drop, as the C#
/// test's `finally` restores `DeviceScale` and `CreateToolTip`.
pub(super) struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        set_content_factory(None);
        set_tooltip_placement(TooltipPlacement::Pointer);
        reset_tooltip_test_state();
        controller::reset();
        crate::device_scale::set_device_scale(1.0);
        crate::ux_scale::set_ux_scale(1.0);
    }
}

/// Shows a fixed size tooltip for a widget of the given bounds (window
/// logical coordinates, y up) at `device_scale`, and returns where it landed,
/// in the same coordinates.
pub(super) fn show_tool_tip(
    hovered_bounds: Rect,
    mouse_position: Point,
    tool_tip_width: f64,
    device_scale: f64,
) -> Rect {
    show_tool_tip_with(
        TooltipPlacement::Widget,
        hovered_bounds,
        mouse_position,
        tool_tip_width,
        device_scale,
    )
}

/// [`show_tool_tip`] under `placement`.
fn show_tool_tip_with(
    placement: TooltipPlacement,
    hovered_bounds: Rect,
    mouse_position: Point,
    tool_tip_width: f64,
    device_scale: f64,
) -> Rect {
    let _restore = Restore;
    reset_tooltip_test_state();
    controller::reset();
    crate::clock::start_virtual();
    crate::device_scale::set_device_scale(device_scale);
    crate::ux_scale::set_ux_scale(1.0);
    set_tooltip_placement(placement);
    set_content_factory(Some(Box::new(move |_text: &str| -> Box<dyn Widget> {
        Box::new(Fixed {
            bounds: Rect::default(),
            children: Vec::new(),
            size: Some(Size::new(tool_tip_width, TOOL_TIP_HEIGHT)),
            base: WidgetBase::new(),
        })
    })));

    let mut hovered: Box<dyn Widget> = Box::new(Fixed {
        bounds: hovered_bounds,
        children: Vec::new(),
        size: None,
        base: WidgetBase::new(),
    });
    hovered.set_tooltip_text(Some("some help".to_string()));
    let window = Fixed {
        bounds: Rect::default(),
        children: vec![hovered],
        size: None,
        base: WidgetBase::new(),
    };
    let mut app = App::new(Box::new(window));
    // The host hands the App device pixels; the tree lays out in logical units.
    let scale = crate::ux_scale::effective_scale();
    app.layout(Size::new(WINDOW_WIDTH * scale, WINDOW_HEIGHT * scale));
    // The fill-the-viewport layout must not have moved the hovered widget.
    app.root_mut().children_mut()[0].set_bounds(hovered_bounds);

    app.on_mouse_move(
        mouse_position.x * scale,
        (WINDOW_HEIGHT - mouse_position.y) * scale,
    );
    app.update_tooltips_for_test();
    crate::clock::advance(tooltip_timings().initial_delay);
    app.update_tooltips_for_test();
    controller::visible_rect()
        .expect("no tooltip was shown - the test setup, not the placement, is wrong")
}

fn show(hovered: Rect, mouse_position: Point) -> Rect {
    show_tool_tip(hovered, mouse_position, TOOL_TIP_WIDTH, 1.0)
}

/// `RectangleDouble(left, bottom, right, top)` as a `Rect`.
fn ltrb(left: f64, bottom: f64, right: f64, top: f64) -> Rect {
    Rect::new(left, bottom, right - left, top - bottom)
}

fn x_center(r: Rect) -> f64 {
    r.x + r.width / 2.0
}

fn top(r: Rect) -> f64 {
    r.y + r.height
}

fn right(r: Rect) -> f64 {
    r.x + r.width
}

#[test]
fn tool_tip_is_centered_on_the_hovered_widget() {
    // Mouse deliberately off center inside the widget - placement must follow the widget, not the mouse
    let hovered = ltrb(200.0, 350.0, 240.0, 380.0);
    let tool_tip = show(hovered, Point::new(205.0, 355.0));

    assert!(
        (x_center(tool_tip) - x_center(hovered)).abs() <= 1.0,
        "the tooltip describes the hovered widget, so it is centered on it: {tool_tip:?}"
    );
}

#[test]
fn centering_does_not_disturb_vertical_placement() {
    let hovered = ltrb(200.0, 350.0, 240.0, 380.0);
    let mouse_position = Point::new(205.0, 355.0);
    let tool_tip = show(hovered, mouse_position);

    assert!(
        (mouse_position.y - top(tool_tip) - CURSOR_CLEARANCE).abs() <= 0.001,
        "the tooltip still hangs its fixed clearance below the cursor: {tool_tip:?}"
    );
}

#[test]
fn tool_tip_is_clamped_to_the_right_edge() {
    // A widget hard against the right edge cannot have a centered tooltip - it has to be pulled back on screen
    let hovered = ltrb(WINDOW_WIDTH - 40.0, 350.0, WINDOW_WIDTH, 380.0);
    let tool_tip = show(hovered, Point::new(WINDOW_WIDTH - 20.0, 355.0));

    assert!(
        (right(tool_tip) - (WINDOW_WIDTH - EDGE_INSET)).abs() <= 1.0,
        "{tool_tip:?}"
    );
}

#[test]
fn tool_tip_is_clamped_to_the_left_edge() {
    let hovered = ltrb(0.0, 350.0, 40.0, 380.0);
    let tool_tip = show(hovered, Point::new(20.0, 355.0));

    assert!((tool_tip.x - EDGE_INSET).abs() <= 1.0, "{tool_tip:?}");
}

#[test]
fn tool_tip_wider_than_the_window_sits_flush_left() {
    // Centered on a mid window widget, a tooltip this wide overhangs both edges, so both clamps fire and
    // they disagree - the left one has to win or the text starts off screen. This is the case that pins
    // their order; a tooltip that fits can satisfy either ordering.
    let hovered = ltrb(380.0, 350.0, 420.0, 380.0);
    let tool_tip = show_tool_tip(hovered, Point::new(400.0, 355.0), WINDOW_WIDTH + 100.0, 1.0);

    assert!(
        (tool_tip.x - EDGE_INSET).abs() <= 1.0,
        "text is read from the left, so an oversized tooltip overflows to the right: {tool_tip:?}"
    );
}

#[test]
fn tool_tip_stays_under_the_cursor_on_a_wide_widget() {
    // A full width row's center is nowhere near the cursor, so centering alone would leave the tooltip
    // pointing at nothing
    let hovered = ltrb(0.0, 350.0, WINDOW_WIDTH, 380.0);
    let mouse_position = Point::new(100.0, 355.0);
    let tool_tip = show(hovered, mouse_position);

    assert!(tool_tip.x <= mouse_position.x, "{tool_tip:?}");
    assert!(right(tool_tip) >= mouse_position.x, "{tool_tip:?}");
    assert!(
        x_center(tool_tip) < x_center(hovered),
        "the tooltip gave up centering to follow the cursor: {tool_tip:?}"
    );
}

/// The default policy is agg-gui's own pointer placement: the same hover
/// lands the tip below-right of the pointer, not centred on the widget, so
/// AtomArtist and the demos are unchanged unless a host opts in.
#[test]
fn rust_only_default_placement_stays_at_the_pointer() {
    assert_eq!(
        crate::widgets::tooltip_placement(),
        TooltipPlacement::Pointer
    );
    let hovered = ltrb(200.0, 350.0, 240.0, 380.0);
    let mouse_position = Point::new(205.0, 355.0);
    let tool_tip = show_tool_tip_with(
        TooltipPlacement::Pointer,
        hovered,
        mouse_position,
        TOOL_TIP_WIDTH,
        1.0,
    );
    // agg-gui's place_panel: left edge at the pointer, top 4 + 10 below it.
    assert_eq!(tool_tip.x, mouse_position.x);
    assert_eq!(top(tool_tip), mouse_position.y - 14.0);
}
