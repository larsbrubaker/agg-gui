//! agg-sharp's `RichTableGridPixelTests.cs`, ported 1:1 against
//! `RichMarkdownEditWidget`, read from the drawn pixels.
//!
//! C#'s editor laid out in device pixels with no draw transform; agg-gui's
//! lays out in logical units and the App paints it under a
//! `ctx.scale(device_scale)` transform, so [`Painted`] lays it out in a
//! `500` x `200` logical container and paints it under that transform into
//! a `500 * s` x `200 * s` device-pixel framebuffer (Y-up, as C#'s back
//! buffer).  Bounds are scaled to device pixels before the pixel checks.  `[NotInParallel]`'s device scale is agg-gui's thread-local one,
//! set and restored per test.  C#'s `[Arguments]` cases are one test each,
//! suffixed with the scale.

use agg_gui::geometry::Point;
use agg_gui::widget::{paint_subtree, set_current_viewport};
use agg_gui::widgets::markdown_editing::edit_operations::whole_block;
use agg_gui::widgets::markdown_editing::{
    DocPosition, RichLayout, RichMarkdownEditWidget, RichSelection,
};
use agg_gui::{Color, Framebuffer, GfxCtx, Rect, Size, Widget};

use super::rich_markdown_edit_widget_tests::fonts;

/// Header, three body rows: the second body row is striped, so a vertical line crosses a stripe.
const TABLE: &str =
    "| Feature | Status | Notes |\n| --- | --- | --- |\n| Headings | Ready | 6 levels |\n\
                     | Tables | Ready | Pipe and grid |\n| Lists | Done | Nested |\n";

/// An editor laid out `editor_width` (logical) wide in a `500` x `200`
/// logical container at `scale`, with a device-pixel framebuffer of the
/// container's size.  Sets the device scale; the caller restores it.
struct Painted {
    editor: RichMarkdownEditWidget,
    fb: Framebuffer,
    scale: f64,
}

impl Painted {
    fn new(scale: f64, editor_width: Option<f64>) -> Self {
        agg_gui::set_device_scale(scale);
        let (width, height) = (500.0, 200.0);
        let editor_width = editor_width.unwrap_or(width);
        set_current_viewport(Size::new(width, height));
        let mut editor = RichMarkdownEditWidget::new(fonts(), None);
        editor.set_markdown(TABLE).unwrap();
        editor.layout(Size::new(editor_width, height));
        editor.set_bounds(Rect::new(0.0, 0.0, editor_width, height));
        Self {
            editor,
            fb: Framebuffer::new((width * scale) as u32, (height * scale) as u32),
            scale,
        }
    }

    /// Clears the container to white and draws the editor; returns a copy
    /// of the pixels (C#'s `new ImageBuffer(container.BackBuffer)`).
    fn render(&mut self) -> Vec<u8> {
        let mut ctx = GfxCtx::new(&mut self.fb);
        ctx.clear(Color::white());
        ctx.scale(self.scale, self.scale);
        paint_subtree(&mut self.editor, &mut ctx);
        drop(ctx);
        self.fb.pixels().to_vec()
    }

    fn width(&self) -> i32 {
        self.fb.width() as i32
    }

    fn height(&self) -> i32 {
        self.fb.height() as i32
    }

    /// Block 0's box (`right` as given) in the editor's (the container's)
    /// coordinates, scaled to device pixels.
    fn block_box(&self, right: f64) -> Rect {
        let origin = self.editor.block_origin(0).unwrap();
        let height = match self.editor.block_layout(0) {
            Some(RichLayout::Table(table)) => table.height,
            _ => panic!("block 0 is not a table"),
        };
        let a = self.editor.view_to_editor(Point::new(0.0, origin));
        let b = self
            .editor
            .view_to_editor(Point::new(right, origin + height));
        let s = self.scale;
        Rect::new(
            a.x.min(b.x) * s,
            a.y.min(b.y) * s,
            (b.x - a.x).abs() * s,
            (b.y - a.y).abs() * s,
        )
    }

    fn table_width(&self) -> f64 {
        match self.editor.block_layout(0) {
            Some(RichLayout::Table(table)) => table.width,
            _ => panic!("block 0 is not a table"),
        }
    }
}

fn pixel(pixels: &[u8], width: i32, x: i32, y: i32) -> [i32; 3] {
    let i = ((y * width + x) * 4) as usize;
    [
        i32::from(pixels[i]),
        i32::from(pixels[i + 1]),
        i32::from(pixels[i + 2]),
    ]
}

