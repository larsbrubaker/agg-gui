//! `rust_only_*` tests for `markdown_editing::block_painter` (agg-sharp's
//! `RichBlockPainter`).  agg-sharp's painter tests (`RichTableGridPixelTests`)
//! drive the edit widget, which is not ported yet; these paint layouts
//! straight into a software framebuffer and read its pixels, and pin the two
//! C# crashes the port fixes.

use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::widgets::markdown_editing::{
    DocPosition, InlineAtomKind, RichBlock, RichBlockKind, RichBlockLayout, RichBlockPainter,
    RichCaret, RichEditColors, RichImage, RichInline, RichLayout, RichLayoutFonts, RichLayoutStyle,
    RichPaintInputs, RichSelection, RichTableCell, RichTableLayout,
};
use agg_gui::{Color, Framebuffer, GfxCtx};

static REGULAR: &[u8] = include_bytes!("../../../demo/assets/Nunito_Regular.ttf");

const W: u32 = 300;
const H: u32 = 120;

fn style() -> RichLayoutStyle {
    RichLayoutStyle::new(
        RichLayoutFonts {
            regular: Arc::new(Font::from_slice(REGULAR).expect("demo font")),
            bold: None,
            mono: None,
        },
        Some(1.0),
    )
}

fn colors() -> RichEditColors {
    RichEditColors {
        code_background: Color::from_rgb8(0, 200, 0),
        quote_bar: Color::from_rgb8(0, 0, 200),
        selection: Color::from_rgb8(200, 0, 200),
        caret: Color::from_rgb8(200, 0, 0),
        hint: Color::from_rgb8(128, 128, 128),
    }
}

fn pixel(fb: &Framebuffer, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

fn count(fb: &Framebuffer, test: impl Fn([u8; 4]) -> bool) -> usize {
    (0..fb.height())
        .flat_map(|y| (0..fb.width()).map(move |x| (x, y)))
        .filter(|&(x, y)| test(pixel(fb, x, y)))
        .count()
}

fn paint(
    layout: &RichLayout,
    block: &RichBlock,
    style: &RichLayoutStyle,
    image: Option<RichImage>,
) -> Framebuffer {
    let mut fb = Framebuffer::new(W, H);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        let colors = colors();
        let loaded = move |_: &agg_gui::widgets::markdown_editing::InlineAtom| image.clone();
        let inputs = RichPaintInputs {
            style,
            colors: &colors,
            loaded_image: &loaded,
        };
        RichBlockPainter::new().draw_block(&mut ctx, layout, block, 0.0, &inputs);
    }
    fb
}

fn text_layout(block: &RichBlock, style: &RichLayoutStyle) -> RichLayout {
    RichLayout::Text(RichBlockLayout::layout(block, W as f64, style, 1, false).expect("text block"))
}

fn table_block(rows: &[&[&str]]) -> RichBlock {
    let mut block = RichBlock::new(RichBlockKind::Table);
    for row in rows {
        block.table_rows.push(
            row.iter()
                .map(|t| {
                    let mut cell = RichTableCell::default();
                    cell.inlines.push(RichInline::run(*t));
                    cell
                })
                .collect(),
        );
        block.column_alignments = vec![None; row.len()];
    }
    block
}

#[test]
fn rust_only_text_is_painted_in_the_text_colour_and_links_in_the_link_colour() {
    let style = style();
    let mut block = RichBlock::new(RichBlockKind::Paragraph);
    block.inlines.push(RichInline::run("Plain words"));
    let fb = paint(&text_layout(&block, &style), &block, &style, None);
    assert!(
        count(&fb, |p| p[3] > 128 && p[0] < 60 && p[2] < 60) > 20,
        "black text inked"
    );

    let mut link = RichInline::run("Linked words");
    link.link_url = Some("https://example.com".into());
    let mut linked = RichBlock::new(RichBlockKind::Paragraph);
    linked.inlines.push(link);
    let fb = paint(&text_layout(&linked, &style), &linked, &style, None);
    let lc = style.link_color.to_rgba8();
    // The link colour is a blue: its ink has more blue than red.
    assert!(lc.b > lc.r);
    assert!(
        count(&fb, |p| p[3] > 128 && p[2] > p[0] + 40) > 20,
        "link ink is blue"
    );
}

#[test]
fn rust_only_code_block_background_fills_its_box() {
    let style = style();
    let mut block = RichBlock::new(RichBlockKind::CodeBlock);
    block.code_text = "let x = 1;".into();
    let layout = text_layout(&block, &style);
    let RichLayout::Text(text) = &layout else {
        unreachable!()
    };
    let code = text.code_background.expect("a code block is shaded");
    let fb = paint(&layout, &block, &style, None);
    // Just inside the box's bottom-left corner, where no glyph reaches.
    let p = pixel(&fb, (code.x + 2.0) as u32, (code.y + 1.5) as u32);
    assert_eq!(p, [0, 200, 0, 255]);
}

#[test]
fn rust_only_image_box_shows_the_image_or_a_placeholder() {
    let style = style();
    let mut block = RichBlock::new(RichBlockKind::Paragraph);
    block
        .inlines
        .push(RichInline::atom(InlineAtomKind::Image, "![a](b.png)"));
    let layout = text_layout(&block, &style);
    let unloaded = paint(&layout, &block, &style, None);
    assert!(
        count(&unloaded, |p| p == [0, 200, 0, 255]) > 20,
        "placeholder shaded"
    );

    let red = RichImage {
        data: Arc::new([255u8, 0, 0, 255].repeat(4)),
        width: 2,
        height: 2,
    };
    let loaded = paint(&layout, &block, &style, Some(red));
    assert_eq!(count(&loaded, |p| p == [0, 200, 0, 255]), 0);
    assert!(
        count(&loaded, |p| p[0] > 200 && p[1] < 40 && p[3] > 200) > 20,
        "image drawn"
    );
}

