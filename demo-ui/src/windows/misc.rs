//! Miscellaneous demo windows: Frame, Extra Viewport, Highlighting,
//! Interactive Container, Font Book, and Misc Demos.
//!
//! These demos showcase layout containers, custom painting, and Unicode glyph
//! display without requiring external state or animation.

use std::sync::Arc;

use agg_gui::{
    Color, Container, DrawCtx, Event, EventResult, FlexColumn, FlexRow,
    Font, Label, MouseButton, Point, Rect, ScrollView, Separator,
    Size, SizedBox, Widget,
};

// ---------------------------------------------------------------------------
// Frame demo
// ---------------------------------------------------------------------------

/// Build the Frame demo — three `Container` widgets with different border and
/// background combinations placed side by side.
pub fn frame_demo(font: Arc<Font>) -> Box<dyn Widget> {
    let mut outer = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(14.0)
        .with_panel_bg();

    outer.push(Box::new(Label::new("Container styles", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    // Three boxes side by side.
    let row = FlexRow::new().with_gap(10.0)
        .add(Box::new(
            Container::new()
                .with_background(Color::rgba(0.22, 0.45, 0.88, 0.12))
                .with_border(Color::rgb(0.22, 0.45, 0.88), 1.5)
                .with_corner_radius(6.0)
                .with_padding(10.0)
                .add(Box::new(Label::new("Accent fill\nblue border", Arc::clone(&font))
                    .with_font_size(12.0)))
        ))
        .add(Box::new(
            Container::new()
                .with_background(Color::rgba(0.18, 0.72, 0.42, 0.12))
                .with_border(Color::rgb(0.18, 0.72, 0.42), 1.5)
                .with_corner_radius(6.0)
                .with_padding(10.0)
                .add(Box::new(Label::new("Green fill\ngreen border", Arc::clone(&font))
                    .with_font_size(12.0)))
        ))
        .add(Box::new(
            Container::new()
                .with_background(Color::rgba(0.88, 0.25, 0.18, 0.10))
                .with_border(Color::rgb(0.88, 0.25, 0.18), 1.5)
                .with_corner_radius(6.0)
                .with_padding(10.0)
                .add(Box::new(Label::new("Danger fill\nred border", Arc::clone(&font))
                    .with_font_size(12.0)))
        ));

    outer.push(Box::new(row), 0.0);

    outer.push(Box::new(Separator::horizontal()), 0.0);
    outer.push(Box::new(Label::new(
        "Containers support background color, border color/width, corner radius,\n\
         and inner padding. Children are laid out in a top-down stack.",
        Arc::clone(&font),
    ).with_font_size(11.0)), 0.0);

    outer.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(outer)
}

// ---------------------------------------------------------------------------
// Extra Viewport demo
// ---------------------------------------------------------------------------

/// Build the Extra Viewport demo — informational placeholder.
pub fn extra_viewport(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(10.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(Box::new(Label::new(
        "Extra viewports are not supported on this platform.",
        Arc::clone(&font),
    ).with_font_size(13.0)), 0.0);

    Box::new(col)
}

// ---------------------------------------------------------------------------
// Highlighting demo
// ---------------------------------------------------------------------------

/// A widget that draws colored highlight boxes behind individual words.
///
/// This simulates syntax highlighting without a real text-layout engine:
/// each word is measured, a highlight rect is drawn behind it, and then the
/// word is drawn on top.
struct HighlightWidget {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    font:     Arc<Font>,
    /// (word, highlight_color, text_color).
    words:    Vec<(&'static str, Color, Color)>,
}

impl Widget for HighlightWidget {
    fn type_name(&self) -> &'static str { "HighlightWidget" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, 36.0);
        Size::new(available.width, 36.0)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(14.0);

        let pad   = 4.0;
        let h     = self.bounds.height;
        let mut x = pad;
        let baseline = h * 0.35; // Y-up: baseline in lower portion

        for (word, bg, fg) in &self.words {
            if let Some(m) = ctx.measure_text(word) {
                let word_w = m.width;
                let box_h  = m.ascent - m.descent + 4.0;
                let box_y  = baseline + m.descent - 2.0;

                // Highlight box.
                ctx.set_fill_color(*bg);
                ctx.begin_path();
                ctx.rounded_rect(x - 2.0, box_y, word_w + 4.0, box_h, 3.0);
                ctx.fill();

                // Word text.
                ctx.set_fill_color(*fg);
                ctx.fill_text(word, x, baseline);

                x += word_w + 8.0; // gap between words
            }
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

/// Build the Highlighting demo — several highlighted word spans demonstrating
/// per-glyph color control.
pub fn highlighting(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(14.0)
        .with_panel_bg();

    col.push(Box::new(Label::new("Colored text segments", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    col.push(Box::new(HighlightWidget {
        bounds:   Rect::default(),
        children: Vec::new(),
        font:     Arc::clone(&font),
        words: vec![
            ("fn",     Color::rgba(0.22, 0.45, 0.88, 0.30), Color::rgb(0.22, 0.45, 0.88)),
            ("main",   Color::rgba(0.86, 0.78, 0.40, 0.30), Color::rgb(0.86, 0.78, 0.40)),
            ("()",     Color::rgba(0.90, 0.90, 0.90, 0.10), Color::rgb(0.70, 0.70, 0.70)),
            ("{",      Color::rgba(0.90, 0.90, 0.90, 0.10), Color::rgb(0.90, 0.90, 0.90)),
        ],
    }), 0.0);

    col.push(Box::new(HighlightWidget {
        bounds:   Rect::default(),
        children: Vec::new(),
        font:     Arc::clone(&font),
        words: vec![
            ("let",    Color::rgba(0.22, 0.45, 0.88, 0.30), Color::rgb(0.22, 0.45, 0.88)),
            ("x",      Color::rgba(0.90, 0.90, 0.90, 0.10), Color::rgb(0.90, 0.90, 0.90)),
            ("=",      Color::rgba(0.90, 0.90, 0.90, 0.10), Color::rgb(0.60, 0.60, 0.60)),
            ("42;",    Color::rgba(0.82, 0.60, 0.45, 0.30), Color::rgb(0.82, 0.60, 0.45)),
        ],
    }), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new(
        "Each token is measured, a highlight rect is drawn, then the text.",
        Arc::clone(&font),
    ).with_font_size(11.0)), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Interactive Container demo
// ---------------------------------------------------------------------------

/// A widget that changes its appearance on hover and click.
struct InteractiveBox {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    font:     Arc<Font>,
    hovered:  bool,
    pressed:  bool,
    clicks:   u32,
}

impl Widget for InteractiveBox {
    fn type_name(&self) -> &'static str { "InteractiveBox" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let w = available.width.min(200.0);
        let h = 60.0_f64;
        self.bounds = Rect::new(0.0, 0.0, w, h);
        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        let bg = if self.pressed {
            v.accent_pressed
        } else if self.hovered {
            v.accent_hovered
        } else {
            v.widget_bg
        };

        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 8.0);
        ctx.fill();

        ctx.set_stroke_color(if self.hovered { v.accent } else { v.widget_stroke });
        ctx.set_line_width(if self.hovered { 2.0 } else { 1.0 });
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 8.0);
        ctx.stroke();

        let text = if self.clicks == 0 {
            "Click me!".to_string()
        } else {
            format!("Clicked {} time{}", self.clicks, if self.clicks == 1 { "" } else { "s" })
        };

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(13.0);
        ctx.set_fill_color(if self.pressed { Color::white() } else { v.text_color });
        if let Some(m) = ctx.measure_text(&text) {
            let tx = (w - m.width) * 0.5;
            let ty = h * 0.5 - (m.ascent - m.descent) * 0.5 + m.descent;
            ctx.fill_text(&text, tx, ty);
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let was_hovered = self.hovered;
                self.hovered = pos.x >= 0.0 && pos.x <= self.bounds.width
                    && pos.y >= 0.0 && pos.y <= self.bounds.height;
                if self.hovered != was_hovered { EventResult::Consumed } else { EventResult::Ignored }
            }
            Event::MouseDown { button: MouseButton::Left, .. } => {
                if self.hovered {
                    self.pressed = true;
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseUp { button: MouseButton::Left, .. } => {
                if self.pressed {
                    self.pressed = false;
                    if self.hovered { self.clicks += 1; }
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        local_pos.x >= 0.0 && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0 && local_pos.y <= self.bounds.height
    }
}

/// Build the Interactive Container demo — a box that responds to hover and click.
pub fn interactive_container(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(Box::new(Label::new("Hover and click the box", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    col.push(Box::new(InteractiveBox {
        bounds:   Rect::default(),
        children: Vec::new(),
        font:     Arc::clone(&font),
        hovered:  false,
        pressed:  false,
        clicks:   0,
    }), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new(
        "Background, border, and label change on hover / press.",
        Arc::clone(&font),
    ).with_font_size(11.0)), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Font Book demo
// ---------------------------------------------------------------------------

/// A single-glyph cell: shows the character large, with a tiny codepoint label.
struct GlyphCell {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    font:     Arc<Font>,
    ch:       char,
}

impl Widget for GlyphCell {
    fn type_name(&self) -> &'static str { "GlyphCell" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, _available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, 38.0, 44.0);
        Size::new(38.0, 44.0)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // Cell background.
        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 4.0);
        ctx.fill();

        // Glyph.
        let glyph = self.ch.to_string();
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(18.0);
        ctx.set_fill_color(v.text_color);
        if let Some(m) = ctx.measure_text(&glyph) {
            let tx = (w - m.width) * 0.5;
            let ty = h * 0.65 - (m.ascent - m.descent) * 0.5 + m.descent;
            ctx.fill_text(&glyph, tx, ty);
        }

        // Codepoint label.
        let cp = format!("{:04X}", self.ch as u32);
        ctx.set_font_size(7.5);
        ctx.set_fill_color(v.text_dim);
        if let Some(m) = ctx.measure_text(&cp) {
            let tx = (w - m.width) * 0.5;
            ctx.fill_text(&cp, tx, 3.0 + m.ascent - m.descent);
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

/// Build the Font Book demo — a scrollable grid of Unicode glyphs.
pub fn font_book(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(12.0)
        .with_panel_bg();

    // Section header helper.
    let header = |text: &str, font: &Arc<Font>| -> Box<dyn Widget> {
        Box::new(Label::new(text, Arc::clone(font))
            .with_font_size(11.0)
            )
    };

    // Glyph row helper: render `chars` in a wrapping FlexRow.
    let glyph_row = |chars: &[char]| -> Box<dyn Widget> {
        let mut row = FlexRow::new().with_gap(4.0);
        for &ch in chars {
            row.push(Box::new(GlyphCell {
                bounds:   Rect::default(),
                children: Vec::new(),
                font:     Arc::clone(&font),
                ch,
            }), 0.0);
        }
        Box::new(row)
    };

    // Basic Latin — uppercase.
    col.push(header("Basic Latin — Uppercase", &font), 0.0);
    col.push(glyph_row(&('A'..='Z').collect::<Vec<_>>()), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Basic Latin — digits.
    col.push(header("Digits", &font), 0.0);
    col.push(glyph_row(&('0'..='9').collect::<Vec<_>>()), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Greek lowercase.
    col.push(header("Greek lowercase", &font), 0.0);
    col.push(glyph_row(&('α'..='ω').collect::<Vec<_>>()), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Common math symbols.
    let math_syms: &[char] = &[
        '∑', '∏', '∫', '√', '∂', '∞', '≈', '≠', '≤', '≥',
        '±', '×', '÷', '∈', '∉', '⊂', '⊃', '∩', '∪', '∅',
    ];
    col.push(header("Math symbols", &font), 0.0);
    col.push(glyph_row(math_syms), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    Box::new(ScrollView::new(Box::new(col)))
}

// ---------------------------------------------------------------------------
// Misc Demos
// ---------------------------------------------------------------------------

/// A small color swatch widget.
struct ColorSwatch {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    color:    Color,
}

impl Widget for ColorSwatch {
    fn type_name(&self) -> &'static str { "ColorSwatch" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, _available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, 28.0, 28.0);
        Size::new(28.0, 28.0)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_fill_color(self.color);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, 28.0, 28.0, 4.0);
        ctx.fill();
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

/// Build the Misc Demos window — a color swatch grid and a lorem ipsum paragraph.
pub fn misc_demos(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(14.0)
        .with_panel_bg();

    // Color swatch grid.
    col.push(Box::new(Label::new("Color swatches", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    let swatches: &[Color] = &[
        Color::rgb(0.88, 0.25, 0.18), // red
        Color::rgb(0.92, 0.55, 0.15), // orange
        Color::rgb(0.92, 0.85, 0.15), // yellow
        Color::rgb(0.25, 0.78, 0.30), // green
        Color::rgb(0.22, 0.65, 0.88), // cyan
        Color::rgb(0.22, 0.45, 0.88), // blue
        Color::rgb(0.60, 0.25, 0.88), // purple
        Color::rgb(0.88, 0.25, 0.65), // pink
        Color::rgb(0.50, 0.50, 0.50), // gray
        Color::rgb(0.90, 0.90, 0.90), // near-white
    ];

    let mut swatch_row = FlexRow::new().with_gap(6.0);
    for &color in swatches {
        swatch_row.push(Box::new(ColorSwatch {
            bounds:   Rect::default(),
            children: Vec::new(),
            color,
        }), 0.0);
    }
    col.push(Box::new(swatch_row), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Lorem ipsum paragraph.
    col.push(Box::new(Label::new("Sample text", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);
    col.push(Box::new(Label::new(
        "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do \
         eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim \
         ad minim veniam, quis nostrud exercitation ullamco laboris.",
        Arc::clone(&font),
    ).with_font_size(12.5)), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    Box::new(ScrollView::new(Box::new(col)))
}
