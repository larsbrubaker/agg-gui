//! agg-sharp's `SvgContextPaintTests` (context-fill and context-stroke, as usvg
//! resolves them: in a use's content, the use's own fill/stroke; in a marker's
//! content, the fill/stroke of the path it marks), ported 1:1. Renders are
//! 100x100 of a 100x100 viewBox.

use super::common::{render, Color};

#[test]
fn a_uses_fill_and_stroke_are_its_contents_context_paint() {
    let used = render("<defs><rect id=\"r\" width=\"50\" height=\"50\" fill=\"context-stroke\" stroke=\"context-fill\" stroke-width=\"10\"/></defs><use xlink:href=\"#r\" x=\"20\" y=\"20\" fill=\"#0000ff\" stroke=\"#008000\"/>");
    assert_eq!(used.at(45, 45), Color::new(0, 128, 0, 255));
    assert_eq!(used.at(17, 45), Color::new(0, 0, 255, 255));

    // A use whose own fill is context-fill passes on the context of the use around it.
    let nested = render("<defs><rect id=\"r\" width=\"50\" height=\"50\" fill=\"context-fill\"/></defs><use id=\"u\" xlink:href=\"#r\" fill=\"context-fill\"/><use xlink:href=\"#u\" x=\"20\" fill=\"#0000ff\"/>");
    assert_eq!(nested.at(45, 25), Color::new(0, 0, 255, 255));
}

#[test]
fn context_paint_without_a_context_element_is_none() {
    let image = render(
        "<rect width=\"50\" height=\"50\" fill=\"context-fill\" stroke=\"context-stroke\"/>",
    );
    assert_eq!(image.at(25, 25).alpha, 0);
}

#[test]
fn a_markers_context_paint_is_the_paths_fill_and_stroke() {
    let image = render("<marker id=\"m\" refX=\"5\" refY=\"5\" markerWidth=\"10\" markerHeight=\"10\" markerUnits=\"userSpaceOnUse\"><rect width=\"10\" height=\"10\" fill=\"context-stroke\"/></marker><path d=\"M 20 50 L 80 50\" fill=\"none\" stroke=\"#008000\" marker-start=\"url(#m)\"/>");
    assert_eq!(image.at(18, 46), Color::new(0, 128, 0, 255));
}

#[test]
fn a_context_gradient_spans_the_context_elements_box() {
    // The gradient runs across the use's box (0..100), not each rect's own half.
    let image = render("<linearGradient id=\"lg\"><stop offset=\"0\" stop-color=\"#ff0000\"/><stop offset=\"1\" stop-color=\"#0000ff\"/></linearGradient><defs><g id=\"g\"><rect width=\"50\" height=\"100\" fill=\"context-fill\"/><rect x=\"50\" width=\"50\" height=\"100\" fill=\"context-fill\"/></g></defs><use xlink:href=\"#g\" fill=\"url(#lg)\"/>");
    let near_middle = image.at(45, 50);
    assert!(near_middle.red > near_middle.blue, "{near_middle:?}");
}
