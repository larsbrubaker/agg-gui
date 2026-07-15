//! Code Editor demo — reproduces egui's `code_editor.rs`
//! (`crates/egui_demo_lib/src/demo/code_editor.rs`) in spirit.
//!
//! An **editable** multiline code buffer rendered in a monospace face, with a
//! hand-rolled Rust syntax highlighter (keywords / strings / line comments /
//! numbers — the same token classes as egui's built-in fallback highlighter,
//! no syntect) and a line-number gutter (our addition, kept from the original
//! read-only view).
//!
//! Editability + highlighting come from [`agg_gui::TextArea`]'s new
//! `with_highlighter` hook, which paints each wrapped line as coloured runs.
//! The gutter is a small sibling widget ([`LineGutter`]) that mirrors the
//! editor's line metrics.
//!
//! Deviations / notes:
//!   * egui offers a `Language` field and a `Theme` selector (both behind the
//!     `syntect` feature). We skip both — the highlighter here is intentionally
//!     Rust-only and the plumbing for a theme picker would dwarf the demo. The
//!     highlight colours are fixed constants below.
//!   * The gutter numbers source lines. Our `TextArea` always word-wraps, so a
//!     line long enough to wrap would push later numbers out of alignment; the
//!     sample fits the width, so this doesn't bite in practice.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    measure_text_metrics, Color, DrawCtx, Event, EventResult, FlexColumn, FlexRow, Font, HAnchor,
    Hyperlink, Label, Rect, ScrollView, Separator, Size, SizedBox, TextArea, TextEditState, Widget,
};

const SOURCE_URL: &str =
    "https://github.com/larsbrubaker/agg-gui/blob/main/demo-ui/src/windows/code_editor_demo.rs";

/// Starting buffer — exercises every token class the highlighter knows.
const SAMPLE: &str = "\
// A tiny agg-gui example
fn main() {
    let greeting = \"Hello, agg-gui!\";
    println!(\"{}\", greeting);

    let values: Vec<f64> = (0..10)
        .map(|i| i as f64 * 0.1)
        .collect();

    for (i, v) in values.iter().enumerate() {
        println!(\"[{i}] {v:.2}\");
    }
}";

const EDITOR_FONT_SIZE: f64 = 13.0;
const EDITOR_PADDING: f64 = 8.0;

// ── Highlight palette (dark-theme friendly, fixed) ──────────────────────────
fn color_keyword() -> Color {
    Color::rgb(0.55, 0.63, 0.96)
}
fn color_string() -> Color {
    Color::rgb(0.68, 0.84, 0.5)
}
fn color_comment() -> Color {
    Color::rgb(0.48, 0.53, 0.6)
}
fn color_number() -> Color {
    Color::rgb(0.92, 0.66, 0.42)
}

/// Load the bundled monospace face for the editor + gutter (the demo's default
/// font is proportional; a code editor wants fixed-width glyphs).
fn code_font() -> Arc<Font> {
    const BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(BYTES).expect("parse CascadiaCode.ttf"))
}

/// Build the Code Editor demo.
pub fn code_editor(font: Arc<Font>) -> Box<dyn Widget> {
    let code_font = code_font();
    let state = Rc::new(RefCell::new(TextEditState {
        text: SAMPLE.to_string(),
        cursor: 0,
        anchor: 0,
        epoch: 0,
    }));

    let bg = Color::rgb(0.12, 0.13, 0.15);
    let mut col = FlexColumn::new().with_gap(0.0).with_background(bg);

    // Header row: description + source link (egui puts these on one line).
    col.push(
        Box::new(
            FlexRow::new()
                .with_gap(8.0)
                .add(Box::new(
                    Label::new(
                        "An example of syntax highlighting in an editable TextEdit.",
                        Arc::clone(&font),
                    )
                    .with_font_size(12.0)
                    .with_color(Color::rgba(1.0, 1.0, 1.0, 0.65)),
                ))
                .add(Box::new(
                    Hyperlink::new("(source code)", Arc::clone(&font))
                        .with_font_size(11.0)
                        .with_h_anchor(HAnchor::CENTER)
                        .on_click(|| crate::url::open_url(SOURCE_URL)),
                )),
        ),
        0.0,
    );
    col.push(Box::new(Separator::horizontal()), 0.0);

    // Editor area: line-number gutter + highlighted editable buffer.
    let editor = TextArea::new(Arc::clone(&code_font))
        .with_font_size(EDITOR_FONT_SIZE)
        .with_padding(EDITOR_PADDING)
        .with_edit_state(Rc::clone(&state))
        .with_highlighter(rust_highlighter);

    let gutter = LineGutter::new(
        Rc::clone(&state),
        Arc::clone(&code_font),
        EDITOR_FONT_SIZE,
        EDITOR_PADDING,
    );

    let editor_row = FlexRow::new()
        .with_gap(0.0)
        .add(Box::new(gutter))
        .add_flex(Box::new(editor), 1.0);

    col.push(
        Box::new(
            SizedBox::new()
                .with_height(360.0)
                .with_child(Box::new(editor_row)),
        ),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    Box::new(ScrollView::new(Box::new(col)))
}

// ── Rust syntax highlighter ─────────────────────────────────────────────────

/// A minimal Rust keyword set — enough to light up the sample and typical code.
const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe",
    "use", "where", "while",
];

/// Per-line tokenizer producing `(start, end, colour)` runs for the given
/// line. Line-oriented (matches the [`agg_gui::TextArea`] highlighter
/// contract): `//` comments, `"…"` strings, numbers and keywords are all
/// resolvable without cross-line state.
fn rust_highlighter(line: &str) -> Vec<(usize, usize, Color)> {
    let mut out: Vec<(usize, usize, Color)> = Vec::new();
    let bytes = line.as_bytes();
    let len = line.len();
    let mut i = 0usize;
    while i < len {
        let c = line[i..].chars().next().unwrap();

        // Line comment: `// …` colours the rest of the line.
        if c == '/' && bytes.get(i + 1) == Some(&b'/') {
            out.push((i, len, color_comment()));
            break;
        }

        // String literal with minimal escape handling.
        if c == '"' {
            let start = i;
            i += 1;
            while i < len {
                let ch = line[i..].chars().next().unwrap();
                i += ch.len_utf8();
                if ch == '\\' {
                    if let Some(nc) = line.get(i..).and_then(|s| s.chars().next()) {
                        i += nc.len_utf8();
                    }
                } else if ch == '"' {
                    break;
                }
            }
            out.push((start, i, color_string()));
            continue;
        }

        // Number literal (digit-led run of alnum / `.` / `_`).
        if c.is_ascii_digit() {
            let start = i;
            while i < len {
                let ch = line[i..].chars().next().unwrap();
                if ch.is_alphanumeric() || ch == '.' || ch == '_' {
                    i += ch.len_utf8();
                } else {
                    break;
                }
            }
            out.push((start, i, color_number()));
            continue;
        }

        // Identifier / keyword.
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < len {
                let ch = line[i..].chars().next().unwrap();
                if ch.is_alphanumeric() || ch == '_' {
                    i += ch.len_utf8();
                } else {
                    break;
                }
            }
            let word = &line[start..i];
            if KEYWORDS.contains(&word) {
                out.push((start, i, color_keyword()));
            }
            continue;
        }

        i += c.len_utf8();
    }
    out
}