#[test]
fn rust_only_table_grid_is_painted_in_the_grid_colour() {
    let style = style();
    let block = table_block(&[
        &["A", "B"],
        &["one", "two"],
        &["three", "four"],
        &["five", "six"],
    ]);
    let table = RichTableLayout::layout(&block, W as f64, &style).expect("table");
    let layout = RichLayout::Table(table);
    let mut painter = RichBlockPainter::new();
    // Tall enough for the whole grid.
    let mut fb = Framebuffer::new(W, 2 * H);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        let colors = colors();
        let inputs = RichPaintInputs {
            style: &style,
            colors: &colors,
            loaded_image: &|_| None,
        };
        painter.draw_block(&mut ctx, &layout, &block, 0.0, &inputs);
    }
    let snap = painter.snapped_grid().clone();
    assert_eq!(snap.lefts.len(), 3);
    assert_eq!(snap.bottoms.len(), 5);
    // Midway along the top rule: the text colour (black) at alpha 150.
    let x = ((snap.lefts[0] + snap.lefts[1]) / 2.0) as u32;
    let p = pixel(&fb, x, snap.bottoms[0] as u32);
    assert_eq!(p[3], 150);
    assert_eq!(&p[..3], &[0, 0, 0]);
    // A striped body row is shaded at the stripe alpha, between its lines.
    let r = layout_stripe(&layout);
    let y = ((snap.bottoms[r] + snap.bottoms[r + 1]) / 2.0) as u32;
    let stripe = pixel(&fb, snap.lefts[1] as u32 + 3, y);
    assert_eq!(stripe[3], RichLayoutStyle::SHADE_ALPHA);
}

fn layout_stripe(layout: &RichLayout) -> usize {
    match layout {
        RichLayout::Table(t) => t.striped_row_indices[0],
        RichLayout::Text(_) => unreachable!(),
    }
}

#[test]
fn rust_only_caret_and_selection_paint_inside_the_block() {
    let style = style();
    let mut block = RichBlock::new(RichBlockKind::Paragraph);
    block.inlines.push(RichInline::run("Some selected text"));
    let layout = text_layout(&block, &style);
    let painter = RichBlockPainter::new();
    let mut fb = Framebuffer::new(W, H);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        // A stale caret past the end is clamped to the end and still drawn.
        painter.draw_caret(
            &mut ctx,
            &layout,
            &block,
            0.0,
            RichCaret::new(500),
            &style,
            colors().caret,
        );
    }
    // The caret's 1px bar may straddle a pixel edge, so it is counted by hue.
    assert!(
        count(&fb, |p| p[3] > 100 && p[0] > 80 && p[1] == 0 && p[2] == 0) > 5,
        "caret drawn"
    );

    let mut fb = Framebuffer::new(W, H);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        let at = |offset| DocPosition {
            block_index: 0,
            offset,
            row: 0,
            column: 0,
        };
        let selection = RichSelection::new(at(5), at(13));
        RichBlockPainter::new().draw_selection(
            &mut ctx,
            &layout,
            0.0,
            &selection,
            0,
            W as f64,
            colors().selection,
        );
    }
    let selected = count(&fb, |p| p == [200, 0, 200, 255]);
    assert!(selected > 100, "selection drawn");
}

#[test]
fn rust_only_a_caret_in_a_row_with_no_cells_draws_nothing() {
    // C# clamped the column to (0, -1) and threw.
    let style = style();
    let mut block = table_block(&[&["A", "B"], &["one", "two"]]);
    block.table_rows.push(Vec::new());
    let table = RichTableLayout::layout(&block, W as f64, &style).expect("table");
    let layout = RichLayout::Table(table);
    let mut fb = Framebuffer::new(W, H);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        let caret = RichCaret {
            row: 2,
            column: 1,
            ..RichCaret::new(0)
        };
        RichBlockPainter::new().draw_caret(
            &mut ctx,
            &layout,
            &block,
            0.0,
            caret,
            &style,
            colors().caret,
        );
    }
    assert_eq!(count(&fb, |p| p[3] != 0), 0);
}

#[test]
fn rust_only_selecting_a_table_whose_first_row_has_no_cells_draws_nothing() {
    // C# divided by the first row's cell count, zero here.
    let style = style();
    let mut block = RichBlock::new(RichBlockKind::Table);
    block.table_rows.push(Vec::new());
    let table = RichTableLayout::layout(&block, W as f64, &style).expect("table");
    let layout = RichLayout::Table(table);
    let mut fb = Framebuffer::new(W, H);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        let start = DocPosition {
            block_index: 0,
            offset: 0,
            row: 0,
            column: 0,
        };
        let end = DocPosition {
            block_index: 1,
            offset: 0,
            row: 0,
            column: 0,
        };
        let selection = RichSelection::new(start, end);
        RichBlockPainter::new().draw_selection(
            &mut ctx,
            &layout,
            0.0,
            &selection,
            0,
            W as f64,
            colors().selection,
        );
    }
    assert_eq!(count(&fb, |p| p[3] != 0), 0);
}
