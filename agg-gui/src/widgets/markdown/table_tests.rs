//! Tests ported 1:1 from agg-sharp
//! `Tests/Agg.Tests/MarkdigAgg/MarkdownTableTests.cs`.
//! `StyledHtmlUsesStripedRowsBoldHeadersAndNoHeaderBackgroundFill` lives in
//! `styled_html_tests.rs` and `MatchingTextPipelineStillRendersTables` in
//! `matching_tests.rs`, beside the code they pin.
//!
//! C# walks the `AggTable` / `AggTableRow` / `AggTableCell` widgets
//! MarkdigAgg builds; this view draws its tables itself, so the same facts
//! come from [`MarkdownView::table_cells`], [`MarkdownView::table_sizes`],
//! [`MarkdownView::table_grid_color`] and the laid-out words, and the
//! separator pixels from a software paint.

use std::sync::Arc;

use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::geometry::Size;
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::widget::Widget;

use super::{MarkdownAlign, MarkdownBlockKind, MarkdownTableCell, MarkdownView};

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/LiberationSans-Regular.ttf");

fn render_at(markdown: &str, width: f64) -> MarkdownView {
    crate::widget::set_current_viewport(Size::new(2000.0, 600.0));
    let mut view = MarkdownView::new(
        markdown,
        Arc::new(Font::from_slice(TEST_FONT).expect("test font")),
    );
    view.layout(Size::new(width, 2000.0));
    view
}

fn render_markdown(markdown: &str) -> MarkdownView {
    render_at(markdown, 2000.0)
}

fn table_count(view: &MarkdownView) -> usize {
    view.blocks()
        .iter()
        .filter(|block| **block == MarkdownBlockKind::Table)
        .count()
}

fn single_table(view: &MarkdownView) -> Vec<MarkdownTableCell> {
    let mut tables = view.table_cells();
    assert_eq!(tables.len(), 1);
    tables.remove(0)
}

fn row(cells: &[MarkdownTableCell], row: usize) -> Vec<MarkdownTableCell> {
    cells.iter().filter(|c| c.row == row).cloned().collect()
}

#[test]
fn pipe_tables_render_header_and_body_cells() {
    let view = render_markdown("| Name | Qty |\n| --- | ---: |\n| Bolt | 4 |\n| Nut | 8 |\n");

    assert_eq!(table_count(&view), 1);
    let cells = single_table(&view);
    let rows = cells.iter().map(|c| c.row).max().map_or(0, |r| r + 1);
    assert_eq!(rows, 3);
    assert!(row(&cells, 0).iter().all(|c| c.header));
    assert!(cells.iter().filter(|c| c.row > 0).all(|c| !c.header));
    assert_eq!(cells.len(), 6);
}

#[test]
fn pipe_tables_measure_visible_cell_widths() {
    let view = render_at(
        "| Feature | Status | Notes |\n| :--- | :---: | ---: |\n\
         | Headings | Ready | 6 levels |\n| Tables | Ready | Pipe and grid |\n",
        800.0,
    );

    let cells = single_table(&view);
    assert!(cells.iter().all(|cell| cell.content_width > 0.0));
    assert!(cells.iter().all(|cell| cell.width > 40.0));
}

#[test]
fn grid_tables_render_agg_table() {
    let view = render_markdown(
        "+------+-----+\n| Name | Qty |\n+======+=====+\n| Bolt | 4   |\n\
         +------+-----+\n| Nut  | 8   |\n+------+-----+\n",
    );

    assert_eq!(table_count(&view), 1);
}

#[test]
fn alignment_markers_apply_expected_cell_anchors() {
    let view =
        render_markdown("| Left | Center | Right |\n| :--- | :---: | ---: |\n| A | B | C |\n");

    let header = row(&single_table(&view), 0);
    assert_eq!(header[0].align, MarkdownAlign::Left);
    assert_eq!(header[1].align, MarkdownAlign::Center);
    assert_eq!(header[2].align, MarkdownAlign::Right);
}

#[test]
fn alignment_markers_position_text_within_cells() {
    let view = render_at(
        "| L | C | R |\n| :--- | :---: | ---: |\n\
         | VeryWideLeftContent | VeryWideCenterContent | VeryWideRightContent |\n",
        900.0,
    );

    let header = row(&single_table(&view), 0);
    let (left, center, right) = (&header[0], &header[1], &header[2]);
    assert_eq!(
        (
            left.text.as_str(),
            center.text.as_str(),
            right.text.as_str()
        ),
        ("L", "C", "R")
    );

    assert!(
        left.content_x < center.content_x,
        "{} < {}",
        left.content_x,
        center.content_x
    );
    assert!(
        center.content_x < right.content_x,
        "{} < {}",
        center.content_x,
        right.content_x
    );
}

#[test]
fn edge_aligned_cells_use_symmetric_insets() {
    let view = render_at(
        "| Feature | Status | Notes |\n| --- | --- | ---: |\n\
         | Headings | Ready | 6 levels |\n| Tables | Ready | Pipe and grid |\n",
        900.0,
    );

    let header = row(&single_table(&view), 0);
    let (left, right) = (&header[0], &header[2]);
    assert_eq!(
        (left.text.as_str(), right.text.as_str()),
        ("Feature", "Notes")
    );

    let left_inset = left.content_x;
    let right_inset = right.width - (right.content_x + right.content_width);

    assert!(
        (left_inset - right_inset).abs() < 1.0,
        "{left_inset} vs {right_inset}"
    );
}

