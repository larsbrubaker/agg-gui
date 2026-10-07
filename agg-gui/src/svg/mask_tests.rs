//! SVG `<mask>` regression tests.
//!
//! MatterCAD's `make_component.svg` / `edit_component.svg` icons clear a
//! circle out of a box with a luminance mask (white rect, black circle).
//! These tests pin that a masked-out region renders transparent, that the
//! unmasked region keeps its paint, and the alpha / linked / opacity /
//! device-scale variants of the same path through `svg/mask.rs`.

use super::*;

/// RGBA of the pixel at SVG coordinates `(x, y)` (Y-down) in a Y-up framebuffer.
fn px(fb: &Framebuffer, x: u32, y: u32) -> [u8; 4] {
    let row = fb.height() - 1 - y;
    let i = ((row * fb.width() + x) * 4) as usize;
    let p = &fb.pixels()[i..i + 4];
    [p[0], p[1], p[2], p[3]]
}

/// MatterCAD `StaticData/Icons/make_component.svg`, verbatim.
const MAKE_COMPONENT: &[u8] = br##"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <mask id="clear" maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="100">
    <rect width="100" height="100" fill="#fff"/>
    <circle cx="76" cy="76" r="24" fill="#000"/>
  </mask>
  <g mask="url(#clear)">
    <path d="M44,20L78,36V72L44,88L10,72V36Z" fill="#000" fill-opacity=".55"/>
    <path d="M44,20L78,36L44,52L10,36Z" fill="#000"/>
  </g>
  <path d="M71,58H81V71H94V81H81V94H71V81H58V71H71Z" fill="#000"/>
</svg>"##;

#[test]
fn luminance_mask_clears_black_region_to_transparent() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
            <mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="10" height="10">
                <rect width="10" height="10" fill="#fff"/>
                <rect width="5" height="10" fill="#000"/>
            </mask>
            <g mask="url(#m)">
                <rect width="10" height="10" fill="#ff0000"/>
            </g>
        </svg>
    "##;
    let fb = render_svg_to_framebuffer(svg).expect("masked SVG should render");
    assert_eq!(px(&fb, 2, 5), [0, 0, 0, 0], "black mask area is cleared");
    assert_eq!(px(&fb, 7, 5), [255, 0, 0, 255], "white mask area paints");
}

#[test]
fn make_component_icon_clears_its_corner_circle() {
    let fb = render_svg_to_framebuffer(MAKE_COMPONENT).expect("icon should render");
    assert_eq!((fb.width(), fb.height()), (100, 100));
    // Inside the box and inside the cleared circle, away from the plus.
    assert_eq!(px(&fb, 63, 63), [0, 0, 0, 0]);
    // Inside the box side face, far from the circle: 55% black.
    assert_eq!(px(&fb, 30, 60), [0, 0, 0, 140]);
    // The unmasked plus draws on top of the clearing.
    assert_eq!(px(&fb, 76, 76), [0, 0, 0, 255]);
}

#[test]
fn make_component_icon_mask_is_crisp_at_device_scale() {
    let fb = render_svg_to_framebuffer_at_size(MAKE_COMPONENT, 200, 200)
        .expect("icon should render at 2x");
    assert_eq!(px(&fb, 126, 126), [0, 0, 0, 0]);
    assert_eq!(px(&fb, 60, 120), [0, 0, 0, 140]);
    // The circle edge is anti-aliased at physical resolution: one physical
    // pixel straddling the edge is partially covered, not stair-stepped
    // from a 1x mask.  At y = 140 the circle (centre 152, r 48) crosses x = 105.5.
    let edge = px(&fb, 105, 140)[3];
    assert!(edge > 0 && edge < 140, "edge pixel alpha {edge}");
}

#[test]
fn alpha_mask_uses_mask_alpha_not_luminance() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
            <mask id="m" mask-type="alpha" maskUnits="userSpaceOnUse" x="0" y="0" width="10" height="10">
                <rect width="5" height="10" fill="#000"/>
            </mask>
            <g mask="url(#m)">
                <rect width="10" height="10" fill="#0000ff"/>
            </g>
        </svg>
    "##;
    let fb = render_svg_to_framebuffer(svg).expect("alpha-masked SVG should render");
    assert_eq!(
        px(&fb, 2, 5),
        [0, 0, 255, 255],
        "opaque black keeps content"
    );
    assert_eq!(px(&fb, 7, 5), [0, 0, 0, 0], "no mask paint clears content");
}

#[test]
fn mask_combines_with_group_opacity() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
            <mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="10" height="10">
                <rect width="10" height="10" fill="#fff"/>
            </mask>
            <g mask="url(#m)" opacity="0.5">
                <rect width="10" height="10" fill="#00ff00"/>
            </g>
        </svg>
    "##;
    let fb = render_svg_to_framebuffer(svg).expect("SVG should render");
    assert_eq!(px(&fb, 5, 5), [0, 128, 0, 128]);
}

#[test]
fn mask_content_outside_mask_rect_is_cleared() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
            <mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="5" height="10">
                <rect width="10" height="10" fill="#fff"/>
            </mask>
            <g mask="url(#m)">
                <rect width="10" height="10" fill="#ff0000"/>
            </g>
        </svg>
    "##;
    let fb = render_svg_to_framebuffer(svg).expect("SVG should render");
    assert_eq!(px(&fb, 2, 5), [255, 0, 0, 255]);
    assert_eq!(px(&fb, 7, 5), [0, 0, 0, 0]);
}

#[test]
fn linked_mask_multiplies_in() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
            <mask id="top" maskUnits="userSpaceOnUse" x="0" y="0" width="10" height="10">
                <rect width="10" height="5" fill="#fff"/>
            </mask>
            <mask id="left" mask="url(#top)" maskUnits="userSpaceOnUse" x="0" y="0" width="10" height="10">
                <rect width="5" height="10" fill="#fff"/>
            </mask>
            <g mask="url(#left)">
                <rect width="10" height="10" fill="#ff0000"/>
            </g>
        </svg>
    "##;
    let fb = render_svg_to_framebuffer(svg).expect("SVG should render");
    assert_eq!(px(&fb, 2, 2), [255, 0, 0, 255], "inside both masks");
    assert_eq!(px(&fb, 7, 2), [0, 0, 0, 0], "outside the element's mask");
    assert_eq!(px(&fb, 2, 7), [0, 0, 0, 0], "outside the linked mask");
}

#[test]
fn mask_on_transformed_group_uses_group_user_space() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="10">
            <mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="10" height="10">
                <rect width="5" height="10" fill="#fff"/>
            </mask>
            <g transform="translate(10 0)" mask="url(#m)">
                <rect width="10" height="10" fill="#ff0000"/>
            </g>
        </svg>
    "##;
    let fb = render_svg_to_framebuffer(svg).expect("SVG should render");
    assert_eq!(px(&fb, 12, 5), [255, 0, 0, 255]);
    assert_eq!(px(&fb, 17, 5), [0, 0, 0, 0]);
    assert_eq!(px(&fb, 3, 5), [0, 0, 0, 0]);
}
