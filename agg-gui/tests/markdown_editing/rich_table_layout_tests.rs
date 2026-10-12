//! agg-sharp's `RichTableLayoutTests` (`Tests/Agg.Tests/MarkdigAgg/
//! RichTableLayoutTests.cs`), ported 1:1 against
//! `markdown_editing::table_layout`.
//!
//! C# lays out with `AggContext.DefaultFont`/`DefaultFontBold`; agg-gui has
//! no implicit fonts, so these tests hand the style the demo's Nunito
//! Regular/Bold and compare a fragment's font with those handles.  The
//! `rust_only_*` tests pin the empty-row hit test fix, the `Result` errors
//! and the pixel-snapped grid.

use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::widgets::markdown_editing::{
    RichAlignment, RichBlock, RichBlockKind, RichCaret, RichInline, RichLayoutError,
    RichLayoutFonts, RichLayoutStyle, RichTableCell, RichTableCellLayout, RichTableGridSnap,
    RichTableLayout,
};
use agg_gui::Point;

const SENTENCE: &str = "The quick brown fox jumps over the lazy dog and keeps on running far away";

static REGULAR: &[u8] = include_bytes!("../../../demo/assets/Nunito_Regular.ttf");
static BOLD: &[u8] = include_bytes!("../../../demo/assets/Nunito_Bold.ttf");

struct Faces {
    regular: Arc<Font>,
    bold: Arc<Font>,
}

fn faces() -> Faces {
    Faces {
        regular: Arc::new(Font::from_slice(REGULAR).expect("demo font")),
        bold: Arc::new(Font::from_slice(BOLD).expect("demo font")),
    }
}

// A fixed scale so the numbers do not depend on the machine's display.
fn style_with(faces: &Faces, scale: f64) -> RichLayoutStyle {
    RichLayoutStyle::new(
        RichLayoutFonts {
            regular: Arc::clone(&faces.regular),
            bold: Some(Arc::clone(&faces.bold)),
            mono: None,
        },
        Some(scale),
    )
}

fn style() -> RichLayoutStyle {
    style_with(&faces(), 1.0)
}

// rows[0] is the header; each string is one cell's plain text.
fn table(rows: &[&[&str]]) -> RichBlock {
    let mut block = RichBlock::new(RichBlockKind::Table);
    for row in rows {
        let cells = row
            .iter()
            .map(|text| {
                let mut cell = RichTableCell::default();
                if !text.is_empty() {
                    cell.inlines.push(RichInline::run(*text));
                }
                cell
            })
            .collect();
        block.table_rows.push(cells);
        block.column_alignments = vec![None; row.len()];
    }
    block
}

fn layout(block: &RichBlock, width: f64, style: &RichLayoutStyle) -> RichTableLayout {
    RichTableLayout::layout(block, width, style).expect("table block")
}

fn cell(layout: &RichTableLayout, row: usize, column: usize) -> &RichTableCellLayout {
    layout.cell(row, column).expect("cell in the table")
}

fn caret(offset: usize, row: usize, column: usize) -> RichCaret {
    RichCaret {
        row,
        column,
        ..RichCaret::new(offset)
    }
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 0.001,
        "{actual} is not within 0.001 of {expected}"
    );
}

#[test]
fn a_table_that_fits_keeps_its_natural_width_and_a_wide_one_shrinks_to_the_block() {
    let long = format!("{SENTENCE} {SENTENCE}");
    let block = table(&[&["Name", "Notes"], &["Fox", &long]]);
    let style = style();

    let wide = layout(&block, 5000.0, &style);
    assert!(wide.width < 5000.0);
    assert_eq!(cell(&wide, 1, 1).lines.len(), 1);

    let narrow = layout(&block, 300.0, &style);
    let sum: f64 = narrow.column_widths.iter().sum();
    assert!(sum <= 300.0);
    assert!(narrow.width <= 300.0);

    // The long column gives up the room; the short one keeps its text on one
    // line.
    close(narrow.column_widths[0], wide.column_widths[0]);
    assert_eq!(cell(&narrow, 1, 0).lines.len(), 1);
    assert!(cell(&narrow, 1, 1).lines.len() > 1);
    for line in &cell(&narrow, 1, 1).lines {
        assert!(line.left + line.width <= cell(&narrow, 1, 1).rect.right());
    }
}

#[test]
fn a_row_is_as_tall_as_its_tallest_cell() {
    let layout = layout(
        &table(&[&["A", "B"], &["short", SENTENCE]]),
        200.0,
        &style(),
    );
    let short_cell = cell(&layout, 1, 0);
    let tall_cell = cell(&layout, 1, 1);

    assert!(tall_cell.lines.len() > 1);
    close(short_cell.rect.height, tall_cell.rect.height);
    assert!(cell(&layout, 1, 0).rect.height > cell(&layout, 0, 0).rect.height);
}

