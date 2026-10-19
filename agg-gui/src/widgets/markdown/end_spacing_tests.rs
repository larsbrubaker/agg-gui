//! Tests for two options a boxed view (a tooltip) uses:
//! [`MarkdownView::with_end_spacing`], which drops the gap after the
//! document's last block so the view's padding is its only bottom inset, and
//! [`MarkdownView::with_emphasis_color`], which draws `*emphasis*` in its own
//! colour.

use std::sync::Arc;

use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::geometry::Size;
use crate::gfx_ctx::GfxCtx;
use crate::text::{measure_text_metrics, Font};
use crate::widget::Widget;

use super::{LayoutItem, MarkdownColors, MarkdownView};

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/LiberationSans-Regular.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).expect("test font"))
}

fn laid_out(view: MarkdownView) -> MarkdownView {
    crate::widget::set_current_viewport(Size::new(800.0, 600.0));
    let mut view = view;
    view.layout(Size::new(300.0, 2000.0));
    view
}

/// Each laid-out line's (y, height, has text), top first.
fn lines(view: &MarkdownView) -> Vec<(f64, f64, bool)> {
    view.items
        .iter()
        .filter_map(|item| match item {
            LayoutItem::Line {
                runs, y, height, ..
            } => Some((*y, *height, !runs.is_empty())),
            _ => None,
        })
        .collect()
}

#[test]
fn without_end_spacing_the_last_line_sits_on_the_bottom_padding() {
    let pad = 5.0;
    let kept = laid_out(MarkdownView::new("Scale", font()).with_padding(pad));
    let dropped = laid_out(
        MarkdownView::new("Scale", font())
            .with_padding(pad)
            .with_end_spacing(false),
    );
    let spacer = measure_text_metrics(&font(), "", 14.0).line_height * 0.65;
    // By default the paragraph's gap follows it to the end of the view.
    assert_eq!(lines(&kept).len(), 2);
    let gap = kept.bounds().height - dropped.bounds().height;
    assert!((gap - spacer).abs() < 1e-9, "{gap} vs {spacer}");
    // Without it the one row fills the view between equal paddings.
    let rows = lines(&dropped);
    assert_eq!(rows.len(), 1, "{rows:?}");
    let (y, height, _) = rows[0];
    assert!((y - pad).abs() < 1e-9, "the row starts on the padding: {y}");
    assert!((dropped.bounds().height - (height + 2.0 * pad)).abs() < 1e-9);
}

#[test]
fn without_end_spacing_the_gap_between_paragraphs_stays() {
    let view = laid_out(
        MarkdownView::new("Fillet\n\n*At least 1 part must be selected*", font())
            .with_end_spacing(false),
    );
    let rows = lines(&view);
    let texts: Vec<bool> = rows.iter().map(|(_, _, text)| *text).collect();
    assert_eq!(texts, vec![true, false, true], "{rows:?}");
}

fn render(view: &mut MarkdownView) -> Framebuffer {
    let size = view.bounds();
    let mut fb = Framebuffer::new(size.width as u32, size.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.set_fill_color(Color::white());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, size.width, size.height);
        ctx.fill();
        view.paint(&mut ctx);
    }
    fb
}

/// Pixels clearly redder than they are green or blue.
fn red_pixels(fb: &Framebuffer) -> usize {
    fb.pixels()
        .chunks_exact(4)
        .filter(|p| p[0] > 150 && p[1] < 100 && p[2] < 100)
        .count()
}

fn colors() -> MarkdownColors {
    MarkdownColors {
        text: Color::black(),
        link: Color::rgb(0.0, 0.0, 1.0),
        code_background: Color::rgb(0.9, 0.9, 0.9),
        rule: Color::rgb(0.5, 0.5, 0.5),
        background: None,
    }
}

#[test]
fn emphasis_draws_in_the_emphasis_color() {
    let red = Color::rgb(1.0, 0.0, 0.0);
    let mut plain = laid_out(MarkdownView::new("Fillet *help line*", font()).with_colors(colors()));
    let mut tinted = laid_out(
        MarkdownView::new("Fillet *help line*", font())
            .with_colors(colors())
            .with_emphasis_color(red),
    );
    assert_eq!(tinted.emphasis_color(), Some(red));
    assert_eq!(red_pixels(&render(&mut plain)), 0);
    assert!(red_pixels(&render(&mut tinted)) > 20);
    // Plain words keep the text colour.
    let mut name = laid_out(
        MarkdownView::new("Fillet", font())
            .with_colors(colors())
            .with_emphasis_color(red),
    );
    assert_eq!(red_pixels(&render(&mut name)), 0);
}
