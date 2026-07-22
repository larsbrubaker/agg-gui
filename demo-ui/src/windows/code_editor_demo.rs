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
//!     highlight palette is theme-aware, though (see [`syntax_palette`]): it
//!     picks a dark or light token set from `ctx.visuals()` so the code stays
//!     legible under both app themes.
//!   * The gutter numbers source lines. Our `TextArea` always word-wraps, so a
//!     line long enough to wrap would push later numbers out of alignment; the
//!     sample fits the width, so this doesn't bite in practice.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    current_visuals, measure_text_metrics, Color, DrawCtx, Event, EventResult, FlexColumn, FlexRow,
    Font, HAnchor, Hyperlink, Label, Rect, Separator, Size, SizedBox, TextArea, TextEditState,
    Visuals, Widget,
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

// ── Highlight palette (theme-aware) ─────────────────────────────────────────

/// Token colours for the Rust highlighter. Resolved per-paint from the active
/// [`Visuals`] so the palette follows dark/light flips: the dark set mirrors the
/// original constants (tuned for the dark editor body), the light set is
/// darkened/saturated so keywords, strings and numbers stay legible against the
/// near-white editor background instead of washing out.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SyntaxPalette {
    keyword: Color,
    string: Color,
    comment: Color,
    number: Color,
}

fn syntax_palette(v: &Visuals) -> SyntaxPalette {
    if v.is_dark() {
        SyntaxPalette {
            keyword: Color::rgb(0.55, 0.63, 0.96),
            string: Color::rgb(0.68, 0.84, 0.5),
            comment: Color::rgb(0.48, 0.53, 0.6),
            number: Color::rgb(0.92, 0.66, 0.42),
        }
    } else {
        SyntaxPalette {
            keyword: Color::rgb(0.13, 0.28, 0.70),
            string: Color::rgb(0.20, 0.52, 0.24),
            comment: Color::rgb(0.45, 0.50, 0.58),
            number: Color::rgb(0.68, 0.38, 0.10),
        }
    }
}

