//! SVG gradient renderer tests.
//!
//! These tests exercise the public SVG rendering helpers while keeping the
//! implementation module below the project line-count limit.

use super::*;

#[test]
fn renders_linear_gradient_fill_via_rgba_target() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="4" height="2">
            <defs>
                <linearGradient id="g" gradientUnits="userSpaceOnUse"
                                x1="0" y1="0" x2="4" y2="0">
                    <stop offset="0" stop-color="#ff0000"/>
                    <stop offset="1" stop-color="#0000ff"/>
                </linearGradient>
            </defs>
            <rect width="4" height="2" fill="url(#g)"/>
        </svg>
    "##;

    let fb = render_svg_to_framebuffer(svg).expect("SVG should render");
    let left = ((fb.width() + 0) * 4) as usize;
    let right = ((fb.width() + 3) * 4) as usize;

    assert!(
        fb.pixels()[left] > fb.pixels()[left + 2],
        "left side should be more red than blue"
    );
    assert!(
        fb.pixels()[right + 2] > fb.pixels()[right],
        "right side should be more blue than red"
    );
}

#[test]
fn renders_linear_gradient_fill_via_lcd_target() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="4" height="2">
            <defs>
                <linearGradient id="g" gradientUnits="userSpaceOnUse"
                                x1="0" y1="0" x2="4" y2="0">
                    <stop offset="0" stop-color="#ff0000"/>
                    <stop offset="1" stop-color="#0000ff"/>
                </linearGradient>
            </defs>
            <rect width="4" height="2" fill="url(#g)"/>
        </svg>
    "##;

    let buffer = render_svg_to_lcd_buffer(svg).expect("SVG should render");
    let row = buffer.width() as usize;
    let left = (row + 0) * 3;
    let right = (row + 3) * 3;

    assert!(
        buffer.color_plane()[left] > buffer.color_plane()[left + 2],
        "left side should be more red than blue"
    );
    assert!(
        buffer.color_plane()[right + 2] > buffer.color_plane()[right],
        "right side should be more blue than red"
    );
}

#[test]
fn renders_linear_gradient_stroke_via_rgba_target() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="8" height="4">
            <defs>
                <linearGradient id="g" gradientUnits="userSpaceOnUse"
                                x1="0" y1="0" x2="8" y2="0">
                    <stop offset="0" stop-color="#ff0000"/>
                    <stop offset="1" stop-color="#0000ff"/>
                </linearGradient>
            </defs>
            <path d="M1 2 L7 2" fill="none" stroke="url(#g)" stroke-width="2"/>
        </svg>
    "##;

    let fb = render_svg_to_framebuffer(svg).expect("SVG should render");
    let left = ((2 * fb.width() + 1) * 4) as usize;
    let right = ((2 * fb.width() + 6) * 4) as usize;

    assert!(
        fb.pixels()[left] > fb.pixels()[left + 2],
        "left side of stroke should be more red than blue"
    );
    assert!(
        fb.pixels()[right + 2] > fb.pixels()[right],
        "right side of stroke should be more blue than red"
    );
}

#[test]
fn renders_linear_gradient_stroke_via_lcd_target() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="8" height="4">
            <defs>
                <linearGradient id="g" gradientUnits="userSpaceOnUse"
                                x1="0" y1="0" x2="8" y2="0">
                    <stop offset="0" stop-color="#ff0000"/>
                    <stop offset="1" stop-color="#0000ff"/>
                </linearGradient>
            </defs>
            <path d="M1 2 L7 2" fill="none" stroke="url(#g)" stroke-width="2"/>
        </svg>
    "##;

    let buffer = render_svg_to_lcd_buffer(svg).expect("SVG should render");
    let row = buffer.width() as usize;
    let left = (2 * row + 1) * 3;
    let right = (2 * row + 6) * 3;

    assert!(
        buffer.color_plane()[left] > buffer.color_plane()[left + 2],
        "left side of stroke should be more red than blue"
    );
    assert!(
        buffer.color_plane()[right + 2] > buffer.color_plane()[right],
        "right side of stroke should be more blue than red"
    );
}