// ── Line-number gutter ──────────────────────────────────────────────────────

/// A slim gutter that paints right-aligned source-line numbers next to the
/// editor, matching its line height (`font_size * 1.35`) and top padding so the
/// numbers line up with the code. Reads the shared edit state for the current
/// line count so numbers track edits.
struct LineGutter {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    state: Rc<RefCell<TextEditState>>,
    font: Arc<Font>,
    font_size: f64,
    padding: f64,
    width: f64,
}

impl LineGutter {
    fn new(
        state: Rc<RefCell<TextEditState>>,
        font: Arc<Font>,
        font_size: f64,
        padding: f64,
    ) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            state,
            font,
            font_size,
            padding,
            width: 0.0,
        }
    }

    fn line_count(&self) -> usize {
        self.state.borrow().text.split('\n').count().max(1)
    }
}

impl Widget for LineGutter {
    fn type_name(&self) -> &'static str {
        "LineGutter"
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
        // Width fits the widest line number plus a small gutter on each side.
        let digits = self.line_count().to_string();
        let num_w = measure_text_metrics(&self.font, &digits, self.font_size).width;
        self.width = num_w + 16.0;
        let h = available.height.max(self.font_size * 1.6);
        self.bounds = Rect::new(0.0, 0.0, self.width, h);
        Size::new(self.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let line_h = self.font_size * 1.35;

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        let m = ctx.measure_text("Ag").unwrap_or_default();

        let count = self.line_count();
        let dim = Color::rgba(1.0, 1.0, 1.0, 0.32);
        ctx.set_fill_color(dim);
        for i in 0..count {
            let num = format!("{}", i + 1);
            let num_w = measure_text_metrics(&self.font, &num, self.font_size).width;
            // Right-align within the gutter, leaving an 8px right margin.
            let x = (w - 8.0 - num_w).max(0.0);
            let line_top = h - self.padding - i as f64 * line_h;
            let line_bottom = line_top - line_h;
            let baseline_y = line_bottom + (line_h - (m.ascent - m.descent)) * 0.5 + m.descent;
            ctx.fill_text(&num, x, baseline_y);
        }
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}