fn groups(sorted: impl Iterator<Item = i32>) -> Vec<Vec<i32>> {
    let mut groups: Vec<Vec<i32>> = Vec::new();
    for value in sorted {
        match groups.last_mut() {
            Some(last) if *last.last().unwrap() == value - 1 => last.push(value),
            _ => groups.push(vec![value]),
        }
    }
    groups
}

fn first(group: &[i32]) -> i32 {
    group[0]
}

fn last(group: &[i32]) -> i32 {
    *group.last().unwrap()
}

/// Columns, then rows, inked across most of `bounds` (expanded by 2 px and
/// kept 3 px inside the image's right and top edges, as C#).
fn find_grid(
    ink: &dyn Fn(i32, i32) -> bool,
    bounds: Rect,
    image_w: i32,
    image_h: i32,
    edge: i32,
) -> (Vec<Vec<i32>>, Vec<Vec<i32>>) {
    let min_x = 0.max(bounds.x.floor() as i32 - 2);
    let max_x = (image_w - edge).min((bounds.x + bounds.width).ceil() as i32 + 2);
    let min_y = 0.max(bounds.y.floor() as i32 - 2);
    let max_y = (image_h - edge).min((bounds.y + bounds.height).ceil() as i32 + 2);
    let vertical = groups((min_x..=max_x).filter(|&x| {
        (min_y..=max_y).filter(|&y| ink(x, y)).count() as f64 > f64::from(max_y - min_y) * 0.6
    }));
    let (span_left, span_right) = match (vertical.first(), vertical.last()) {
        (Some(l), Some(r)) => (first(l), last(r)),
        _ => return (vertical, Vec::new()),
    };
    let horizontal = groups((min_y..=max_y).filter(|&y| {
        (span_left..=span_right).filter(|&x| ink(x, y)).count() as f64
            > f64::from(span_right - span_left) * 0.6
    }));
    (vertical, horizontal)
}

fn table_grid_lines_are_crisp_whole_pixels_of_one_tone(device_scale: f64) {
    let mut p = Painted::new(device_scale, None);
    let image = p.render();
    let table_bounds = p.block_box(p.table_width());
    agg_gui::set_device_scale(1.0);
    let (w, h) = (p.width(), p.height());

    let tone = |x: i32, y: i32| pixel(&image, w, x, y)[0];
    // Grid lines are the text colour at alpha 150; stripes and the white page are far lighter.
    let ink = |x: i32, y: i32| tone(x, y) < 200;
    // The grid can start on the image's first column; nothing is drawn outside the image.
    let ink_at = |x: i32, y: i32| x >= 0 && y >= 0 && x < w && y < h && ink(x, y);

    let (vertical, horizontal) = find_grid(&ink, table_bounds, w, h, 3);
    assert_eq!(vertical.len(), 4, "scale {device_scale}");

    // 4 rows -> 5 rules; 3 columns -> 4 verticals; each exactly round(DeviceScale) pixels thick.
    assert_eq!(horizontal.len(), 5, "scale {device_scale}");
    let thickness = (device_scale.round_ties_even() as usize).max(1);
    assert!(horizontal.iter().all(|g| g.len() == thickness));
    assert!(vertical.iter().all(|g| g.len() == thickness));

    let grid_left = first(&vertical[0]);
    let grid_right = last(&vertical[3]);
    let grid_bottom = first(&horizontal[0]);
    let grid_top = last(&horizontal[4]);

    // The reference: the middle of the first vertical, half way between the top two rules.
    let line_tone = tone(
        first(&vertical[0]),
        (first(&horizontal[4]) + last(&horizontal[3])) / 2,
    );

    // Every pixel of every line, crossings and the stretch over the stripe included, is that one tone, and
    // each line spans the outer borders with no gaps.
    for &y in horizontal.iter().flatten() {
        for x in grid_left..=grid_right {
            assert!(
                (tone(x, y) - line_tone).abs() <= 2,
                "({x}, {y}) at {device_scale}"
            );
        }
        assert!(!ink_at(grid_left - 1, y));
        assert!(!ink_at(grid_right + 1, y));
    }
    for &x in vertical.iter().flatten() {
        for y in grid_bottom..=grid_top {
            assert!(
                (tone(x, y) - line_tone).abs() <= 2,
                "({x}, {y}) at {device_scale}"
            );
        }
        assert!(!ink_at(x, grid_bottom - 1));
        assert!(!ink_at(x, grid_top + 1));
    }

    // Crisp edges: the pixel beside a line is the same tone as the one beyond it (no partial coverage).
    for (gi, group) in vertical.iter().enumerate() {
        for r in 0..horizontal.len() - 1 {
            let y = (last(&horizontal[r]) + first(&horizontal[r + 1])) / 2;
            if gi != 0 {
                assert!((tone(first(group) - 1, y) - tone(first(group) - 2, y)).abs() <= 2);
            }
            if gi != vertical.len() - 1 {
                assert!((tone(last(group) + 1, y) - tone(last(group) + 2, y)).abs() <= 2);
            }
        }
    }
    for (gi, group) in horizontal.iter().enumerate() {
        let x = (last(&vertical[0]) + first(&vertical[1])) / 2;
        if gi != 0 {
            assert!((tone(x, first(group) - 1) - tone(x, first(group) - 2)).abs() <= 2);
        }
        if gi != horizontal.len() - 1 {
            assert!((tone(x, last(group) + 1) - tone(x, last(group) + 2)).abs() <= 2);
        }
    }
}

