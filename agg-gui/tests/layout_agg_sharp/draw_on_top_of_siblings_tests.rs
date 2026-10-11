//! Port of agg-sharp `DrawOnTopOfSiblingsTests.cs`: a child flagged
//! `draw_on_top_of_siblings` (C# `GuiWidget.DrawOnTopOfSiblings`, which
//! MatterCAD's selected ChromeTab sets) paints over its siblings without
//! moving in the child list.

use agg_gui::widgets::AbsoluteLayout;
use agg_gui::{Color, Container, Size, Widget};

use super::support::{layout_at, pixel, render, BLUE, RED};

fn rect(color: Color, x: f64) -> Container {
    Container::new()
        .with_background(color)
        .with_min_size(Size::new(60.0, 100.0))
        .with_max_size(Size::new(60.0, 100.0))
        .with_origin(x, 0.0)
}

fn render_overlap_pixel(parent: &mut AbsoluteLayout) -> [u8; 4] {
    layout_at(parent, Size::new(100.0, 100.0));
    let fb = render(parent, 100, 100);
    pixel(&fb, 50, 50)
}

#[test]
fn flagged_child_draws_over_siblings_without_moving_in_child_list() {
    // two children that overlap between x 40 and x 60
    let mut parent = AbsoluteLayout::new()
        .add(Box::new(
            rect(Color::rgba(1.0, 0.0, 0.0, 1.0), 0.0).with_name("childA"),
        ))
        .add(Box::new(
            rect(Color::rgba(0.0, 0.0, 1.0, 1.0), 40.0).with_name("childB"),
        ));

    // baseline, without the flag the later child in the list paints over the earlier one
    assert_eq!(render_overlap_pixel(&mut parent), BLUE);

    parent.children_mut()[0]
        .widget_base_mut()
        .expect("Container embeds a WidgetBase")
        .draw_on_top_of_siblings = true;

    assert_eq!(render_overlap_pixel(&mut parent), RED);

    // the flag must not reorder the children, layout (flow slots) depends on list order
    assert_eq!(parent.children()[0].id(), Some("childA"));
    assert_eq!(parent.children()[1].id(), Some("childB"));
}
