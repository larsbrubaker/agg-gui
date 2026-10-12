//! Port of agg-sharp `Agg.GoldenImages/GpuTextBaselineSnapTests.cs`: with
//! baseline snapping on (agg-gui's hinting toggle,
//! `font_settings::set_hinting_enabled`, C#'s
//! `TypeFacePrinter.SnapBaselinesToWholePixels`) a label whose baseline falls
//! inside a device pixel draws its bottom row solid, and with it off only
//! partly covers that row, on the GPU exactly as in software, straight to the
//! frame and through a retained GPU layer, at 1x and at fractional display
//! scales.
//!
//! C#'s `GuiWidget.DeviceScale` sizes widgets in device pixels under a unit
//! transform; agg-gui keeps sizes logical and the host paints the tree under
//! `scale(device_scale)` (`widget/app.rs`), so the port sets the device scale
//! and applies that scale before painting. C#'s `TextWidget` is agg-gui's
//! `Label` (its origin is its baseline there; here the label's bounds are
//! placed so its baseline lands where C#'s does), and the double-buffered
//! holder asks for a retained GPU layer as `Window` does.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::font_settings::{hinting_enabled, lcd_enabled, set_hinting_enabled, set_lcd_enabled};
use agg_gui::text::{measure_text_metrics, Font};
use agg_gui::widget::{BackbufferKind, BackbufferSpec, BackbufferState, Widget};
use agg_gui::widgets::Label;
use agg_gui::{device_scale, set_device_scale, Event, EventResult, Insets, Rect, Size};

use super::harness::{at, software_tree_with, GpuFrames};

const FRAME_W: u32 = 200;
const FRAME_H: u32 = 90;

fn font() -> Arc<Font> {
    let bytes: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Regular.ttf");
    Arc::new(Font::from_slice(bytes).expect("parse the test font"))
}

/// A plain widget holding children, optionally painting through a retained
/// GPU layer (C#'s `GuiWidget` with `DoubleBuffer`).
struct Holder {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    double_buffer: bool,
    backbuffer: BackbufferState,
}

impl Holder {
    fn new(bounds: Rect, double_buffer: bool, child: Box<dyn Widget>) -> Self {
        Self {
            bounds,
            children: vec![child],
            double_buffer,
            backbuffer: BackbufferState::new(),
        }
    }
}

impl Widget for Holder {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn backbuffer_spec(&mut self) -> BackbufferSpec {
        if !self.double_buffer {
            return BackbufferSpec::none();
        }
        BackbufferSpec {
            kind: BackbufferKind::GlFbo,
            cached: true,
            alpha: 1.0,
            outsets: Insets::ZERO,
            rounded_clip: None,
        }
    }
    fn backbuffer_state_mut(&mut self) -> Option<&mut BackbufferState> {
        if self.double_buffer {
            Some(&mut self.backbuffer)
        } else {
            None
        }
    }
}

/// C#'s `BuildLabel`: the holder's fraction (2.3) and the label's (10.1)
/// together put the baseline at y 12.4, inside a pixel.
fn build_label(double_buffer: bool) -> Holder {
    let font = font();
    let mut label = Label::new("HHH", Arc::clone(&font))
        .with_font_size(12.0)
        .with_color(Color::black());
    let size = label.layout(Size::new(FRAME_W as f64, FRAME_H as f64));
    let baseline = measure_text_metrics(&font, "HHH", 12.0).centered_baseline_y(size.height);
    label.set_bounds(Rect::new(6.0, 10.1 - baseline, size.width, size.height));
    let holder = Holder::new(
        Rect::new(3.0, 2.3, FRAME_W as f64 - 10.0, FRAME_H as f64 - 10.0),
        double_buffer,
        Box::new(label),
    );
    Holder::new(
        Rect::new(0.0, 0.0, FRAME_W as f64, FRAME_H as f64),
        false,
        Box::new(holder),
    )
}

fn row_ink(image: &[u8], y: u32) -> i32 {
    (0..FRAME_W)
        .map(|x| 255 - at(image, FRAME_W, x, y)[1] as i32)
        .sum()
}

