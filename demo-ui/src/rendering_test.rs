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
    Color, DrawCtx, Event, EventResult, FlexColumn,
    Font, Label, Rect, ScrollView, Separator, Size, Widget,
};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Build the Rendering Test view.  Returns a `ScrollView` wrapping all sections.
pub fn rendering_test_view(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new().with_gap(4.0).with_padding(10.0);

    let lbl = |text: &str, sz: f64, f: &Arc<Font>| -> Box<dyn Widget> {
        Box::new(Label::new(text, Arc::clone(f)).with_font_size(sz).with_wrap(true))
    };
    let heading = |text: &str, f: &Arc<Font>| -> Box<dyn Widget> {
        Box::new(Label::new(text, Arc::clone(f)).with_font_size(16.0))
    };

    // ── Header ───────────────────────────────────────────────────────────────
    col.push(lbl("This is made to test that the agg-gui rendering backend is set up correctly.", 13.0, &font), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── Pixel alignment test ─────────────────────────────────────────────────
    col.push(heading("Pixel alignment test", &font), 0.0);
    col.push(lbl("If anything is blurry, then everything will be blurry, including text.", 13.0, &font), 0.0);
    col.push(lbl("You might need a magnifying glass to check this test.", 13.0, &font), 0.0);
    col.push(lbl("The lines should be exactly one physical pixel wide, one physical pixel apart.", 13.0, &font), 0.0);
    col.push(lbl("They should be perfectly white and black.", 13.0, &font), 0.0);

    col.push(Box::new(PixelTestLines { bounds: Rect::default(), children: Vec::new() }), 0.0);

    col.push(lbl("The first square should be exactly one physical pixel big.", 13.0, &font), 0.0);
    col.push(lbl("They should be exactly one physical pixel apart.", 13.0, &font), 0.0);
    col.push(lbl("Each subsequent square should be one physical pixel larger than the previous.", 13.0, &font), 0.0);
    col.push(lbl("They should be perfectly aligned to the physical pixel grid.", 13.0, &font), 0.0);

    col.push(Box::new(PixelTestSquares { bounds: Rect::default(), children: Vec::new() }), 0.0);

    col.push(lbl("The strokes should align to the physical pixel grid.", 13.0, &font), 0.0);

    col.push(Box::new(PixelTestStrokes { bounds: Rect::default(), children: Vec::new() }), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── Text rendering ───────────────────────────────────────────────────────
    col.push(heading("Text rendering", &font), 0.0);

    // Matches egui's text_on_bg() calls.
    let text_rows: &[(f32, f32, f32, f32, f32, f32)] = &[
        (200./255., 200./255., 200./255., 230./255., 230./255., 230./255.),
        (140./255., 140./255., 140./255.,  28./255.,  28./255.,  28./255.),
        ( 39./255.,  39./255.,  39./255., 255./255., 255./255., 255./255.),
        (220./255., 220./255., 220./255.,  30./255.,  30./255.,  30./255.),
    ];
    for &(fr, fg, fb, br, bg_b, bb) in text_rows {
        let fg_c = Color::rgb(fr, fg, fb);
        let bg_c = Color::rgb(br, bg_b, bb);
        let (fi, bi) = (
            (fr * 255.0) as u32, (fr * 255.0) as u32,
        );
        let _ = fi; let _ = bi;
        let fg_u = ((fr*255.0) as u32, (fg*255.0) as u32, (fb*255.0) as u32);
        let bg_u = ((br*255.0) as u32, (bg_b*255.0) as u32, (bb*255.0) as u32);
        col.push(Box::new(TextOnBg {
            bounds: Rect::default(),
            children: Vec::new(),
            font: Arc::clone(&font),
            fg: fg_c, bg: bg_c,
            fg_u8: fg_u, bg_u8: bg_u,
        }), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(lbl("The left side shows how lines of different widths look.", 13.0, &font), 0.0);
    col.push(lbl("The right side tests text rendering at different opacities and sizes.", 13.0, &font), 0.0);
    col.push(lbl("The top and bottom images should look symmetrical in their intensities.", 13.0, &font), 0.0);

    col.push(Box::new(BlendingTest {
        bounds: Rect::default(), children: Vec::new(), font: Arc::clone(&font),
    }), 0.0);

    Box::new(ScrollView::new(Box::new(col)))
}

// ---------------------------------------------------------------------------
// Pixel test: alternating 1-px stripes
// ---------------------------------------------------------------------------

/// Draws two blocks side by side:
/// - Left: alternating 1-px white/black vertical columns (n/2 pairs)
/// - Right: alternating 1-px white/black horizontal rows (n/2 pairs)
struct PixelTestLines {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
}

const PT_N: f64 = 96.0;  // number of columns / rows in each block
const PT_GAP: f64 = 8.0; // gap between the two blocks

impl Widget for PixelTestLines {
    fn type_name(&self) -> &'static str { "PixelTestLines" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

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

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ---------------------------------------------------------------------------
// Pixel test: increasing-size squares
// ---------------------------------------------------------------------------

/// Draws squares of size 1px, 2px, … num px with 1-px gaps between them.
struct PixelTestSquares {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
}

const PT_SQ_N: usize = 10;

impl Widget for PixelTestSquares {
    fn type_name(&self) -> &'static str { "PixelTestSquares" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

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
        let h      = self.bounds.height;
        let top_y  = h - 2.0; // 2-px margin below the text above
        let mut x  = 0.0_f64;
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

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ---------------------------------------------------------------------------
// Pixel test: stroke grids (outlined squares at 1px, 2px, 3px border)
// ---------------------------------------------------------------------------

/// Three rows of outlined squares, one row per stroke thickness (1px, 2px, 3px).
struct PixelTestStrokes {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for PixelTestStrokes {
    fn type_name(&self) -> &'static str { "PixelTestStrokes" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        // 3 rows, each row height = num_squares + thickness*2 + gap
        let n = 10_usize; // number of squares
        let max_thickness = 3.0_f64;
        // Row heights: for thickness t, row_h = n + t*2 (in pixels)
        let total_h: f64 = (1..=3).map(|t| n as f64 + t as f64 * 2.0 + 2.0).sum();
        let total_w: f64 = (1..=n).map(|s| (s + max_thickness as usize * 2 + 1) as f64).sum::<f64>() + 4.0;
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
            let t     = thickness as f64;
            let row_h = n as f64 + t * 2.0 + 2.0;

            // Top-align the s×s logical rect inside the row with a 1-px +
            // t-px margin (room for the outer stroke above and a visual gap).
            let logical_top = row_top - 1.0 - t;
            let mut cursor_x = t; // left margin = thickness

            ctx.set_fill_color(color);

            for size in 1..=n {
                let s   = size as f64;
                let rx  = cursor_x;
                let ry  = logical_top - s;   // bottom of the s×s hole in Y-up
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
                ctx.rect(rx - t,     logical_top, s + 2.0 * t, t);
                ctx.fill();
                ctx.begin_path();
                ctx.rect(rx - t,     ry - t,      s + 2.0 * t, t);
                ctx.fill();
                ctx.begin_path();
                ctx.rect(rx - t,     ry,          t,            s);
                ctx.fill();
                ctx.begin_path();
                ctx.rect(rx + s,     ry,          t,            s);
                ctx.fill();

                cursor_x += s + t * 2.0 + 1.0;
            }

            row_top -= row_h;
        }

        ctx.restore();
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ---------------------------------------------------------------------------
// Text rendering rows: "▣ The quick brown fox…" on colored backgrounds
// ---------------------------------------------------------------------------

/// One row matching egui's `text_on_bg()`: colored text on colored background
/// followed by a "(fg) on (bg)" value label.
struct TextOnBg {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    font:     Arc<Font>,
    fg:       Color,
    bg:       Color,
    fg_u8:    (u32, u32, u32),
    bg_u8:    (u32, u32, u32),
}

const TEXT_ON_BG_H: f64 = 22.0;
const TEXT_ON_BG_TEXT: &str =
    "\u{25A3} The quick brown fox jumps over the lazy dog and runs away.";

impl Widget for TextOnBg {
    fn type_name(&self) -> &'static str { "TextOnBg" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, TEXT_ON_BG_H);
        Size::new(available.width, TEXT_ON_BG_H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w   = self.bounds.width;
        let h   = TEXT_ON_BG_H;
        let pad = 4.0_f64;

        // Measure text so we can draw a snug background rect.
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(12.5);
        let text_w = ctx.measure_text(TEXT_ON_BG_TEXT)
            .map(|m| m.width)
            .unwrap_or(300.0);
        let text_h = 14.0_f64;

        // Background rect behind the fox sentence.
        ctx.set_fill_color(self.bg);
        ctx.begin_path();
        ctx.rect(0.0, (h - text_h - pad * 2.0) * 0.5, text_w + pad * 2.0, text_h + pad * 2.0);
        ctx.fill();

        // Colored text.
        ctx.set_fill_color(self.fg);
        let baseline = h * 0.32 + 3.0;
        ctx.fill_text(TEXT_ON_BG_TEXT, pad, baseline);

        // Value label (right of the bg rect).
        let label = format!(
            "({} {} {}) on ({} {} {})",
            self.fg_u8.0, self.fg_u8.1, self.fg_u8.2,
            self.bg_u8.0, self.bg_u8.1, self.bg_u8.2,
        );
        ctx.set_fill_color(ctx.visuals().text_dim);
        ctx.set_font_size(11.5);
        ctx.fill_text(&label, text_w + pad * 2.0 + 8.0, baseline);
        let _ = w;
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ---------------------------------------------------------------------------
// Blending / feathering test
// ---------------------------------------------------------------------------

/// 512 × 256 canvas split top (black bg) / bottom (white bg).
///
/// Each half:
/// - Left side: Bézier curves of 7 increasing stroke widths with width labels
/// - Right side: opacity-fade text labels at 8 levels, then font-size text samples
struct BlendingTest {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    font:     Arc<Font>,
}

impl Widget for BlendingTest {
    fn type_name(&self) -> &'static str { "BlendingTest" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let w = available.width.min(512.0);
        let h = 512.0_f64;
        self.bounds = Rect::new(0.0, 0.0, w, h);
        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let half_h = h * 0.5;

        // Top half: black background, white content.
        ctx.set_fill_color(Color::black());
        ctx.begin_path();
        ctx.rect(0.0, half_h, w, half_h);
        ctx.fill();
        paint_half(ctx, &self.font, 0.0, half_h, w, half_h, Color::white());

        // Bottom half: white background, black content.
        ctx.set_fill_color(Color::white());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, half_h);
        ctx.fill();
        paint_half(ctx, &self.font, 0.0, 0.0, w, half_h, Color::black());
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

/// Paint one half of the blending test, matching egui's `paint_fine_lines_and_text`:
/// - Left side:  corner-sweeping CubicBézier arcs (spiral inward) at 7 stroke widths
/// - Right side: three text columns (white / gray / black) at 8 opacity levels,
///               followed by font-size ramp samples
///
/// The arc rect starts at the left half of this half-panel, shrunk 16 px on every side.
/// Each iteration the visual top drops 24 px and the right edge retreats 24 px, producing
/// the characteristic nested-arc spiral seen in egui.  Y-up coordinate system throughout.
fn paint_half(
    ctx:    &mut dyn DrawCtx,
    font:   &Arc<Font>,
    ox:     f64,  // origin x (lower-left of this half in widget coords)
    oy:     f64,  // origin y
    w:      f64,
    h:      f64,
    color:  Color,
) {
    ctx.set_font(Arc::clone(font));

    // ── Right side: three opacity columns + font-size ramp ───────────────
    // Columns: white / gray / black at decreasing opacities (egui has all three).
    let right_x = ox + w * 0.5 + 4.0;
    let col_w   = (w * 0.5 - 8.0) / 3.0;
    let row_h   = 20.0_f64;
    // Y-up: visually "top" = oy + h; rows step downward.
    let mut text_y = oy + h - row_h * 0.7;

    let opacities: &[f32] = &[1.00, 0.50, 0.25, 0.10, 0.05, 0.02, 0.01, 0.00];
    ctx.set_font_size(11.0);
    for &op in opacities {
        ctx.set_fill_color(Color::white().with_alpha(op));
        ctx.fill_text(&format!("{:.0}% white", 100.0 * op), right_x, text_y);
        ctx.set_fill_color(Color::rgb(0.5, 0.5, 0.5).with_alpha(op));
        ctx.fill_text(&format!("{:.0}% gray",  100.0 * op), right_x + col_w, text_y);
        ctx.set_fill_color(Color::black().with_alpha(op));
        ctx.fill_text(&format!("{:.0}% black", 100.0 * op), right_x + col_w * 2.0, text_y);
        text_y -= row_h;
    }

    // Font-size ramp: drawn in the half's primary color.
    let font_sizes: &[f64] = &[6.0, 7.0, 8.0, 9.0, 10.0, 12.0, 14.0];
    ctx.set_fill_color(color);
    for &sz in font_sizes {
        ctx.set_font_size(sz);
        ctx.fill_text(
            &format!("{sz}px - The quick brown fox jumps over the lazy dog and runs away."),
            right_x,
            text_y,
        );
        text_y -= sz + 1.0;
    }

    // ── Left side: corner-sweeping CubicBézier arcs (egui pattern) ───────
    // Rect is the left half of this half-panel, shrunk 16 px on all sides.
    // In Y-up: visual top = high Y, visual bottom = low Y.
    let rect_left       = ox + 16.0;
    let mut rect_right  = ox + w * 0.5 - 16.0;
    let mut rect_top    = oy + h - 16.0; // Y-up: visual top has large Y
    let rect_bottom     = oy + 16.0;

    let widths: &[f64] = &[0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 4.0];
    ctx.set_font_size(10.0);

    for &lw in widths {
        let center_y = (rect_top + rect_bottom) * 0.5;

        // Label at the visual top-left of the current rect.
        ctx.set_fill_color(color);
        ctx.fill_text(&format!("{lw}"), rect_left, rect_top);

        // CubicBézier sweeping from near left_top to right_top, then down to right_bottom.
        // Egui Y-down: left_top+vec2(16,0) → right_top → right_center → right_bottom.
        // Y-up translation: top=high Y, bottom=low Y — the shape is identical.
        ctx.set_stroke_color(color);
        ctx.set_line_width(lw);
        ctx.begin_path();
        ctx.move_to(rect_left + 16.0, rect_top);
        ctx.cubic_to(
            rect_right, rect_top,    // CP1: right_top
            rect_right, center_y,    // CP2: right_center
            rect_right, rect_bottom, // end:  right_bottom
        );
        ctx.stroke();

        // Shrink rect for next iteration:
        // egui Y-down min.y += 24 → visual top retreats → Y-up: rect_top decreases.
        rect_top   -= 24.0;
        rect_right -= 24.0;
    }

    // ── Gradient bar: transparent → opaque ───────────────────────────────
    let left_x = ox + 16.0;
    let grad_y = oy + 10.0;
    ctx.set_fill_color(color);
    ctx.set_font_size(9.0);
    ctx.fill_text("transparent --> opaque", left_x, grad_y + 12.0);

    let grad_w  = w * 0.5 - 24.0;
    let steps   = 32_usize;
    let step_w  = grad_w / steps as f64;
    for i in 0..steps {
        let alpha = i as f32 / steps as f32;
        ctx.set_fill_color(color.with_alpha(alpha));
        ctx.begin_path();
        ctx.rect(left_x + i as f64 * step_w, grad_y - 8.0, step_w, 8.0);
        ctx.fill();
    }
}
