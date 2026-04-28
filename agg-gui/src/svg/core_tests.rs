//! Core SVG renderer regression tests.
//!
//! These cover the public render helpers and baseline path/image/stroke behavior
//! that does not belong to a more specific SVG feature test module.

use super::*;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use base64::Engine;

#[test]
fn renders_solid_path_via_library_api() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="4" height="4">
            <rect x="1" y="1" width="2" height="2" fill="#ff0000"/>
        </svg>
    "##;

    let mut fb = Framebuffer::new(4, 4);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        render_svg(svg, &mut ctx).expect("SVG should render");
    }

    let center = ((2 * fb.width() + 2) * 4) as usize;
    assert_eq!(&fb.pixels()[center..center + 4], &[255, 0, 0, 255]);
}

#[test]
fn renders_to_framebuffer_via_library_target_helper() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="2" height="2">
            <rect width="2" height="2" fill="#ff0000"/>
        </svg>
    "##;

    let fb = render_svg_to_framebuffer(svg).expect("SVG should render");
    assert_eq!(fb.width(), 2);
    assert_eq!(fb.height(), 2);
    assert_eq!(&fb.pixels()[0..4], &[255, 0, 0, 255]);
}

#[test]
fn renders_resvg_suite_case_at_reference_png_size() {
    let svg = include_bytes!("../../../tests/resvg-test-suite/tests/shapes/rect/simple-case.svg");
    let png = include_bytes!("../../../tests/resvg-test-suite/tests/shapes/rect/simple-case.png");
    let reference = image::load_from_memory(png)
        .expect("reference PNG should decode")
        .to_rgba8();

    let fb = render_svg_to_framebuffer_at_size(svg, reference.width(), reference.height())
        .expect("SVG should render at reference size");

    assert_eq!(fb.width(), reference.width());
    assert_eq!(fb.height(), reference.height());

    let center = ((250 * fb.width() + 250) * 4) as usize;
    assert_eq!(
        &fb.pixels()[center..center + 4],
        &reference.as_raw()[center..center + 4],
        "simple resvg-suite center pixel should match the reference PNG"
    );
}

#[test]
fn renders_to_lcd_buffer_via_library_target_helper() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="2" height="2">
            <rect width="2" height="2" fill="#ff0000"/>
        </svg>
    "##;

    let buffer = render_svg_to_lcd_buffer(svg).expect("SVG should render");
    assert_eq!(buffer.width(), 2);
    assert_eq!(buffer.height(), 2);
    assert!(buffer
        .color_plane()
        .chunks_exact(3)
        .any(|px| px == [255, 0, 0]));
    assert!(buffer.alpha_plane().iter().any(|&alpha| alpha > 0));
}

#[test]
fn lcd_target_preserves_per_channel_coverage() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="32" height="32">
            <path d="M 4 28 L 28 4" stroke="#000000" stroke-width="3" fill="none"/>
        </svg>
    "##;

    let buffer = render_svg_to_lcd_buffer_at_size(svg, 32, 32).expect("SVG should render");
    assert!(
        buffer
            .alpha_plane()
            .chunks_exact(3)
            .any(|px| px[0] != px[1] || px[1] != px[2]),
        "LCD SVG target should retain per-channel coverage, not collapse to grayscale alpha"
    );
}

#[test]
fn honors_even_odd_fill_rule() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="5" height="5">
            <path fill="#ff0000" fill-rule="evenodd"
                  d="M0 0 H5 V5 H0 Z M1 1 H4 V4 H1 Z"/>
        </svg>
    "##;

    let mut fb = Framebuffer::new(5, 5);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        render_svg(svg, &mut ctx).expect("SVG should render");
    }

    let center = ((2 * fb.width() + 2) * 4) as usize;
    assert_eq!(&fb.pixels()[center..center + 4], &[0, 0, 0, 0]);
}