#[test]
fn pipe_tables_render_cell_gridlines() {
    let view = render_markdown(
        "| Feature | Status |\n| --- | --- |\n| Tables | Ready |\n| Images | Local |\n",
    );

    // The table paints the whole grid itself (MarkdownTableGridTests checks the pixels): this
    // view has no cell borders or rule widgets to check, only the grid's colour.
    assert_eq!(table_count(&view), 1);
    assert!(view.table_grid_color().a > 0.0);
}

#[test]
fn pipe_tables_render_visible_horizontal_separators() {
    let (w, h) = (600usize, 220usize);
    let mut view = render_at(
        "| Feature | Status |\n| --- | --- |\n| Headings | Ready |\n| Tables | Ready |\n",
        w as f64,
    );
    let mut fb = Framebuffer::new(w as u32, h as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.set_fill_color(Color::white());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w as f64, h as f64);
        ctx.fill();
        // The view's top at the container's top, as C#'s top-anchored widget.
        let content_h = view.bounds().height;
        ctx.translate(0.0, h as f64 - content_h);
        view.paint(&mut ctx);
    }

    // The top of the first body row: the header row's bottom.
    let cells = single_table(&view);
    let table_w: f64 = row(&cells, 0).iter().map(|c| c.width).sum();
    let separator_y = view
        .table_row_tops()
        .first()
        .and_then(|rows| rows.get(1).copied())
        .expect("a body row")
        + (h as f64 - view.bounds().height);
    let separator_y = separator_y.round_ties_even() as i64;
    let (left, right) = (8i64, (8.0 + table_w).round_ties_even() as i64);
    let mut found = false;
    for y in (separator_y - 2)..=(separator_y + 2) {
        for x in (left + 5)..=(right - 5) {
            if y < 0 || y >= h as i64 || x < 0 || x >= w as i64 {
                continue;
            }
            let i = (y as usize * w + x as usize) * 4;
            if fb.pixels()[i..i + 3] != [255, 255, 255] {
                found = true;
            }
        }
    }
    assert!(found);
}

#[test]
fn pipe_tables_use_striping_bold_headers_and_semi_transparent_borders() {
    let view = render_markdown(
        "| Feature | Status |\n| --- | --- |\n| Headings | Ready |\n| Tables | Ready |\n| Images | Ready |\n",
    );

    let cells = single_table(&view);
    assert_eq!(row(&cells, 0)[0].stripe.a, 0.0);
    assert!(row(&cells, 2)[0].stripe.a > 0.0);
    let words = view.laid_out_words();
    assert!(words
        .iter()
        .filter(|(w, _)| w == "Feature" || w == "Status")
        .all(|(_, style)| style.bold));
    let text = crate::theme::current_visuals().text_color;
    assert_eq!(view.table_grid_color(), text.with_alpha(150.0 / 255.0));
}

#[test]
fn inline_markdown_inside_cells_still_renders() {
    let view = render_markdown(
        "| Item | Notes |\n| --- | --- |\n| Bolt | **Strong** text |\n| Docs | [Guide](https://example.com) |\n",
    );

    let cells = single_table(&view);
    let links: usize = cells.iter().map(|cell| cell.links.len()).sum();
    assert_eq!(links, 1);
    assert!(view
        .laid_out_words()
        .iter()
        .any(|(w, style)| w == "Strong" && style.bold));
}

#[test]
fn rust_only_a_cell_link_is_hit_where_it_is_drawn() {
    let view = render_markdown(
        "| Item | Notes |\n| --- | --- |\n| Bolt | **Strong** text |\n| Docs | [Guide](https://example.com) |\n",
    );

    let cells = single_table(&view);
    let guide = &row(&cells, 2)[1];
    assert_eq!(guide.links, vec!["https://example.com".to_string()]);
    let tops = &view.table_row_tops()[0];
    let col0 = row(&cells, 2)[0].width;
    let y = tops[2] - 3.0;
    let x = 8.0 + col0 + guide.content_x + 3.0;
    assert_eq!(
        view.link_at_point(crate::geometry::Point::new(x, y)),
        Some("https://example.com")
    );
    // Past the link's text, in the same cell, and on the row above: no link.
    let past = 8.0 + col0 + guide.content_x + guide.content_width + 3.0;
    assert_eq!(
        view.link_at_point(crate::geometry::Point::new(past, y)),
        None
    );
    assert_eq!(
        view.link_at_point(crate::geometry::Point::new(x, tops[1] - 3.0)),
        None
    );
}

#[test]
fn paragraphs_around_tables_still_render() {
    let view = render_markdown(
        "Intro paragraph.\n\n| Name | Qty |\n| --- | --- |\n| Bolt | 4 |\n\nOutro paragraph.\n",
    );

    let words = view.laid_out_words();
    assert_eq!(table_count(&view), 1);
    assert!(words.iter().any(|(w, _)| w == "Intro"));
    assert!(words.iter().any(|(w, _)| w == "Outro"));
}

#[test]
fn invalid_table_syntax_does_not_render_agg_table() {
    let view = render_markdown("| Name | Qty |\n| Bolt | 4 |\n");

    assert_eq!(table_count(&view), 0);
}

#[test]
fn table_stretches_to_container_width_after_layout() {
    let view = render_at(
        "| Name | Qty |\n| --- | ---: |\n| Bolt | 4 |\n| Nut | 8 |\n",
        800.0,
    );

    let sizes = view.table_sizes();
    assert_eq!(sizes.len(), 1);
    // Stretched: as wide as the view's content (800 less the 8 padding each side).
    assert_eq!(sizes[0].width, 800.0 - 16.0);
    assert!(sizes[0].width > 0.0);
    assert!(sizes[0].height > 0.0);
}
