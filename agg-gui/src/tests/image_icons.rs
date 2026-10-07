//! Image icons on `Button` and `TreeView` nodes (`IconImage`): the image
//! replaces the glyph / procedural icon, reserves its logical width in
//! layout, and is rasterised at the current device scale.

use std::sync::Arc;

use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::geometry::{Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::icon_image::IconImage;
use crate::text::Font;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::tree_view::{NodeIcon, NodeIconWidget, TreeView};
use crate::widgets::Button;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(super::TEST_FONT).expect("font"))
}

fn solid_icon(rgb: &str, logical: f64) -> IconImage {
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="{rgb}"/></svg>"#
    );
    IconImage::from_svg_data(svg.as_bytes(), Size::new(logical, logical)).expect("icon svg")
}

fn rgb_at(fb: &Framebuffer, x: f64, y: f64) -> [u8; 3] {
    let i = ((y as u32 * fb.width() + x as u32) * 4) as usize;
    [fb.pixels()[i], fb.pixels()[i + 1], fb.pixels()[i + 2]]
}

#[test]
fn icon_only_button_centres_its_image() {
    let icon = solid_icon("#00ff00", 16.0);
    let mut button = Button::new("", font())
        .with_image_icon(icon)
        .with_min_size(Size::new(40.0, 0.0));
    let size = button.layout(Size::new(40.0, 30.0));
    button.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));

    let mut fb = Framebuffer::new(size.width as u32, size.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        paint_subtree(&mut button, &mut ctx);
    }
    let (cx, cy) = (size.width * 0.5, size.height * 0.5);
    assert_eq!(
        rgb_at(&fb, cx, cy),
        [0, 255, 0],
        "image at the button centre"
    );
    assert_ne!(
        rgb_at(&fb, cx - 10.0, cy),
        [0, 255, 0],
        "image spans only its 16 px logical width"
    );
}

#[test]
fn image_icon_takes_the_glyph_slot_width_in_layout() {
    let with_image = |w: f64| {
        let mut b = Button::new("Go", font()).with_image_icon(solid_icon("#ff0000", w));
        b.layout(Size::new(400.0, 30.0)).width
    };
    // Wider image → wider button by the same amount.
    assert_eq!(with_image(30.0) - with_image(10.0), 20.0);
}

#[test]
fn tree_node_image_replaces_the_procedural_icon() {
    let mut tree = TreeView::new(font()).with_row_height(20.0);
    let root = tree.add_root("Part", NodeIcon::File);
    tree.set_node_icon_image(root, Some(solid_icon("#0000ff", 14.0)));
    tree.layout(Size::new(200.0, 40.0));
    tree.set_bounds(Rect::new(0.0, 0.0, 200.0, 40.0));

    let mut fb = Framebuffer::new(200, 40);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        paint_subtree(&mut tree, &mut ctx);
    }
    // Row 0 occupies the top 20 px; the icon cell follows the 18 px expand
    // column.  Its centre is blue, not the procedural file colour.
    let blue = (0..200).any(|x| rgb_at(&fb, x as f64, 30.0) == [0, 0, 255]);
    assert!(blue, "the node's image is painted in its row");
}

#[test]
fn node_icon_widget_reserves_a_wider_image() {
    let mut cell = NodeIconWidget::new(NodeIcon::Folder).with_image(Some(solid_icon("#fff", 24.0)));
    assert_eq!(cell.layout(Size::new(100.0, 20.0)).width, 24.0 + 4.0);
    let mut plain = NodeIconWidget::new(NodeIcon::Folder);
    assert_eq!(plain.layout(Size::new(100.0, 20.0)).width, 14.0 + 4.0);
}

#[test]
fn widget_paint_rasterises_at_device_scale() {
    // device_scale is thread-local, so this does not leak into other tests.
    crate::device_scale::set_device_scale(2.0);
    let icon = solid_icon("#00ff00", 16.0);
    let mut button = Button::new("", font()).with_image_icon(icon.clone());
    let size = button.layout(Size::new(40.0, 30.0));
    button.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    let mut fb = Framebuffer::new(80, 60);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        paint_subtree(&mut button, &mut ctx);
    }
    crate::device_scale::set_device_scale(1.0);
    assert_eq!(
        icon.cached_raster_sizes(),
        vec![(32, 32)],
        "a 16 px icon painted at 2x is rasterised at 32 physical px"
    );
}
