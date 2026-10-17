//! `agg_gui::Opacity` on the GPU renderer: a red panel wrapped in it, painted
//! over white through a headless frame, comes back pure red at opacity 1, red
//! blended half way over white at 0.5 (one alpha layer composite) and plain
//! white at 0. Skips (passes trivially) when the machine has no adapter.

use std::cell::Cell;
use std::rc::Rc;

use agg_gui::color::Color;
use agg_gui::{App, Container, Opacity, Size};

use crate::headless::{HeadlessFrame, HeadlessGpu};

const SIZE: u32 = 24;

/// The centre pixel of the panel painted at `opacity`, or `None` with no GPU.
fn centre_at(opacity: f64) -> Option<[u8; 4]> {
    let gpu = match HeadlessGpu::shared() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: {e}");
            return None;
        }
    };
    let cell = Rc::new(Cell::new(opacity));
    let panel = Container::new().with_background(Color::rgba(1.0, 0.0, 0.0, 1.0));
    let mut app = App::new(Box::new(Opacity::new(cell, Box::new(panel))));
    app.layout(Size::new(SIZE as f64, SIZE as f64));
    let mut frame = HeadlessFrame::new(gpu, SIZE, SIZE);
    frame.render_app(Color::white(), &mut app);
    let data = frame.read_rgba().expect("read back");
    let i = (((SIZE / 2) * SIZE + SIZE / 2) * 4) as usize;
    Some([data[i], data[i + 1], data[i + 2], data[i + 3]])
}

#[test]
fn an_opaque_panel_paints_unchanged() {
    let Some(px) = centre_at(1.0) else { return };
    assert_eq!(px, [255, 0, 0, 255]);
}

#[test]
fn a_half_faded_panel_blends_once_over_white() {
    let Some([r, g, b, a]) = centre_at(0.5) else {
        return;
    };
    assert_eq!((r, a), (255, 255));
    assert!((120..=136).contains(&g), "green half way; got {g}");
    assert!((120..=136).contains(&b), "blue half way; got {b}");
}

#[test]
fn a_faded_out_panel_paints_nothing() {
    let Some(px) = centre_at(0.0) else { return };
    assert_eq!(px, [255, 255, 255, 255]);
}