#[test]
fn table_grid_lines_are_crisp_whole_pixels_of_one_tone_1_0() {
    table_grid_lines_are_crisp_whole_pixels_of_one_tone(1.0);
}

#[test]
fn table_grid_lines_are_crisp_whole_pixels_of_one_tone_1_25() {
    table_grid_lines_are_crisp_whole_pixels_of_one_tone(1.25);
}

#[test]
fn table_grid_lines_are_crisp_whole_pixels_of_one_tone_1_5() {
    table_grid_lines_are_crisp_whole_pixels_of_one_tone(1.5);
}

#[test]
fn table_grid_lines_are_crisp_whole_pixels_of_one_tone_2_0() {
    table_grid_lines_are_crisp_whole_pixels_of_one_tone(2.0);
}

fn whole_table_selection_stays_off_the_grid_lines(device_scale: f64) {
    // The editor is narrower than the container, so there are page pixels right of the block to check.
    let mut p = Painted::new(device_scale, Some(420.0));
    p.editor.select(RichSelection::at(DocPosition::new(0, 0)));
    let plain = p.render();
    let selection = whole_block(p.editor.document(), 0);
    p.editor.select(selection);
    assert!(p.editor.selection().whole_block);
    let selected = p.render();
    let block_right = p.editor.block_bounds(0).unwrap();
    let block = p.block_box(block_right.x + block_right.width);
    agg_gui::set_device_scale(1.0);
    let (w, h) = (p.width(), p.height());

    let difference = |x: i32, y: i32| {
        let (a, b) = (pixel(&plain, w, x, y), pixel(&selected, w, x, y));
        (a[0] - b[0])
            .abs()
            .max((a[1] - b[1]).abs())
            .max((a[2] - b[2]).abs())
    };

    // The grid, found on the unselected image as the first test does: lines are far darker than the page.
    let ink = |x: i32, y: i32| pixel(&plain, w, x, y)[0] < 200;
    let (vertical, horizontal) = find_grid(&ink, block, w, h, 1);
    assert_eq!(vertical.len(), 4, "scale {device_scale}");
    assert_eq!(horizontal.len(), 5, "scale {device_scale}");
    let grid_left = first(&vertical[0]);
    let grid_right = last(&vertical[3]);
    let grid_bottom = first(&horizontal[0]);
    let grid_top = last(&horizontal[4]);

    // Every cell is visibly shaded, sampled just inside its bottom-left corner where no text is drawn.
    for (c, column_line) in vertical[..vertical.len() - 1].iter().enumerate() {
        for (r, rule) in horizontal[..horizontal.len() - 1].iter().enumerate() {
            let d = difference(last(column_line) + 2, last(rule) + 2);
            assert!(d > 10, "cell ({c}, {r}) at {device_scale}: {d}");
        }
    }

    // Every grid-line pixel keeps its unselected colour.
    for &y in horizontal.iter().flatten() {
        for x in grid_left..=grid_right {
            assert!(difference(x, y) <= 2, "({x}, {y}) at {device_scale}");
        }
    }
    for &x in vertical.iter().flatten() {
        for y in grid_bottom..=grid_top {
            assert!(difference(x, y) <= 2, "({x}, {y}) at {device_scale}");
        }
    }

    // The shading runs right of the grid to the block's right edge and stops there: the scroll margin
    // beside the block is untouched.
    let right = (block.x + block.width).round_ties_even() as i32;
    let mid_y = (grid_bottom + grid_top) / 2;
    assert!(right + 2 < w);
    assert!(difference(right - 2, mid_y) > 10);
    for x in right + 1..w {
        assert!(
            difference(x, mid_y) <= 2,
            "({x}, {mid_y}) at {device_scale}"
        );
    }
}

#[test]
fn whole_table_selection_stays_off_the_grid_lines_1_0() {
    whole_table_selection_stays_off_the_grid_lines(1.0);
}

#[test]
fn whole_table_selection_stays_off_the_grid_lines_2_0() {
    whole_table_selection_stays_off_the_grid_lines(2.0);
}
