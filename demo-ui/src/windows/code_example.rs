//! Code Example demo window.
//!
//! Mirrors the egui "Code Example" demo: each row shows a syntax-colored code
//! snippet on the left and its live rendered output on the right.  The `age`
//! value is shared between a `DragValue`, an increment `Button`, and a custom
//! `AgeDisplay` widget so all three stay in sync.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, CollapsingHeader, Color, DragValue, DrawCtx, Event, EventResult, FlexColumn, FlexRow,
    Font, Insets, Label, Rect, ScrollView, Separator, Size, SizedBox, TextField, Widget,
};

// ---------------------------------------------------------------------------
// AgeDisplay — custom widget
// ---------------------------------------------------------------------------

/// A widget that reads the age from a shared `Cell` on every layout call and
/// updates a backbuffered [`Label`] child so the displayed value stays in sync
/// without raw `fill_text` calls.
pub struct AgeDisplay {
    pub bounds: Rect,
    pub children: Vec<Box<dyn Widget>>,
    pub name: Rc<RefCell<String>>,
    pub age: Rc<Cell<u32>>,
    debug_repr: bool,
}

impl AgeDisplay {
    fn new(name: Rc<RefCell<String>>, age: Rc<Cell<u32>>, font: Arc<Font>) -> Self {
        let text = format!("{} is {}", name.borrow(), age.get());
        Self {
            bounds: Rect::default(),
            children: vec![Box::new(Label::new(text, font).with_font_size(13.0))],
            name,
            age,
            debug_repr: false,
        }
    }

    fn debug(name: Rc<RefCell<String>>, age: Rc<Cell<u32>>, font: Arc<Font>) -> Self {
        let text = format!(
            "CodeExample {{ name: \"{}\", age: {} }}",
            name.borrow(),
            age.get()
        );
        Self {
            bounds: Rect::default(),
            children: vec![Box::new(Label::new(text, font).with_font_size(11.0))],
            name,
            age,
            debug_repr: true,
        }
    }

    fn current_text(&self) -> String {
        if self.debug_repr {
            format!(
                "CodeExample {{ name: \"{}\", age: {} }}",
                self.name.borrow(),
                self.age.get()
            )
        } else {
            format!("{} is {}", self.name.borrow(), self.age.get())
        }
    }
}