#[test]
fn header_cells_are_bold_and_body_cells_are_not() {
    let faces = faces();
    let layout = layout(
        &table(&[&["Head"], &["Body"]]),
        400.0,
        &style_with(&faces, 1.0),
    );

    let face_of = |row| {
        cell(&layout, row, 0).lines[0].fragments[0]
            .face
            .clone()
            .expect("text face")
    };
    assert!(Arc::ptr_eq(face_of(0).font(), &faces.bold));
    assert!(Arc::ptr_eq(face_of(1).font(), &faces.regular));
    let header = layout.header_background.expect("a header row");
    close(header.top(), cell(&layout, 0, 0).rect.top());
}

#[test]
fn column_alignment_offsets_each_cells_text() {
    let mut block = table(&[
        &[
            "Left column header",
            "Center column header",
            "Right column header",
        ],
        &["a", "b", "c"],
    ]);
    block.column_alignments = vec![
        None,
        Some(RichAlignment::Center),
        Some(RichAlignment::Right),
    ];
    let style = style();
    let layout = layout(&block, 1000.0, &style);

    let left = cell(&layout, 1, 0);
    let center = cell(&layout, 1, 1);
    let right = cell(&layout, 1, 2);
    let pad_x = style.table_cell_padding_x();

    close(left.lines[0].left, left.rect.left() + pad_x);
    let center_text = center.rect.width - 2.0 * pad_x;
    close(
        center.lines[0].left,
        center.rect.left() + pad_x + (center_text - center.lines[0].width) / 2.0,
    );
    close(
        right.lines[0].left + right.lines[0].width,
        right.rect.right() - pad_x,
    );
}

#[test]
fn every_caret_in_every_cell_hit_tests_back_to_itself() {
    let mut block = table(&[
        &["Name", "", "Notes"],
        &["Fox", "", SENTENCE],
        &["", "x", "Dog"],
    ]);
    block.column_alignments = vec![
        None,
        Some(RichAlignment::Center),
        Some(RichAlignment::Right),
    ];
    let layout = layout(&block, 260.0, &style());

    for row in 0..block.table_rows.len() {
        for column in 0..block.table_rows[row].len() {
            for line in &cell(&layout, row, column).lines {
                let mut previous_x = None;
                for stop in &line.caret_stops {
                    let caret = stop.caret;
                    assert_eq!(caret.row, row);
                    assert_eq!(caret.column, column);

                    // Trailing spaces past a wrapped line's edge share an x;
                    // only the first is clickable.
                    let shares_x = previous_x == Some(stop.x);
                    previous_x = Some(stop.x);
                    if !shares_x {
                        let rect = layout.caret_rect(caret, 1.0).expect("caret in its cell");
                        assert_eq!(layout.hit_test(rect.center()), caret);
                    }
                }
            }
        }
    }
}

#[test]
fn an_empty_cell_holds_a_caret() {
    let layout = layout(&table(&[&["A", ""], &["", ""]]), 400.0, &style());
    let empty = cell(&layout, 1, 1);
    let rect = layout
        .caret_rect_at(1, 1, 0, false, 1.0)
        .expect("caret in the cell");

    assert!(rect.height > 0.0);
    assert!(empty.rect.contains(rect.center()));
    assert_eq!(layout.hit_test(rect.center()), caret(0, 1, 1));
}

#[test]
fn padding_and_grid_line_clicks_go_to_the_nearest_cell() {
    let layout = layout(
        &table(&[&["Alpha", "Beta"], &["one", "two"]]),
        400.0,
        &style(),
    );
    let top_left = cell(&layout, 0, 0);
    let bottom_right = cell(&layout, 1, 1);

    // The cell's top-left padding corner goes to its first caret.
    assert_eq!(
        layout.hit_test(Point::new(
            top_left.rect.left() + 1.0,
            top_left.rect.top() - 1.0
        )),
        RichCaret::new(0)
    );

    // Just right of and just left of the middle of the vertical line between
    // the columns.
    let divider = (top_left.rect.right() + cell(&layout, 0, 1).rect.left()) / 2.0;
    let y = top_left.rect.center().y;
    assert_eq!(layout.hit_test(Point::new(divider - 0.1, y)).column, 0);
    assert_eq!(layout.hit_test(Point::new(divider + 0.1, y)).column, 1);

    // Above and below the middle of the line between the rows.
    let rule = (top_left.rect.bottom() + cell(&layout, 1, 0).rect.top()) / 2.0;
    let x = top_left.rect.center().x;
    assert_eq!(layout.hit_test(Point::new(x, rule + 0.1)).row, 0);
    assert_eq!(layout.hit_test(Point::new(x, rule - 0.1)).row, 1);

    // Past the grid's bottom-right corner is the last cell's end.
    assert_eq!(
        layout.hit_test(Point::new(layout.width + 50.0, -50.0)),
        caret(3, 1, 1)
    );
    assert!(bottom_right.rect.right() < layout.width);
}