/// The bottom inked row's ink over the row above it. "H" is upright stems
/// down to the baseline, so a baseline on a pixel edge gives a bottom row as
/// dark as the one above (about 1), and a baseline inside a pixel only partly
/// covers its bottom row. Rows are top-row-first, so the scan runs upward
/// from the last row.
fn bottom_row_coverage(image: &[u8]) -> f64 {
    for y in (1..FRAME_H).rev() {
        if row_ink(image, y) > 0 {
            return row_ink(image, y) as f64 / row_ink(image, y - 1).max(1) as f64;
        }
    }
    0.0
}

fn assert_same_image(expected: &[u8], actual: &[u8], what: &str) {
    let mut worst = (0, 0, 0);
    for y in 0..FRAME_H {
        for x in 0..FRAME_W {
            let (a, b) = (at(expected, FRAME_W, x, y), at(actual, FRAME_W, x, y));
            let d = (0..3)
                .map(|c| (a[c] as i32 - b[c] as i32).abs())
                .max()
                .unwrap_or(0);
            if d > worst.0 {
                worst = (d, x, y);
            }
        }
    }
    let (d, x, y) = worst;
    assert!(
        d <= 2,
        "{what}: worst channel delta {d} at ({x}, {y}) top-row-first, expected {:?} got {:?}",
        at(expected, FRAME_W, x, y),
        at(actual, FRAME_W, x, y)
    );
}

/// Restores the process-wide settings the test changes, as C#'s `finally`.
struct Restore {
    hinting: bool,
    lcd: bool,
    scale: f64,
}

impl Drop for Restore {
    fn drop(&mut self) {
        set_hinting_enabled(self.hinting);
        set_lcd_enabled(self.lcd);
        set_device_scale(self.scale);
    }
}

fn snapping_case(display_scale: f64, double_buffer: bool) {
    let _restore = Restore {
        hinting: hinting_enabled(),
        lcd: lcd_enabled(),
        scale: device_scale(),
    };
    let Some(mut frames) = GpuFrames::new(FRAME_W, FRAME_H) else {
        return;
    };
    set_device_scale(display_scale);
    set_lcd_enabled(false);
    let what = format!("scale {display_scale}, double buffer {double_buffer}");
    let before = move |ctx: &mut dyn DrawCtx| ctx.scale(display_scale, display_scale);

    // One tree drawn both ways, so a label already on screen has to follow
    // the toggle as well.
    let mut root = build_label(double_buffer);
    set_hinting_enabled(true);
    let snapped_gpu = frames.paint(Color::white(), &mut root, &before);
    let snapped_cpu =
        software_tree_with(FRAME_W, FRAME_H, Color::white(), false, &mut root, &before);

    set_hinting_enabled(false);
    let unsnapped_gpu = frames.paint(Color::white(), &mut root, &before);
    let unsnapped_cpu =
        software_tree_with(FRAME_W, FRAME_H, Color::white(), false, &mut root, &before);

    let snapped = bottom_row_coverage(&snapped_gpu);
    let unsnapped = bottom_row_coverage(&unsnapped_gpu);
    assert!(
        snapped > 0.9,
        "{what}: with snapping on the baseline sits on a pixel edge, so the stems' bottom row is solid (was {snapped:.2})"
    );
    assert!(
        unsnapped < 0.8,
        "{what}: with snapping off the baseline is inside a pixel, so the bottom row is only partly covered (was {unsnapped:.2})"
    );
    assert_same_image(
        &snapped_cpu,
        &snapped_gpu,
        &format!("{what}: the GPU snaps the baseline as the CPU does"),
    );
    assert_same_image(
        &unsnapped_cpu,
        &unsnapped_gpu,
        &format!("{what}: the GPU leaves an unsnapped baseline where the CPU does"),
    );
}

#[test]
fn snapping_puts_the_gpu_baseline_on_a_whole_device_pixel() {
    for (display_scale, double_buffer) in [
        (1.0, false),
        (1.0, true),
        (1.5, false),
        (1.5, true),
        (3.5, true),
    ] {
        snapping_case(display_scale, double_buffer);
    }
}