#[test]
fn renders_radial_gradient_fill_via_rgba_target() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="5" height="5">
            <defs>
                <radialGradient id="g" gradientUnits="userSpaceOnUse"
                                cx="2.5" cy="2.5" r="2.5">
                    <stop offset="0" stop-color="#ff0000"/>
                    <stop offset="1" stop-color="#0000ff"/>
                </radialGradient>
            </defs>
            <rect width="5" height="5" fill="url(#g)"/>
        </svg>
    "##;

    let fb = render_svg_to_framebuffer(svg).expect("SVG should render");
    let center = ((2 * fb.width() + 2) * 4) as usize;
    let corner = ((4 * fb.width() + 0) * 4) as usize;

    assert!(
        fb.pixels()[center] > fb.pixels()[center + 2],
        "center should be more red than blue"
    );
    assert!(
        fb.pixels()[corner + 2] > fb.pixels()[corner],
        "corner should be more blue than red"
    );
}

#[test]
fn renders_radial_gradient_fill_via_lcd_target() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="5" height="5">
            <defs>
                <radialGradient id="g" gradientUnits="userSpaceOnUse"
                                cx="2.5" cy="2.5" r="2.5">
                    <stop offset="0" stop-color="#ff0000"/>
                    <stop offset="1" stop-color="#0000ff"/>
                </radialGradient>
            </defs>
            <rect width="5" height="5" fill="url(#g)"/>
        </svg>
    "##;

    let buffer = render_svg_to_lcd_buffer(svg).expect("SVG should render");
    let row = buffer.width() as usize;
    let center = (2 * row + 2) * 3;
    let corner = (4 * row + 0) * 3;

    assert!(
        buffer.color_plane()[center] > buffer.color_plane()[center + 2],
        "center should be more red than blue"
    );
    assert!(
        buffer.color_plane()[corner + 2] > buffer.color_plane()[corner],
        "corner should be more blue than red"
    );
}

#[test]
fn renders_pattern_fill_via_rgba_target() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="4" height="2">
            <defs>
                <pattern id="p" patternUnits="userSpaceOnUse"
                         x="0" y="0" width="2" height="2">
                    <rect x="0" y="0" width="1" height="2" fill="#00ff00"/>
                    <rect x="1" y="0" width="1" height="2" fill="#0000ff"/>
                </pattern>
            </defs>
            <rect width="4" height="2" fill="url(#p)"/>
        </svg>
    "##;

    let fb = render_svg_to_framebuffer(svg).expect("SVG should render");
    let green = ((fb.width() + 0) * 4) as usize;
    let blue = ((fb.width() + 1) * 4) as usize;
    let repeated_green = ((fb.width() + 2) * 4) as usize;

    assert!(
        fb.pixels()[green + 1] > fb.pixels()[green + 2],
        "first pattern column should be green"
    );
    assert!(
        fb.pixels()[blue + 2] > fb.pixels()[blue + 1],
        "second pattern column should be blue"
    );
    assert!(
        fb.pixels()[repeated_green + 1] > fb.pixels()[repeated_green + 2],
        "pattern should repeat horizontally"
    );
}

#[test]
fn renders_pattern_fill_via_lcd_target() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="4" height="2">
            <defs>
                <pattern id="p" patternUnits="userSpaceOnUse"
                         x="0" y="0" width="2" height="2">
                    <rect x="0" y="0" width="1" height="2" fill="#00ff00"/>
                    <rect x="1" y="0" width="1" height="2" fill="#0000ff"/>
                </pattern>
            </defs>
            <rect width="4" height="2" fill="url(#p)"/>
        </svg>
    "##;

    let buffer = render_svg_to_lcd_buffer(svg).expect("SVG should render");
    let row = buffer.width() as usize;
    let green = (row + 0) * 3;
    let blue = (row + 1) * 3;
    let repeated_green = (row + 2) * 3;

    assert!(
        buffer.color_plane()[green + 1] > buffer.color_plane()[green + 2],
        "first pattern column should be green"
    );
    assert!(
        buffer.color_plane()[blue + 2] > buffer.color_plane()[blue + 1],
        "second pattern column should be blue"
    );
    assert!(
        buffer.color_plane()[repeated_green + 1] > buffer.color_plane()[repeated_green + 2],
        "pattern should repeat horizontally"
    );
}