/// Gutter colours (background, line-number text) for the active theme. Reading
/// these per-paint is what fixes the reported bug: the gutter used to be
/// transparent over a hard-coded dark column, so it stayed dark in the light
/// theme. `panel_fill` gives the conventional slightly-recessed gutter in both
/// themes and `text_dim` keeps the numbers legible against it.
fn gutter_colors(v: &Visuals) -> (Color, Color) {
    (v.panel_fill, v.text_dim)
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

    // Theme-aware panel background (read per-paint) instead of a hard-coded
    // dark fill, so the header strip behind the description follows the app
    // theme like the editor body does.
    let mut col = FlexColumn::new().with_gap(0.0).with_panel_bg();

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
                    .with_dim(true),
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

    // Fill the window's remaining height: the editor row takes all space left
    // after the fixed header + separator, so it grows and shrinks with the
    // window. The TextArea scrolls its contents internally, so the window never
    // needs an outer vertical scrollbar. (Previously this was pinned to a
    // fixed-height SizedBox inside a vertical ScrollView, which froze the
    // editor at 360 px regardless of window size.)
    col.push(Box::new(editor_row), 1.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    Box::new(col)
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
    // Resolve the palette from the live theme; the TextArea re-rasters its
    // backbuffer on a visuals-epoch flip, so this re-runs with the new palette.
    let pal = syntax_palette(&current_visuals());
    let mut out: Vec<(usize, usize, Color)> = Vec::new();
    let bytes = line.as_bytes();
    let len = line.len();
    let mut i = 0usize;
    while i < len {
        let c = line[i..].chars().next().unwrap();

        // Line comment: `// …` colours the rest of the line.
        if c == '/' && bytes.get(i + 1) == Some(&b'/') {
            out.push((i, len, pal.comment));
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
            out.push((start, i, pal.string));
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
            out.push((start, i, pal.number));
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
                out.push((start, i, pal.keyword));
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

        // Paint the gutter's own theme-aware background. Previously the gutter
        // was transparent and showed the (hard-coded dark) column behind it, so
        // it stayed dark even in the light theme — the reported bug.
        let (bg, num_color) = gutter_colors(&ctx.visuals());
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        let m = ctx.measure_text("Ag").unwrap_or_default();

        let count = self.line_count();
        ctx.set_fill_color(num_color);
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

#[cfg(test)]
mod scroll_bench;

#[cfg(test)]
mod tests {
    use super::*;
    use agg_gui::find_widget_by_type;

    /// Serialises tests that mutate the process-global visuals epoch (via
    /// `set_visuals`) against each other, mirroring the node-editor precedent
    /// (`node-editor/src/widget/tests.rs`). The pure-function colour tests below
    /// take an explicit `Visuals` and need no guard; only the end-to-end
    /// highlighter test flips the global palette.
    static VISUALS_EPOCH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn luminance(c: Color) -> f32 {
        0.299 * c.r + 0.587 * c.g + 0.114 * c.b
    }
    fn is_dark_color(c: Color) -> bool {
        luminance(c) < 0.5
    }

    fn test_font() -> Arc<Font> {
        const BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
        Arc::new(Font::from_slice(BYTES).expect("parse CascadiaCode.ttf"))
    }

    /// The gutter background and number colour must follow the active theme:
    /// dark-on-light in the light theme, light-on-dark in the dark theme. This
    /// is the regression guard for "the gutter is dark in both themes".
    #[test]
    fn gutter_colors_follow_theme() {
        let (dark_bg, dark_num) = gutter_colors(&Visuals::dark());
        let (light_bg, light_num) = gutter_colors(&Visuals::light());

        assert!(
            is_dark_color(dark_bg),
            "dark-theme gutter background must be dark: {dark_bg:?}"
        );
        assert!(
            !is_dark_color(light_bg),
            "light-theme gutter background must be light (this is the reported bug): {light_bg:?}"
        );
        // Numbers must contrast their own gutter background in each theme.
        assert!(
            !is_dark_color(dark_num),
            "dark-theme line numbers must be light: {dark_num:?}"
        );
        assert!(
            is_dark_color(light_num),
            "light-theme line numbers must be dark: {light_num:?}"
        );
    }

    /// The syntax palette must not reuse the dark constants in the light theme,
    /// and each light-theme token must be dark enough to read against the
    /// near-white editor background.
    #[test]
    fn syntax_palette_is_legible_in_light_theme() {
        let dark = syntax_palette(&Visuals::dark());
        let light = syntax_palette(&Visuals::light());

        assert_ne!(dark.keyword, light.keyword, "keyword colour must adapt");
        assert_ne!(dark.string, light.string, "string colour must adapt");
        assert_ne!(dark.number, light.number, "number colour must adapt");
        assert_ne!(dark.comment, light.comment, "comment colour must adapt");

        // Editor body in the light theme is ~white (`widget_bg`); every token
        // needs a comfortable luminance gap from it.
        let editor_bg = Visuals::light().widget_bg;
        for c in [light.keyword, light.string, light.number, light.comment] {
            assert!(
                luminance(editor_bg) - luminance(c) > 0.25,
                "light-theme token {c:?} is too pale for the white editor background"
            );
        }
    }

    /// The highlighter (a plain `fn` passed to `with_highlighter`) must resolve
    /// its palette from the live theme at call time, so a runtime dark/light
    /// flip repaints in the new colours. Mutates the global palette — guarded.
    #[test]
    fn rust_highlighter_follows_active_theme() {
        let _guard = VISUALS_EPOCH_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());

        // "// x" tokenises to a single comment run spanning the line.
        agg_gui::set_visuals(Visuals::light());
        let light_runs = rust_highlighter("// hi");
        assert_eq!(light_runs[0].2, syntax_palette(&Visuals::light()).comment);

        agg_gui::set_visuals(Visuals::dark());
        let dark_runs = rust_highlighter("// hi");
        assert_eq!(dark_runs[0].2, syntax_palette(&Visuals::dark()).comment);

        assert_ne!(
            light_runs[0].2, dark_runs[0].2,
            "comment colour must differ between themes"
        );

        // Restore a known default for other tests sharing the process.
        agg_gui::set_visuals(Visuals::dark());
    }

    fn text_area_height(content: &dyn Widget) -> f64 {
        find_widget_by_type(content, "TextArea")
            .expect("code editor content must contain a TextArea")
            .bounds()
            .height
    }

    /// The editor must stretch to fill the window's remaining height and track
    /// resizes — laying the content out at a taller available size must give the
    /// TextArea a taller bounds. Regression for the "editor stays 360 px tall"
    /// bug (the fixed-height SizedBox + outer ScrollView froze the height).
    #[test]
    fn editor_fills_window_height_and_tracks_resize() {
        let mut small = code_editor(test_font());
        small.layout(Size::new(600.0, 500.0));
        let h_small = text_area_height(small.as_ref());

        let mut large = code_editor(test_font());
        large.layout(Size::new(600.0, 800.0));
        let h_large = text_area_height(large.as_ref());

        assert!(
            h_large > h_small + 100.0,
            "TextArea must grow with the window height: {h_small} px at a 500 px \
             window vs {h_large} px at an 800 px window"
        );
    }

    /// The line-number gutter must stay the same height as the editor at any
    /// window size, so the numbers keep tracking the code.
    #[test]
    fn gutter_matches_editor_height() {
        let mut content = code_editor(test_font());
        content.layout(Size::new(600.0, 720.0));
        let editor_h = text_area_height(content.as_ref());
        let gutter_h = find_widget_by_type(content.as_ref(), "LineGutter")
            .expect("code editor content must contain a LineGutter")
            .bounds()
            .height;
        assert!(
            (gutter_h - editor_h).abs() < 1.0,
            "gutter height {gutter_h} must match editor height {editor_h}"
        );
    }
}
