//! Tests ported 1:1 from agg-sharp
//! `Tests/Agg.Tests/MarkdigAgg/MarkdownTableGridTests.cs`, read from the
//! drawn pixels as C# reads its back buffer.
//!
//! C# laid the `MarkdownWidget` out in device pixels inside a
//! `700 * DeviceScale` container; agg-gui widgets lay out in logical units
//! and are drawn under a device-scale transform, so [`paint_at_scale`] lays
//! the view out at the logical size, paints it into a framebuffer of the
//! device size under `scale(s, s)` (as `App` does), and the cell and table
//! boxes are mapped to device pixels the same way.  A cell's width is
//! compared in device pixels (`width * s`), which is what C#'s `cell.Width`
//! was.  The `[NotInParallel]` device scale is agg-gui's thread-local one,
//! set and restored per test.

use std::sync::Arc;

use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::geometry::{Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::widget::Widget;

use super::MarkdownView;

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/LiberationSans-Regular.ttf");

const TABLE: &str = "| Feature | Status | Notes |\n| :--- | :---: | ---: |\n\
                     | Headings | Ready | 6 levels |\n| Tables | Ready | Pipe and grid |\n";

/// A view painted at a device scale: the view, its framebuffer (device
/// pixels, Y-up), the scale and the logical offset the view was drawn at.
pub(super) struct Painted {
    pub view: MarkdownView,
    pub fb: Framebuffer,
    pub scale: f64,
    pub offset_y: f64,
}

impl Painted {
    pub fn red(&self, x: i32, y: i32) -> i32 {
        let i = ((y as u32 * self.fb.width() + x as u32) * 4) as usize;
        i32::from(self.fb.pixels()[i])
    }

    /// `rect` (the view's logical coordinates) in device pixels.
    pub fn to_device(&self, rect: Rect) -> Rect {
        Rect::new(
            rect.x * self.scale,
            (rect.y + self.offset_y) * self.scale,
            rect.width * self.scale,
            rect.height * self.scale,
        )
    }
}

/// Lays `markdown` out in a `width` x `height` logical container at
/// `scale` and paints it, top-anchored, into a white framebuffer of
/// `width * scale` x `height * scale` device pixels, in the light theme.
/// Leaves the device
/// scale set; callers restore it.
pub(super) fn paint_at_scale(markdown: &str, width: f64, height: f64, scale: f64) -> Painted {
    crate::set_device_scale(scale);
    crate::widget::set_current_viewport(Size::new(width, height));
    let mut view = MarkdownView::new(
        markdown,
        Arc::new(Font::from_slice(TEST_FONT).expect("test font")),
    );
    view.layout(Size::new(width, height));
    let (w, h) = (
        (width * scale).round() as u32,
        (height * scale).round() as u32,
    );
    let mut fb = Framebuffer::new(w, h);
    let offset_y = height - view.bounds().height;
    // C# drew with a default (light) `ThemeConfig`: dark text on white.
    let saved_visuals = crate::theme::current_visuals();
    crate::theme::set_visuals(crate::theme::Visuals::light());
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        ctx.scale(scale, scale);
        ctx.translate(0.0, offset_y);
        view.paint(&mut ctx);
    }
    crate::theme::set_visuals(saved_visuals);
    Painted {
        view,
        fb,
        scale,
        offset_y,
    }
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

fn table_grid_lines_meet_at_every_junction(device_scale: f64) {
    let p = paint_at_scale(TABLE, 700.0, 260.0, device_scale);
    crate::set_device_scale(1.0);
    let (iw, ih) = (p.fb.width() as i32, p.fb.height() as i32);
    let bounds = p.to_device(p.view.table_bounds()[0]);
    let min_x = 0.max(bounds.x.floor() as i32);
    let max_x = (iw - 1).min((bounds.x + bounds.width).ceil() as i32);
    let min_y = 0.max(bounds.y.floor() as i32);
    let max_y = (ih - 1).min((bounds.y + bounds.height).ceil() as i32);

    // Grid lines are the theme text color at alpha 150 (about 135 gray on white); stripes stay near 236.
    let ink = |x: i32, y: i32| p.red(x, y) < 180;

    let vertical = groups((min_x..=max_x).filter(|&x| {
        (min_y..=max_y).filter(|&y| ink(x, y)).count() as f64 > f64::from(max_y - min_y) * 0.6
    }));
    assert!(vertical.len() >= 2, "scale {device_scale}");
    let span_left = vertical[0][0];
    let span_right = *vertical.last().unwrap().last().unwrap();
    let horizontal = groups((min_y..=max_y).filter(|&y| {
        (span_left..=span_right).filter(|&x| ink(x, y)).count() as f64
            > f64::from(span_right - span_left) * 0.6
    }));

    // 3 rows -> 4 horizontal rules; 3 columns -> 4 vertical lines.
    assert_eq!(horizontal.len(), 4, "scale {device_scale}");
    assert_eq!(vertical.len(), 4, "scale {device_scale}");

    let grid_left = vertical[0][0];
    let grid_right = *vertical[3].last().unwrap();
    let grid_bottom = horizontal[0][0];
    let grid_top = *horizontal[3].last().unwrap();

    for &y in horizontal.iter().flatten() {
        // The rule reaches both outer borders and stops there.
        let gaps = (grid_left..=grid_right).filter(|&x| !ink(x, y)).count();
        assert_eq!(gaps, 0, "scale {device_scale} row {y}");
        assert!(!ink(grid_left - 1, y));
        assert!(!ink(grid_right + 1, y));
    }

    for &x in vertical.iter().flatten() {
        // The line is unbroken from the bottom rule to the top rule, and stops at them.
        let gaps = (grid_bottom..=grid_top).filter(|&y| !ink(x, y)).count();
        assert_eq!(gaps, 0, "scale {device_scale} column {x}");
        assert!(!ink(x, grid_bottom - 1));
        assert!(!ink(x, grid_top + 1));
    }

    // Every line is the same whole number of device pixels thick.
    let thickness = (device_scale.round_ties_even() as usize).max(1);
    assert!(
        horizontal.iter().all(|g| g.len() == thickness),
        "scale {device_scale}"
    );
    assert!(
        vertical.iter().all(|g| g.len() == thickness),
        "scale {device_scale}"
    );

    // Each column shows the width layout gave its cells: no line ate into a cell, and consistent rounding
    // kept every column the same (at 1.25 the cell edges fall on half pixels).
    let cells = &p.view.table_cells()[0];
    let header: Vec<_> = cells.iter().filter(|c| c.row == 0).collect();
    for (c, cell) in header.iter().enumerate() {
        let interior = vertical[c + 1][0] - vertical[c].last().unwrap() - 1;
        let width = cell.width * device_scale;
        assert!(
            (f64::from(interior) - width).abs() <= 0.5,
            "scale {device_scale} column {c}: {interior} vs {width}"
        );
    }
}

#[test]
fn table_grid_lines_meet_at_every_junction_1_0() {
    table_grid_lines_meet_at_every_junction(1.0);
}

#[test]
fn table_grid_lines_meet_at_every_junction_1_25() {
    table_grid_lines_meet_at_every_junction(1.25);
}

#[test]
fn table_grid_lines_meet_at_every_junction_1_5() {
    table_grid_lines_meet_at_every_junction(1.5);
}

#[test]
fn table_grid_lines_meet_at_every_junction_2_0() {
    table_grid_lines_meet_at_every_junction(2.0);
}

fn striped_rows_paint_between_the_grid_lines(device_scale: f64) {
    let p = paint_at_scale(TABLE, 500.0, 330.0, device_scale);
    crate::set_device_scale(1.0);
    let cells = &p.view.table_cells()[0];
    let row = |r: usize| -> Vec<_> { cells.iter().filter(|c| c.row == r).collect() };
    let (plain_row, striped_row) = (row(1), row(2));
    assert_eq!(plain_row[0].stripe.a, 0.0);
    assert!(striped_row[0].stripe.a > 0.0);

    // The most common value inside a cell is its background (text covers only part of it).
    let cell_background = |bounds: Rect| -> i32 {
        let b = p.to_device(bounds);
        let mut counts = std::collections::BTreeMap::<i32, usize>::new();
        for y in (b.y.ceil() as i32 + 1)..((b.y + b.height).floor() as i32 - 1) {
            for x in (b.x.ceil() as i32 + 1)..((b.x + b.width).floor() as i32 - 1) {
                *counts.entry(p.red(x, y)).or_default() += 1;
            }
        }
        counts
            .into_iter()
            .max_by_key(|&(_, n)| n)
            .map(|(red, _)| red)
            .unwrap()
    };

    for c in 0..striped_row.len() {
        let plain = cell_background(plain_row[c].bounds);
        let striped = cell_background(striped_row[c].bounds);
        assert_eq!(plain, 255, "scale {device_scale} column {c}");
        assert!(striped <= 245, "scale {device_scale} column {c}: {striped}");
    }

    // A vertical grid line pixel reads the same beside an unstriped and a striped cell: the stripe
    // never lies under (and so never tints) a line.
    let line_pixel = |cells: &[&super::MarkdownTableCell]| -> i32 {
        let cell = p.to_device(cells[1].bounds);
        let x = cell.x.floor() as i32 - 1;
        let y = (cell.y + cell.height * 0.5).floor() as i32;
        p.red(x, y)
    };

    let plain_line = line_pixel(&plain_row);
    assert!(plain_line < 180, "scale {device_scale}: {plain_line}");
    assert_eq!(line_pixel(&striped_row), plain_line, "scale {device_scale}");
}

#[test]
fn striped_rows_paint_between_the_grid_lines_1_0() {
    striped_rows_paint_between_the_grid_lines(1.0);
}

#[test]
fn striped_rows_paint_between_the_grid_lines_2_0() {
    striped_rows_paint_between_the_grid_lines(2.0);
}
