//! SVG opacity compositing tests.
//!
//! SVG group opacity is applied after rendering the group into an isolated
//! buffer.  These tests catch the darker overlap produced by incorrectly
//! multiplying opacity into each child before compositing.

use super::*;

#[test]
fn group_opacity_is_applied_after_children_are_composited() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="3" height="3">
            <g opacity="0.5">
                <rect width="3" height="3" fill="#ff0000"/>
                <rect width="3" height="3" fill="#0000ff"/>
            </g>
        </svg>
    "##;

    let fb = render_svg_to_framebuffer(svg).expect("SVG should render");
    let center = ((1 * fb.width() + 1) * 4) as usize;

    assert_eq!(
        &fb.pixels()[center..center + 4],
        &[0, 0, 128, 128],
        "top blue child should be composited opaquely inside the group, then the whole group should be faded once"
    );
}
