//! Scrolling demo — six-tab reimplementation of egui's `Scrolling` sample.
//!
//! Tabs (in order):
//!   1. **Appearance**   — full egui-style Details section plus live
//!                         `ScrollBarVisibility` switch and Content length.
//!   2. **Scroll to**    — programmatic scroll-to-index, scroll-to-offset, and
//!                         scroll-by with alignment control.
//!   3. **Many lines**   — row-count slider; rows painted via the viewport
//!                         callback so only visible rows hit text raster.
//!   4. **Large canvas** — 10 000 rows with indentation, custom painter that
//!                         honours the viewport rect exposed by `ScrollView`.
//!   5. **Stick to end** — `with_stick_to_bottom(true)` with an auto-increment
//!                         row counter — scrolls up detach, returning sticks.
//!   6. **Bidirectional**— 100 lorem-ipsum paragraphs, no wrap, both axes.

mod helpers;
mod appearance;
mod scroll_to;
mod many_lines;
mod large_canvas;
mod stick_to_end;
mod bidirectional;

use std::sync::Arc;

use agg_gui::{Font, TabView, Widget};

pub fn scrolling_demo(font: Arc<Font>) -> Box<dyn Widget> {
    let tv = TabView::new(Arc::clone(&font))
        .with_font_size(12.0)
        .add_tab("Appearance",    appearance::build(Arc::clone(&font)))
        .add_tab("Scroll to",     scroll_to::build(Arc::clone(&font)))
        .add_tab("Many lines",    many_lines::build(Arc::clone(&font)))
        .add_tab("Large canvas",  large_canvas::build(Arc::clone(&font)))
        .add_tab("Stick to end",  stick_to_end::build(Arc::clone(&font)))
        .add_tab("Bidirectional", bidirectional::build(Arc::clone(&font)));
    Box::new(tv)
}