impl Widget for AgeDisplay {
    fn type_name(&self) -> &'static str {
        "AgeDisplay"
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
        // Refresh the live text on the Label child — `set_label_text`
        // only invalidates Label's glyph cache when the value changed.
        let text = self.current_text();
        let s = if let Some(child) = self.children.first_mut() {
            child.set_label_text(&text);
            child.layout(Size::new(available.width, 20.0))
        } else {
            Size::new(0.0, 20.0)
        };
        let h = s.height.max(20.0);
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        if let Some(child) = self.children.first_mut() {
            child.set_bounds(Rect::new(0.0, (h - s.height) * 0.5, s.width, s.height));
        }
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let color = ctx.visuals().text_color;
        if let Some(child) = self.children.first_mut() {
            child.set_label_color(color);
        }
        // Label child paints itself via the framework's tree walk.
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ---------------------------------------------------------------------------
// code_example builder
// ---------------------------------------------------------------------------

/// Build the Code Example demo — side-by-side code snippets and live output.
pub fn code_example(font: Arc<Font>) -> Box<dyn Widget> {
    // Shared state.
    let name = Rc::new(RefCell::new("Arthur".to_string()));
    let age = Rc::new(Cell::new(42_u32));

    // Code snippet color palette (dark-theme syntax colors).
    let kw = Color::rgb(0.56, 0.74, 0.95); // blue — keywords / types
    let fn_ = Color::rgb(0.86, 0.78, 0.55); // gold — function names
    let str_ = Color::rgb(0.82, 0.60, 0.45); // orange — string literals
    let dim = Color::rgba(1.0, 1.0, 1.0, 0.38);
    let fg = Color::rgba(0.88, 0.90, 0.93, 1.0);
    let code_bg = Color::rgb(0.12, 0.13, 0.15);

    // Suppress unused-variable warning — kw is here for completeness with the
    // palette even though this demo currently uses only a subset of colors.
    let _ = kw;

    /// Build a one-line syntax-colored code label.
    fn code_line(text: &str, color: Color, font: &Arc<Font>) -> Box<dyn Widget> {
        Box::new(
            Label::new(text, Arc::clone(font))
                .with_font_size(11.5)
                .with_color(color),
        )
    }

    /// Wrap code lines in a dark-bg padded box.
    fn code_box(lines: Vec<Box<dyn Widget>>, code_bg: Color) -> Box<dyn Widget> {
        let mut col = FlexColumn::new().with_gap(0.0).with_background(code_bg);
        for l in lines {
            col.push(l, 0.0);
        }
        Box::new(
            SizedBox::new()
                .with_margin(Insets::all(4.0))
                .with_child(Box::new(col)),
        )
    }

    /// One row: [code box (fixed 210px)] | [gap] | [output widget].
    fn row(code: Box<dyn Widget>, output: Box<dyn Widget>) -> Box<dyn Widget> {
        Box::new(
            FlexRow::new()
                .with_gap(12.0)
                .add(Box::new(SizedBox::new().with_width(210.0).with_child(code)))
                .add_flex(output, 1.0),
        )
    }

    let mut col = FlexColumn::new()
        .with_gap(10.0)
        .with_padding(12.0)
        .with_panel_bg();

    // ── Heading row ───────────────────────────────────────────────────────────
    col.push(
        row(
            code_box(
                vec![code_line("ui.heading(\"Example\");", fg, &font)],
                code_bg,
            ),
            Box::new(Label::new("Example", Arc::clone(&font)).with_font_size(18.0)),
        ),
        0.0,
    );

    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── Name text field row ───────────────────────────────────────────────────
    col.push(
        row(
            code_box(
                vec![
                    code_line("FlexRow::new()", fg, &font),
                    code_line("  .add(Label::new(", fg, &font),
                    code_line("    \"Name\", font))", dim, &font),
                    code_line("  .add(TextField::new(font))", fn_, &font),
                ],
                code_bg,
            ),
            Box::new(
                FlexRow::new()
                    .with_gap(8.0)
                    .add(Box::new(
                        Label::new("Name", Arc::clone(&font)).with_font_size(13.0),
                    ))
                    .add_flex(
                        Box::new(
                            SizedBox::new().with_height(28.0).with_child(Box::new(
                                TextField::new(Arc::clone(&font))
                                    .with_font_size(13.0)
                                    .with_text_cell(Rc::clone(&name)),
                            )),
                        ),
                        1.0,
                    ),
            ),
        ),
        0.0,
    );

    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── Age drag-value row ────────────────────────────────────────────────────
    {
        let age2 = Rc::clone(&age);
        col.push(
            row(
                code_box(
                    vec![
                        code_line("DragValue::new(", fn_, &font),
                        code_line("    age, 0.0, 120.0, font)", fg, &font),
                        code_line("    // suffix: \" years\"", dim, &font),
                    ],
                    code_bg,
                ),
                Box::new(
                    FlexRow::new()
                        .with_gap(6.0)
                        .add(Box::new(
                            SizedBox::new()
                                .with_width(90.0)
                                .with_height(28.0)
                                .with_child(Box::new(
                                    DragValue::new(age.get() as f64, 0.0, 120.0, Arc::clone(&font))
                                        .with_decimals(0)
                                        .on_change(move |v| age2.set(v as u32)),
                                )),
                        ))
                        .add(Box::new(
                            Label::new("years", Arc::clone(&font)).with_font_size(13.0),
                        )),
                ),
            ),
            0.0,
        );
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── Increment button row ──────────────────────────────────────────────────
    {
        let age3 = Rc::clone(&age);
        col.push(
            row(
                code_box(
                    vec![
                        code_line("if Button::new(", fn_, &font),
                        code_line("    \"Increment\", font)", str_, &font),
                        code_line("    .on_click(|| *age += 1)", dim, &font),
                    ],
                    code_bg,
                ),
                Box::new(
                    SizedBox::new().with_height(28.0).with_child(Box::new(
                        Button::new("Increment", Arc::clone(&font))
                            .with_font_size(13.0)
                            .on_click(move || {
                                age3.set(age3.get().saturating_add(1));
                            }),
                    )),
                ),
            ),
            0.0,
        );
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── Dynamic age label row ─────────────────────────────────────────────────
    col.push(
        row(
            code_box(
                vec![code_line(
                    "ui.label(format!(\"{name} is {age}\"));",
                    fg,
                    &font,
                )],
                code_bg,
            ),
            Box::new(AgeDisplay::new(
                Rc::clone(&name),
                Rc::clone(&age),
                Arc::clone(&font),
            )),
        ),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(
            CollapsingHeader::new("Theme", Arc::clone(&font))
                .default_open(false)
                .with_content(Box::new(
                    Label::new(
                        "Syntax highlighting is represented by the fixed dark code palette in this agg-gui demo.",
                        Arc::clone(&font),
                    )
                    .with_font_size(12.0)
                    .with_wrap(true),
                )),
        ),
        0.0,
    );
    col.push(
        Box::new(AgeDisplay::debug(name, age, Arc::clone(&font))),
        0.0,
    );

    Box::new(ScrollView::new(Box::new(col)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_font() -> Arc<Font> {
        const BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
        Arc::new(Font::from_slice(BYTES).expect("parse CascadiaCode.ttf"))
    }

    #[test]
    fn age_display_uses_live_name_and_age_state() {
        let name = Rc::new(RefCell::new("Arthur".to_string()));
        let age = Rc::new(Cell::new(42_u32));
        let mut display = AgeDisplay::new(Rc::clone(&name), Rc::clone(&age), test_font());

        display.layout(Size::new(240.0, 40.0));
        assert_eq!(display.label.text_str(), "Arthur is 42");

        *name.borrow_mut() = "Ford".to_string();
        age.set(43);
        display.layout(Size::new(240.0, 40.0));
        assert_eq!(display.label.text_str(), "Ford is 43");
    }
}
