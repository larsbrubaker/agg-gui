//! Tests for images in [`MarkdownView`] table cells (`cell_images.rs`): the
//! behaviour of MatterCAD's
//! `MarkdownTableImageSizingTests.ImagesInTableRow_AllHaveSameWidth`
//! (`Tests/MatterCADTests/Standard`), read through `table_cells`'
//! `image_widths` where C# reads the `ImageLinkSimpleX` widgets of the
//! `AggTable`, plus `rust_only_*` tests that a cell's images stay in the
//! cell through layout, paint, selection, hit-testing and copy.

use std::sync::Arc;

use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::widget::Widget;

use super::selection::SelectableKind;
use super::{LayoutItem, LineRun, MarkdownBlockKind, MarkdownView};

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

const IMAGE_ROW: &str = "|A|BB|CCC|DDDDDDDDDD|\n| :--- | :--- | :--- | :--- |\n|![img](test.png)|![img](test.png)|![img](test.png)|![img](test.png)|";

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).expect("test font"))
}

fn laid_out(mut view: MarkdownView, size: Size) -> MarkdownView {
    crate::widget::set_current_viewport(size);
    view.layout(size);
    view.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    view
}

fn view(markdown: &str) -> MarkdownView {
    laid_out(
        MarkdownView::new(markdown, font()),
        Size::new(600.0, 2000.0),
    )
}

/// A view whose images all decode to a `w`×`h` solid red picture.
fn view_with_images(markdown: &str, w: u32, h: u32, size: Size) -> MarkdownView {
    let pixels = [255u8, 0, 0, 255].repeat((w * h) as usize);
    let view = MarkdownView::new(markdown, font())
        .with_image_provider(move |_| Some((pixels.clone(), w, h)));
    laid_out(view, size)
}

#[test]
fn images_in_table_row_all_have_same_width() {
    let widget = view(IMAGE_ROW);

    let tables = widget.table_cells();
    let table = tables
        .iter()
        .find(|cells| cells.iter().any(|c| !c.image_widths.is_empty()))
        .expect("a table holding the images");
    let widths: Vec<f64> = table
        .iter()
        .flat_map(|c| c.image_widths.iter().copied())
        .collect();

    assert_eq!(widths.len(), 4);
    let max_width = widths.iter().copied().fold(f64::MIN, f64::max);
    for (i, width) in widths.iter().enumerate() {
        assert_eq!(
            *width, max_width,
            "Image {i} width ({width}) should equal the max image width ({max_width})"
        );
    }
}

#[test]
fn rust_only_table_cell_images_stay_in_their_cells() {
    let widget = view(IMAGE_ROW);
    // No paragraph of images after the table: the table is the only block
    // and no row holds an image.
    assert_eq!(widget.blocks(), vec![MarkdownBlockKind::Table]);
    let row_images = widget.items.iter().any(|item| match item {
        LayoutItem::Line { runs, .. } => runs.iter().any(|r| matches!(r, LineRun::Image { .. })),
        _ => false,
    });
    assert!(!row_images, "the images moved out of the table");
    // The cells' text is empty (the alt text is the image's, not the cell's).
    let cells = &widget.table_cells()[0];
    assert!(cells[4..].iter().all(|c| c.text.is_empty()));
    assert_eq!(widget.image_widths().len(), 4);
    assert_eq!(
        widget.plain_text(),
        "A\tBB\tCCC\tDDDDDDDDDD\nimg\timg\timg\timg\n"
    );
}

#[test]
fn rust_only_a_loaded_cell_image_widens_its_column_and_heightens_its_row() {
    let widget = view_with_images(
        "| a | b |\n|---|---|\n| ![i](x.png) | text |\n",
        96,
        48,
        Size::new(600.0, 2000.0),
    );
    let cells = &widget.table_cells()[0];
    assert_eq!(cells[2].image_widths, vec![96.0]);
    assert!(cells[2].width >= 96.0 + 24.0, "{}", cells[2].width);
    let Some(LayoutItem::Table { row_heights, .. }) = widget
        .items
        .iter()
        .find(|item| matches!(item, LayoutItem::Table { .. }))
    else {
        panic!("a table");
    };
    assert!(row_heights[1] >= 48.0 + 8.0, "{row_heights:?}");
    assert!(row_heights[0] < row_heights[1], "{row_heights:?}");
}

#[test]
fn rust_only_text_and_an_image_share_a_cell_in_order() {
    let widget = view("| head |\n|---|\n| before ![pic](x.png) after |\n");
    assert_eq!(widget.table_cells()[0][1].text, "before after");
    assert!(
        widget.plain_text().contains("before pic after"),
        "{:?}",
        widget.plain_text()
    );
    let xs: Vec<(String, f64)> = widget
        .selectable_fragments
        .iter()
        .filter(|f| f.place.table.is_some() && f.text != "head")
        .map(|f| (f.text.clone(), f.text_x))
        .collect();
    assert_eq!(xs.len(), 3, "{xs:?}");
    assert!(xs[0].1 < xs[1].1 && xs[1].1 < xs[2].1, "{xs:?}");
}

#[test]
fn rust_only_a_cell_image_hit_tests_and_paints() {
    let size = Size::new(400.0, 200.0);
    let mut widget = view_with_images("| a |\n|---|\n| ![pic](x.png) |\n", 20, 20, size);
    let image = widget
        .selectable_fragments
        .iter()
        .find(|f| matches!(f.kind, SelectableKind::Image { .. }))
        .expect("an image fragment")
        .clone();
    let centre = Point::new(
        image.text_x + image.width * 0.5,
        image.y + image.height * 0.5,
    );
    assert_eq!(
        widget.hit_image(centre).map(|(url, alt, _)| (url, alt)),
        Some(("x.png".to_string(), "pic".to_string()))
    );

    let mut fb = Framebuffer::new(size.width as u32, size.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.set_fill_color(Color::white());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, size.width, size.height);
        ctx.fill();
        widget.paint(&mut ctx);
    }
    let (x, y) = (centre.x as usize, centre.y as usize);
    let i = (y * size.width as usize + x) * 4;
    let px = &fb.pixels()[i..i + 4];
    assert!(px[0] > 200 && px[1] < 60 && px[2] < 60, "pixel {px:?}");
}

#[test]
fn rust_only_partial_copy_keeps_images() {
    let widget = view("Before ![para](p.png) after.\n\nMore text.\n");
    let text = widget.plain_text().to_string();
    let end = text.find("after").expect("after") + "after".len();
    let (_, html) = widget.copy_payloads(0..end);
    assert!(html.contains("<img src=\"p.png\" alt=\"para\""), "{html}");

    let widget = view("Lead.\n\n| a |\n|---|\n| ![cell](c.png) |\n\nTail.\n");
    let text = widget.plain_text().to_string();
    let start = text.find("cell").expect("cell");
    let (_, html) = widget.copy_payloads(start..start + 4);
    assert!(html.contains("<td style="), "{html}");
    assert!(html.contains("<img src=\"c.png\" alt=\"cell\""), "{html}");
}
