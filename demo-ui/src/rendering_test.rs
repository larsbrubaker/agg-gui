//! Rendering test — exact port of egui's `ColorTest` / pixel alignment test.
//!
//! Matches the layout and content of egui's rendering test tab so that agg-gui
//! rendering quality can be compared side-by-side with the egui reference.
//!
//! Sections (top-to-bottom, matching egui):
//! 1. Header text
//! 2. Pixel alignment test (alternating 1-px stripes, squares, stroke grids)
//! 3. Text rendering (4 text-on-bg rows)
//! 4. Blending / feathering test (512×256 split canvas)

use std::sync::Arc;

use agg_gui::{
    Color, DrawCtx, Event, EventResult, FlexColumn, Font, Label, Rect, ScrollView, Separator, Size,
    Widget,
};

mod blending;
mod color;
use blending::BlendingTest;
use color::ColorTest;
// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Build the Rendering Test view.  Returns a `ScrollView` wrapping all sections.
pub fn rendering_test_view(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new().with_gap(4.0).with_padding(10.0);

    let lbl = |text: &str, sz: f64, f: &Arc<Font>| -> Box<dyn Widget> {
        Box::new(
            Label::new(text, Arc::clone(f))
                .with_font_size(sz)
                .with_wrap(true),
        )
    };
    let heading = |text: &str, f: &Arc<Font>| -> Box<dyn Widget> {
        Box::new(Label::new(text, Arc::clone(f)).with_font_size(16.0))
    };

    // ── Header ───────────────────────────────────────────────────────────────
    col.push(
        lbl(
            "This is made to test that the agg-gui rendering backend is set up correctly.",
            13.0,
            &font,
        ),
        0.0,
    );
    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── Pixel alignment test ─────────────────────────────────────────────────
    col.push(heading("Pixel alignment test", &font), 0.0);
    col.push(
        lbl(
            "If anything is blurry, then everything will be blurry, including text.",
            13.0,
            &font,
        ),
        0.0,
    );
    col.push(
        lbl(
            "You might need a magnifying glass to check this test.",
            13.0,
            &font,
        ),
        0.0,
    );
    col.push(
        lbl(
            "The lines should be exactly one physical pixel wide, one physical pixel apart.",
            13.0,
            &font,
        ),
        0.0,
    );
    col.push(
        lbl("They should be perfectly white and black.", 13.0, &font),
        0.0,
    );

    col.push(
        Box::new(PixelTestLines {
            bounds: Rect::default(),
            children: Vec::new(),
        }),
        0.0,
    );

    // Same patterns drawn via AGG software raster into a bitmap, then blit
    // through the Arc-keyed GL texture cache — the identical path used by
    // `Label` backbuffers.  Should be VISUALLY IDENTICAL to the direct-GL
    // grid above; any divergence exposes a bug in the bitmap→texture path.
    col.push(
        lbl(
            "The same two grids, drawn to a bitmap first then blit — \
                  must match pixel-for-pixel.",
            13.0,
            &font,
        ),
        0.0,
    );
    col.push(
        Box::new(PixelTestLinesBitmap {
            bounds: Rect::default(),
            children: Vec::new(),
            bitmap_vertical: None,
            bitmap_horizontal: None,
        }),
        0.0,
    );

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(heading("Color test", &font), 0.0);
    col.push(
        lbl(
            "If the rendering is done right, all groups of gradients will look uniform.",
            13.0,
            &font,
        ),
        0.0,
    );
    col.push(
        Box::new(ColorTest {
            bounds: Rect::default(),
            children: Vec::new(),
            font: Arc::clone(&font),
        }),
        0.0,
    );

    col.push(
        lbl(
            "The first square should be exactly one physical pixel big.",
            13.0,
            &font,
        ),
        0.0,
    );
    col.push(
        lbl(
            "They should be exactly one physical pixel apart.",
            13.0,
            &font,
        ),
        0.0,
    );
    col.push(
        lbl(
            "Each subsequent square should be one physical pixel larger than the previous.",
            13.0,
            &font,
        ),
        0.0,
    );
    col.push(
        lbl(
            "They should be perfectly aligned to the physical pixel grid.",
            13.0,
            &font,
        ),
        0.0,
    );

    col.push(
        Box::new(PixelTestSquares {
            bounds: Rect::default(),
            children: Vec::new(),
        }),
        0.0,
    );

    col.push(
        lbl(
            "The strokes should align to the physical pixel grid.",
            13.0,
            &font,
        ),
        0.0,
    );

    col.push(
        Box::new(PixelTestStrokes {
            bounds: Rect::default(),
            children: Vec::new(),
        }),
        0.0,
    );

    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── Text rendering ───────────────────────────────────────────────────────
    col.push(heading("Text rendering", &font), 0.0);

    // Matches egui's text_on_bg() calls.
    let text_rows: &[(f32, f32, f32, f32, f32, f32)] = &[
        (
            200. / 255.,
            200. / 255.,
            200. / 255.,
            230. / 255.,
            230. / 255.,
            230. / 255.,
        ),
        (
            140. / 255.,
            140. / 255.,
            140. / 255.,
            28. / 255.,
            28. / 255.,
            28. / 255.,
        ),
        (
            39. / 255.,
            39. / 255.,
            39. / 255.,
            255. / 255.,
            255. / 255.,
            255. / 255.,
        ),
        (
            220. / 255.,
            220. / 255.,
            220. / 255.,
            30. / 255.,
            30. / 255.,
            30. / 255.,
        ),
    ];
    for &(fr, fg, fb, br, bg_b, bb) in text_rows {
        let fg_c = Color::rgb(fr, fg, fb);
        let bg_c = Color::rgb(br, bg_b, bb);
        let (fi, bi) = ((fr * 255.0) as u32, (fr * 255.0) as u32);
        let _ = fi;
        let _ = bi;
        let fg_u = (
            (fr * 255.0) as u32,
            (fg * 255.0) as u32,
            (fb * 255.0) as u32,
        );
        let bg_u = (
            (br * 255.0) as u32,
            (bg_b * 255.0) as u32,
            (bb * 255.0) as u32,
        );
        col.push(
            Box::new(TextOnBg {
                bounds: Rect::default(),
                children: Vec::new(),
                font: Arc::clone(&font),
                fg: fg_c,
                bg: bg_c,
                fg_u8: fg_u,
                bg_u8: bg_u,
            }),
            0.0,
        );
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        lbl(
            "The left side shows how lines of different widths look.",
            13.0,
            &font,
        ),
        0.0,
    );
    col.push(
        lbl(
            "The right side tests text rendering at different opacities and sizes.",
            13.0,
            &font,
        ),
        0.0,
    );
    col.push(
        lbl(
            "The top and bottom images should look symmetrical in their intensities.",
            13.0,
            &font,
        ),
        0.0,
    );

    col.push(
        Box::new(BlendingTest {
            bounds: Rect::default(),
            children: Vec::new(),
            font: Arc::clone(&font),
        }),
        0.0,
    );

    Box::new(ScrollView::new(Box::new(col)))
}

