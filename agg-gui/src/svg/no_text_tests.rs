//! SVG rendering without the `svg-text` feature.
//!
//! An app that turns the feature off still gets every SVG drawn: usvg drops
//! `<text>` while parsing, so the rest of the document renders as usual and the
//! text leaves no pixels. Every case goes through the default parse options,
//! which hold the system's fonts when the feature is on, so each one would
//! find drawn text there. The text-drawing counterpart is `text_tests.rs`.

use super::*;
use base64::Engine;

/// A red rect over the left half, and green text over the right half only.
const RECT_AND_TEXT: &[u8] = br##"
    <svg xmlns="http://www.w3.org/2000/svg" width="40" height="20">
        <rect width="20" height="20" fill="#ff0000"/>
        <text x="22" y="16" font-family="sans-serif" font-size="16"
              fill="#00ff00">Hi</text>
    </svg>
"##;

fn is_red(px: &[u8]) -> bool {
    px[0] > 200 && px[1] < 40 && px[2] < 40 && px[3] > 200
}

/// Checks a 40x20 render: the right (text) half is empty, and returns how
/// many left (rect) half pixels are red.
fn red_rect_pixels_with_empty_text_half(fb: &Framebuffer) -> usize {
    assert_eq!((fb.width(), fb.height()), (40, 20));
    let mut red = 0;
    for (index, px) in fb.pixels().chunks_exact(4).enumerate() {
        let x = index as u32 % fb.width();
        if x < 20 {
            red += usize::from(is_red(px));
        } else {
            assert_eq!(px, [0, 0, 0, 0], "no text pixel at x={x}");
        }
    }
    red
}

#[test]
fn svg_text_off_draws_the_rect_and_no_text() {
    let fb = render_svg_to_framebuffer(RECT_AND_TEXT).expect("an SVG with text still renders");
    assert_eq!(
        red_rect_pixels_with_empty_text_half(&fb),
        20 * 20,
        "the rect covers its whole half in red"
    );
}

#[test]
fn svg_text_off_leaves_no_text_node_in_the_tree() {
    let tree = parse_svg_tree(RECT_AND_TEXT, None).expect("an SVG with text still parses");
    let children = tree.root().children();
    assert_eq!(children.len(), 1, "only the rect is left");
    assert!(matches!(children[0], usvg::Node::Path(_)));
}

/// An SVG drawn as an `<image>` of another SVG is parsed with the parent's
/// options (`nested_image.rs`), so its text is skipped the same way.
#[test]
fn svg_text_off_skips_the_text_of_a_nested_svg_image() {
    let encoded = base64::engine::general_purpose::STANDARD.encode(RECT_AND_TEXT);
    let outer = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20">
            <image width="40" height="20" href="data:image/svg+xml;base64,{encoded}"/>
        </svg>"#
    );
    let fb = render_svg_to_framebuffer(outer.as_bytes()).expect("the nested SVG renders");
    let red = red_rect_pixels_with_empty_text_half(&fb);
    assert!(red >= 18 * 18, "the nested rect draws, {red} red pixels");
}