#[test]
fn multiplies_group_and_paint_opacity() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="3" height="3">
            <g opacity="0.5">
                <rect x="0" y="0" width="3" height="3" fill="#ff0000" fill-opacity="0.5"/>
            </g>
        </svg>
    "##;

    let mut fb = Framebuffer::new(3, 3);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        render_svg(svg, &mut ctx).expect("SVG should render");
    }

    let center = ((1 * fb.width() + 1) * 4) as usize;
    assert_eq!(&fb.pixels()[center..center + 4], &[64, 0, 0, 64]);
}

#[test]
fn renders_embedded_png_image_via_draw_ctx() {
    let mut png = Vec::new();
    let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 255, 0, 255]));
    image::DynamicImage::ImageRgba8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut png),
            image::ImageOutputFormat::Png,
        )
        .expect("test PNG should encode");
    let encoded = base64::engine::general_purpose::STANDARD.encode(png);
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg"
                xmlns:xlink="http://www.w3.org/1999/xlink"
                width="3" height="3">
               <image width="3" height="3" xlink:href="data:image/png;base64,{encoded}"/>
           </svg>"#
    );

    let mut fb = Framebuffer::new(3, 3);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        render_svg(svg.as_bytes(), &mut ctx).expect("SVG should render");
    }

    assert!(
        fb.pixels().chunks_exact(4).any(|px| px == [0, 255, 0, 255]),
        "embedded image should paint at least one green pixel"
    );
}

#[test]
fn applies_svg_node_transforms() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="4" height="4">
            <g transform="translate(1 1)">
                <rect x="0" y="0" width="1" height="1" fill="#0000ff"/>
            </g>
        </svg>
    "##;

    let mut fb = Framebuffer::new(4, 4);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        render_svg(svg, &mut ctx).expect("SVG should render");
    }

    let translated = ((2 * fb.width() + 1) * 4) as usize;
    assert_eq!(&fb.pixels()[translated..translated + 4], &[0, 0, 255, 255]);
}

#[test]
fn svg_coordinates_are_y_down_in_visual_space() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="6" height="6">
            <rect x="1" y="1" width="1" height="1" fill="#ff0000"/>
            <rect x="4" y="4" width="1" height="1" fill="#0000ff"/>
        </svg>
    "##;

    let fb = render_svg_to_framebuffer(svg).expect("SVG should render");

    let top_left_svg_pixel = ((4 * fb.width() + 1) * 4) as usize;
    assert_eq!(
        &fb.pixels()[top_left_svg_pixel..top_left_svg_pixel + 4],
        &[255, 0, 0, 255],
        "SVG y=1 should land one pixel below the visual top"
    );

    let bottom_right_svg_pixel = ((1 * fb.width() + 4) * 4) as usize;
    assert_eq!(
        &fb.pixels()[bottom_right_svg_pixel..bottom_right_svg_pixel + 4],
        &[0, 0, 255, 255],
        "SVG y=4 should land one pixel above the visual bottom"
    );
}

#[test]
fn applies_stroke_dash_array() {
    let solid = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="12" height="3">
            <path d="M0 1.5 H12" stroke="#000000" stroke-width="1" fill="none"/>
        </svg>
    "##;
    let dashed = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="12" height="3">
            <path d="M0 1.5 H12" stroke="#000000" stroke-width="1"
                  stroke-dasharray="2 2" fill="none"/>
        </svg>
    "##;

    let painted_pixels = |svg: &[u8]| -> usize {
        let mut fb = Framebuffer::new(12, 3);
        {
            let mut ctx = GfxCtx::new(&mut fb);
            render_svg(svg, &mut ctx).expect("SVG should render");
        }
        fb.pixels().chunks_exact(4).filter(|px| px[3] > 0).count()
    };

    let solid_count = painted_pixels(solid);
    let dashed_count = painted_pixels(dashed);
    assert!(dashed_count > 0, "dashed stroke should paint some pixels");
    assert!(
        dashed_count < solid_count,
        "dashed stroke should paint fewer pixels than a solid stroke"
    );
}