// ---------------------------------------------------------------------------
// Pixel test: alternating 1-px stripes
// ---------------------------------------------------------------------------

/// Draws two blocks side by side:
/// - Left: alternating 1-px white/black vertical columns (n/2 pairs)
/// - Right: alternating 1-px white/black horizontal rows (n/2 pairs)
///
/// Direct paint — no backbuffer, no cache.  The companion
/// `PixelTestLinesBitmap` below renders the same content through a
/// cached bitmap path, and the test is that the two produce visually
/// identical output.
struct PixelTestLines {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

const PT_N: f64 = 96.0; // number of columns / rows in each block
const PT_GAP: f64 = 8.0; // gap between the two blocks

impl Widget for PixelTestLines {
    fn type_name(&self) -> &'static str {
        "PixelTestLines"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        // vertical block: PT_N px wide (n/2 pairs of 2px each = n)
        // horizontal block: PT_N px wide
        let w = PT_N + PT_GAP + PT_N;
        let h = PT_N;
        self.bounds = Rect::new(0.0, 0.0, w.min(available.width), h);
        Size::new(w.min(available.width), h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let h = self.bounds.height;
        let n = PT_N as usize;

        ctx.save();
        ctx.snap_to_pixel();

        // Vertical stripes — left block (Y-up: full height).
        for i in 0..(n / 2) {
            let x = (2 * i) as f64;
            ctx.set_fill_color(Color::white());
            ctx.begin_path();
            ctx.rect(x, 0.0, 1.0, h);
            ctx.fill();
            ctx.set_fill_color(Color::black());
            ctx.begin_path();
            ctx.rect(x + 1.0, 0.0, 1.0, h);
            ctx.fill();
        }

        let off_x = PT_N + PT_GAP;

        // Horizontal stripes — right block.
        // In Y-up, row 0 is at the bottom. Match egui visual order: white at bottom.
        for i in 0..(n / 2) {
            let y = (2 * i) as f64;
            ctx.set_fill_color(Color::white());
            ctx.begin_path();
            ctx.rect(off_x, y, PT_N, 1.0);
            ctx.fill();
            ctx.set_fill_color(Color::black());
            ctx.begin_path();
            ctx.rect(off_x, y + 1.0, PT_N, 1.0);
            ctx.fill();
        }

        ctx.restore();
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ---------------------------------------------------------------------------
// Pixel test: 1-px stripes via SOFTWARE BITMAP → GL texture blit
// ---------------------------------------------------------------------------

/// Same two grids as [`PixelTestLines`] but rasterized into an off-screen
/// `Framebuffer` by AGG first, then blit to the screen through the
/// Arc-keyed GL texture cache — the **exact** path that `Label` backbuffers
/// use.  Sits next to the direct polygon-drawn version so the user can
/// visually confirm both paths produce identical pixels.
///
/// Bitmaps are cached on the widget (not in the global image_cache) because
/// their content is static for the lifetime of the widget and we don't want
/// to share with labels that happen to collide on key.
struct PixelTestLinesBitmap {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    /// Arc so the GL L2 cache can key on pointer identity and hold a Weak
    /// ref; re-use across frames with zero CPU work after the first paint.
    ///
    /// Always rendered through the **RGBA** path, irrespective of the
    /// global LCD toggle: the reference `PixelTestLines` above uses
    /// `ctx.rect / fill` directly through the outer GL ctx (no LCD
    /// filter), so for the bitmap-then-blit comparison to be
    /// pixel-for-pixel meaningful the bitmap path has to match that
    /// pipeline.  Routing 1-px stripes through `LcdBuffer`'s
    /// 3×-supersampled + 5-tap text filter would inject colour fringing
    /// the reference doesn't have — that's a different test (and the
    /// 5-tap filter is mathematically guaranteed to smear at the
    /// 1-pixel-stripe Nyquist frequency, so no amount of pipeline
    /// tweaking can recover parity).  The LCD raster pipeline has its
    /// own validation in the LCD Subpixel demo.
    bitmap_vertical: Option<Arc<Vec<u8>>>,
    bitmap_horizontal: Option<Arc<Vec<u8>>>,
}

impl PixelTestLinesBitmap {
    /// Rasterize a PT_N × PT_N bitmap via AGG software (`Rgba` path),
    /// un-premultiply, wrap in an Arc.
    fn raster_to_bitmap<F>(fill_stripes: F) -> Arc<Vec<u8>>
    where
        F: FnOnce(&mut dyn agg_gui::DrawCtx),
    {
        use agg_gui::framebuffer::unpremultiply_rgba_inplace;
        use agg_gui::{Framebuffer, GfxCtx};
        let bw = PT_N as u32;
        let bh = PT_N as u32;
        let mut fb = Framebuffer::new(bw, bh);
        {
            let mut gfx = GfxCtx::new(&mut fb);
            fill_stripes(&mut gfx);
        }
        let mut pixels = fb.pixels_flipped();
        unpremultiply_rgba_inplace(&mut pixels);
        Arc::new(pixels)
    }
}

impl Widget for PixelTestLinesBitmap {
    fn type_name(&self) -> &'static str {
        "PixelTestLinesBitmap"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        let w = PT_N + PT_GAP + PT_N;
        let h = PT_N;
        self.bounds = Rect::new(0.0, 0.0, w.min(available.width), h);
        Size::new(w.min(available.width), h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let n = PT_N as usize;

        // Stripe-fill closures — shared by Rgba and Lcd raster helpers
        // so the patterns are byte-identical modulo the chosen pipeline.
        let vertical_stripes = |gfx: &mut dyn DrawCtx| {
            for i in 0..(n / 2) {
                let x = (2 * i) as f64;
                gfx.set_fill_color(Color::white());
                gfx.begin_path();
                gfx.rect(x, 0.0, 1.0, PT_N);
                gfx.fill();
                gfx.set_fill_color(Color::black());
                gfx.begin_path();
                gfx.rect(x + 1.0, 0.0, 1.0, PT_N);
                gfx.fill();
            }
        };
        let horizontal_stripes = |gfx: &mut dyn DrawCtx| {
            for i in 0..(n / 2) {
                let y = (2 * i) as f64;
                gfx.set_fill_color(Color::white());
                gfx.begin_path();
                gfx.rect(0.0, y, PT_N, 1.0);
                gfx.fill();
                gfx.set_fill_color(Color::black());
                gfx.begin_path();
                gfx.rect(0.0, y + 1.0, PT_N, 1.0);
                gfx.fill();
            }
        };

        ctx.save();
        ctx.snap_to_pixel();
        let bw = PT_N as u32;
        let bh = PT_N as u32;
        let off_x = PT_N + PT_GAP;

        // Always rasterise to a software RGBA bitmap, then blit through
        // the GL texture cache.  Matches `PixelTestLines`'s direct-draw
        // pipeline (also pure RGBA), so the two blocks should be
        // pixel-for-pixel identical regardless of the LCD setting.
        {
            if self.bitmap_vertical.is_none() {
                self.bitmap_vertical = Some(Self::raster_to_bitmap(vertical_stripes));
            }
            if self.bitmap_horizontal.is_none() {
                self.bitmap_horizontal = Some(Self::raster_to_bitmap(horizontal_stripes));
            }
            if let Some(arc) = self.bitmap_vertical.as_ref() {
                ctx.draw_image_rgba_arc(arc, bw, bh, 0.0, 0.0, PT_N, PT_N);
            }
            if let Some(arc) = self.bitmap_horizontal.as_ref() {
                ctx.draw_image_rgba_arc(arc, bw, bh, off_x, 0.0, PT_N, PT_N);
            }
        }
        ctx.restore();
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ---------------------------------------------------------------------------
// Pixel test: increasing-size squares
// ---------------------------------------------------------------------------

/// Draws squares of size 1px, 2px, … num px with 1-px gaps between them.
struct PixelTestSquares {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

const PT_SQ_N: usize = 10;

impl Widget for PixelTestSquares {
    fn type_name(&self) -> &'static str {
        "PixelTestSquares"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        let w: f64 = (1..=PT_SQ_N).map(|s| s as f64 + 1.0).sum::<f64>();
        let h = PT_SQ_N as f64 + 4.0;
        self.bounds = Rect::new(0.0, 0.0, w.min(available.width), h);
        Size::new(w.min(available.width), h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.save();
        ctx.snap_to_pixel();

        let v = ctx.visuals();
        let color = v.text_color;
        // Match egui's top-alignment: all squares share their TOP edge.  In
        // Y-up that means high y = top; each square extends downward so a
        // 1-px square sits flush with the top while a 10-px square reaches
        // further down.  egui Y-down does the mirror of this.
        let h = self.bounds.height;
        let top_y = h - 2.0; // 2-px margin below the text above
        let mut x = 0.0_f64;
        for size in 1..=PT_SQ_N {
            let s = size as f64;
            ctx.set_fill_color(color);
            ctx.begin_path();
            ctx.rect(x, top_y - s, s, s);
            ctx.fill();
            x += s + 1.0;
        }

        ctx.restore();
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ---------------------------------------------------------------------------
// Pixel test: stroke grids (outlined squares at 1px, 2px, 3px border)
// ---------------------------------------------------------------------------

/// Three rows of outlined squares, one row per stroke thickness (1px, 2px, 3px).
struct PixelTestStrokes {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for PixelTestStrokes {
    fn type_name(&self) -> &'static str {
        "PixelTestStrokes"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        // 3 rows, each row height = num_squares + thickness*2 + gap
        let n = 10_usize; // number of squares
        let max_thickness = 3.0_f64;
        // Row heights: for thickness t, row_h = n + t*2 (in pixels)
        let total_h: f64 = (1..=3).map(|t| n as f64 + t as f64 * 2.0 + 2.0).sum();
        let total_w: f64 = (1..=n)
            .map(|s| (s + max_thickness as usize * 2 + 1) as f64)
            .sum::<f64>()
            + 4.0;
        self.bounds = Rect::new(0.0, 0.0, total_w.min(available.width), total_h);
        Size::new(total_w.min(available.width), total_h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.save();
        ctx.snap_to_pixel();

        let v = ctx.visuals();
        let color = v.text_color;
        let n = 10_usize;

        // Match egui row order: thinnest stroke at the visual TOP, thickest
        // at the BOTTOM.  In Y-up top = high y, so start at bounds.height
        // and move downward (subtract row_h each iteration).
        let mut row_top = self.bounds.height;
        for thickness in 1_usize..=3 {
            let t = thickness as f64;
            let row_h = n as f64 + t * 2.0 + 2.0;

            // Top-align the s×s logical rect inside the row with a 1-px +
            // t-px margin (room for the outer stroke above and a visual gap).
            let logical_top = row_top - 1.0 - t;
            let mut cursor_x = t; // left margin = thickness

            ctx.set_fill_color(color);

            for size in 1..=n {
                let s = size as f64;
                let rx = cursor_x;
                let ry = logical_top - s; // bottom of the s×s hole in Y-up
                                          // Draw the outlined ring as four filled rectangles — GUARANTEES
                                          // pixel-perfect corners (no miter / round-join retreat, no AA
                                          // blur from a stroke sampling across corner pixels).  The
                                          // geometry matches egui's `StrokeKind::Outside`: logical rect
                                          // stays empty, stroke ring of thickness t surrounds it.
                                          // Top bar:    (rx-t, logical_top,     s+2t, t)
                                          // Bottom bar: (rx-t, ry-t,            s+2t, t)
                                          // Left bar:   (rx-t, ry,              t,    s)
                                          // Right bar:  (rx+s, ry,              t,    s)
                ctx.begin_path();
                ctx.rect(rx - t, logical_top, s + 2.0 * t, t);
                ctx.fill();
                ctx.begin_path();
                ctx.rect(rx - t, ry - t, s + 2.0 * t, t);
                ctx.fill();
                ctx.begin_path();
                ctx.rect(rx - t, ry, t, s);
                ctx.fill();
                ctx.begin_path();
                ctx.rect(rx + s, ry, t, s);
                ctx.fill();

                cursor_x += s + t * 2.0 + 1.0;
            }

            row_top -= row_h;
        }

        ctx.restore();
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ---------------------------------------------------------------------------
// Text rendering rows: "▣ The quick brown fox…" on colored backgrounds
// ---------------------------------------------------------------------------

/// One row matching egui's `text_on_bg()`: colored text on colored background
/// followed by a "(fg) on (bg)" value label.
struct TextOnBg {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    fg: Color,
    bg: Color,
    fg_u8: (u32, u32, u32),
    bg_u8: (u32, u32, u32),
}

const TEXT_ON_BG_H: f64 = 22.0;
const TEXT_ON_BG_TEXT: &str = "\u{25A3} The quick brown fox jumps over the lazy dog and runs away.";

impl Widget for TextOnBg {
    fn type_name(&self) -> &'static str {
        "TextOnBg"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, TEXT_ON_BG_H);
        Size::new(available.width, TEXT_ON_BG_H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = TEXT_ON_BG_H;
        let pad = 4.0_f64;

        // Measure text so we can draw a snug background rect.
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(12.5);
        let text_w = ctx
            .measure_text(TEXT_ON_BG_TEXT)
            .map(|m| m.width)
            .unwrap_or(300.0);
        let text_h = 14.0_f64;

        // Background rect behind the fox sentence.
        ctx.set_fill_color(self.bg);
        ctx.begin_path();
        ctx.rect(
            0.0,
            (h - text_h - pad * 2.0) * 0.5,
            text_w + pad * 2.0,
            text_h + pad * 2.0,
        );
        ctx.fill();

        // Colored text.
        ctx.set_fill_color(self.fg);
        let baseline = h * 0.32 + 3.0;
        ctx.fill_text(TEXT_ON_BG_TEXT, pad, baseline);

        // Value label (right of the bg rect).
        let label = format!(
            "({} {} {}) on ({} {} {})",
            self.fg_u8.0, self.fg_u8.1, self.fg_u8.2, self.bg_u8.0, self.bg_u8.1, self.bg_u8.2,
        );
        ctx.set_fill_color(ctx.visuals().text_dim);
        ctx.set_font_size(11.5);
        ctx.fill_text(&label, text_w + pad * 2.0 + 8.0, baseline);
        let _ = w;
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}