#[test]
fn a_header_only_table_lays_out_with_a_full_grid() {
    let style = style();
    let block = table(&[&["Only", "Header"]]);
    let layout = layout(&block, 400.0, &style);
    let (before, after) = style.block_spacing(&block, false);

    assert_eq!(layout.rows.len(), 1);
    assert_eq!(layout.striped_rows.len(), 0);

    // Two horizontal rules (above and below the row) and three vertical ones.
    assert_eq!(layout.grid_lines.len(), 5);
    let line = style.table_grid_line_width();
    close(
        layout.height,
        before + line + cell(&layout, 0, 0).rect.height + line + after,
    );
    let header = cell(&layout, 0, 1).rect;
    assert_eq!(
        layout.hit_test(Point::new(header.right() - 1.0, header.center().y)),
        caret(6, 0, 1)
    );
}

#[test]
fn every_other_body_row_is_striped_like_the_viewer() {
    let layout = layout(
        &table(&[&["H"], &["1"], &["2"], &["3"], &["4"]]),
        400.0,
        &style(),
    );

    assert_eq!(layout.striped_rows.len(), 2);
    close(layout.striped_rows[0].top(), cell(&layout, 2, 0).rect.top());
    close(layout.striped_rows[1].top(), cell(&layout, 4, 0).rect.top());
    assert_eq!(layout.striped_row_indices, vec![2, 4]);
}

#[test]
fn rust_only_a_row_with_no_cells_hit_tests_to_its_start() {
    // C# indexed cell -1 of an empty row and threw.
    let mut block = table(&[&["A", "B"], &["one", "two"]]);
    block.table_rows.push(Vec::new());
    let layout = layout(&block, 400.0, &style());

    assert_eq!(
        layout.hit_test(Point::new(layout.width / 2.0, -50.0)),
        caret(0, 2, 0)
    );
}

#[test]
fn rust_only_layout_errors_are_results() {
    let style = style();
    let paragraph = RichBlock::new(RichBlockKind::Paragraph);
    assert!(matches!(
        RichTableLayout::layout(&paragraph, 400.0, &style),
        Err(RichLayoutError::NotTableBlock)
    ));

    let layout = layout(&table(&[&["A"], &["bc"]]), 400.0, &style);
    assert_eq!(
        layout.caret_rect_at(2, 0, 0, false, 1.0).err(),
        Some(RichLayoutError::CellOutOfRange { row: 2, column: 0 })
    );
    assert_eq!(
        layout.caret_rect_at(1, 0, 3, false, 1.0).err(),
        Some(RichLayoutError::OffsetOutOfRange {
            offset: 3,
            length: 2
        })
    );
    assert!(layout.caret_rect_at(1, 0, 2, false, 1.0).is_ok());
}

#[test]
fn rust_only_the_grid_snaps_to_whole_device_pixels() {
    let faces = faces();
    for (scale, thickness) in [(1.0, 1), (1.25, 1), (1.5, 2), (2.0, 2)] {
        let style = style_with(&faces, scale);
        let layout = layout(
            &table(&[
                &["Feature", "Status"],
                &["Headings", "Ready"],
                &["Tables", "Ready"],
            ]),
            500.0,
            &style,
        );
        let mut snap = RichTableGridSnap::default();
        // Drawn as the App draws: logical units under a `scale` transform.
        assert!(layout.snap_grid(10.3, 0.25, 0.5, scale, &mut snap));

        assert_eq!(snap.thickness, thickness, "scale {scale}");
        assert_eq!(
            f64::from(snap.thickness),
            style.table_grid_line_width() * scale
        );
        assert_eq!(snap.line_width, f64::from(thickness) / scale);
        // Three columns' worth of verticals (two columns + the right edge),
        // four rules (three rows + the bottom).
        assert_eq!(snap.lefts.len(), 3);
        assert_eq!(snap.bottoms.len(), 4);
        // Whole device pixels once the transform is applied.
        for &left in &snap.lefts {
            let device = left * scale + 0.25;
            assert!((device - device.round()).abs() < 1e-9, "scale {scale}");
        }
        for &bottom in &snap.bottoms {
            let device = bottom * scale + 0.5;
            assert!((device - device.round()).abs() < 1e-9, "scale {scale}");
        }
        // Lefts left to right, rules top to bottom.
        assert!(snap.lefts.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(snap.bottoms.windows(2).all(|pair| pair[0] > pair[1]));

        // Refilled in place, not appended to.
        assert!(layout.snap_grid(0.0, 0.0, 0.0, scale, &mut snap));
        assert_eq!(snap.lefts.len(), 3);
    }

    let empty = layout(&table(&[]), 400.0, &style_with(&faces, 1.0));
    let mut snap = RichTableGridSnap {
        lefts: vec![1.0],
        bottoms: vec![2.0],
        thickness: 1,
        line_width: 1.0,
    };
    assert!(!empty.snap_grid(0.0, 0.0, 0.0, 1.0, &mut snap));
    assert!(snap.lefts.is_empty() && snap.bottoms.is_empty());
}
