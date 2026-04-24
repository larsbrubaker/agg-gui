//! Basic interactive widget demos: sliders, text editing, tooltips, and code
//! editor.
//!
//! Each function returns a `Box<dyn Widget>` ready to be placed inside a
//! floating `Window`.  Section-header labels carry no explicit color so they
//! follow `ctx.visuals().text_color` and remain readable in both dark and
//! light mode.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, Color, FlexColumn, FlexRow, Font, Label, ScrollView, Separator, SizedBox, Slider,
    TextField, Tooltip, Widget,
};

// ---------------------------------------------------------------------------
// Sliders demo
// ---------------------------------------------------------------------------

/// Build the Sliders demo — four sliders with different ranges and step sizes
/// demonstrating the full flexibility of the `Slider` widget.
pub fn sliders(font: Arc<Font>) -> Box<dyn Widget> {
    let v0 = Rc::new(Cell::new(0.5_f64));
    let v1 = Rc::new(Cell::new(25.0_f64));
    let v2 = Rc::new(Cell::new(0.001_f64));
    let v3 = Rc::new(Cell::new(0.75_f64));

    let mut col = FlexColumn::new()
        .with_gap(18.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(
        Box::new(Label::new("Float  0.0 → 1.0", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );
    {
        let sv = Rc::clone(&v0);
        col.push(
            Box::new(
                Slider::new(v0.get(), 0.0, 1.0, Arc::clone(&font))
                    .with_step(0.01)
                    .on_change(move |v| sv.set(v)),
            ),
            0.0,
        );
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(Label::new("Integer  0 → 100", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );
    {
        let sv = Rc::clone(&v1);
        col.push(
            Box::new(
                Slider::new(v1.get(), 0.0, 100.0, Arc::clone(&font))
                    .with_step(1.0)
                    .on_change(move |v| sv.set(v)),
            ),
            0.0,
        );
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(Label::new("Small step  0.0001 → 0.01", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );
    {
        let sv = Rc::clone(&v2);
        col.push(
            Box::new(
                Slider::new(v2.get(), 0.0001, 0.01, Arc::clone(&font))
                    .with_step(0.0001)
                    .on_change(move |v| sv.set(v)),
            ),
            0.0,
        );
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(Label::new("Clamped range  0.25 → 0.75", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );
    {
        let sv = Rc::clone(&v3);
        col.push(
            Box::new(
                Slider::new(v3.get(), 0.25, 0.75, Arc::clone(&font))
                    .with_step(0.005)
                    .on_change(move |v| sv.set(v)),
            ),
            0.0,
        );
    }

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Text Edit demo
// ---------------------------------------------------------------------------

/// Build the Text Edit demo — three `TextField` variants: empty, pre-filled,
/// and read-only.
pub fn text_edit(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(14.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(
        Box::new(Label::new("Single-line", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );
    col.push(
        Box::new(
            SizedBox::new().with_height(32.0).with_child(Box::new(
                TextField::new(Arc::clone(&font))
                    .with_font_size(13.0)
                    .with_placeholder("Click to edit…"),
            )),
        ),
        0.0,
    );

    col.push(
        Box::new(Label::new("With initial text", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );
    col.push(
        Box::new(
            SizedBox::new().with_height(32.0).with_child(Box::new(
                TextField::new(Arc::clone(&font))
                    .with_font_size(13.0)
                    .with_text("Hello, world!"),
            )),
        ),
        0.0,
    );

    col.push(
        Box::new(Label::new("Read-only", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );
    col.push(
        Box::new(
            SizedBox::new().with_height(32.0).with_child(Box::new(
                TextField::new(Arc::clone(&font))
                    .with_font_size(13.0)
                    .with_text("This field is read-only")
                    .with_read_only(true),
            )),
        ),
        0.0,
    );

    col.push(
        Box::new(
            Label::new(
                "Ctrl+A select all • Ctrl+C/X/V clipboard • Home/End • Shift+arrows",
                Arc::clone(&font),
            )
            .with_font_size(11.0),
        ),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Tooltips demo
// ---------------------------------------------------------------------------

/// Build the Tooltips demo — buttons demonstrating hover-over tooltip behavior.
///
/// Each button is wrapped in a [`Tooltip`] widget that shows a small info panel
/// after the cursor rests over it for ~0.5 s.
pub fn tooltips(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(14.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(
        Box::new(
            Label::new(
                "Tooltip demos — hover a button to see its tip",
                Arc::clone(&font),
            )
            .with_font_size(12.0),
        ),
        0.0,
    );

    let tips = [
        ("Hover me (A)", "This is tooltip A.\nHover delay: ~0.5 s"),
        ("Hover me (B)", "Tooltip B: click to activate the button"),
        (
            "Hover me (C)",
            "Tooltip C: the panel follows the cursor position",
        ),
    ];

    for (label, tip) in tips {
        let btn = Box::new(
            Button::new(label, Arc::clone(&font))
                .with_font_size(13.0)
                .on_click(|| {}),
        );
        let wrapped = Tooltip::new(btn, tip, Arc::clone(&font));
        col.push(
            Box::new(
                SizedBox::new()
                    .with_height(30.0)
                    .with_child(Box::new(wrapped)),
            ),
            0.0,
        );
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(
            Label::new(
                "Tooltips are rendered inline within the widget's local space.\n\
         A future global overlay layer will allow true floating tooltips.",
                Arc::clone(&font),
            )
            .with_font_size(11.0),
        ),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Code Editor demo
// ---------------------------------------------------------------------------

/// Build the Code Editor demo — a dark-themed, read-only source view with
/// line numbers and a command bar at the bottom.
///
/// Each line is rendered as a `FlexRow` of two `Label`s (line-number gutter +
/// code text) so the layout engine handles wrapping and sizing automatically.
pub fn code_editor(font: Arc<Font>) -> Box<dyn Widget> {
    const SAMPLE: &str = "\
fn main() {\n\
    let greeting = \"Hello, agg-gui!\";\n\
    println!(\"{}\", greeting);\n\
\n\
    let values: Vec<f64> = (0..10)\n\
        .map(|i| i as f64 * 0.1)\n\
        .collect();\n\
\n\
    for (i, v) in values.iter().enumerate() {\n\
        println!(\"[{i}] {v:.2}\");\n\
    }\n\
}";

    let bg = Color::rgb(0.12, 0.13, 0.15);
    let mut col = FlexColumn::new().with_gap(0.0).with_background(bg);

    col.push(
        Box::new(
            Label::new("main.rs", Arc::clone(&font))
                .with_font_size(11.0)
                .with_color(Color::rgba(1.0, 1.0, 1.0, 0.45)),
        ),
        0.0,
    );
    col.push(Box::new(Separator::horizontal()), 0.0);

    // Render each line as a label — simple but effective without a real editor widget.
    for (i, line) in SAMPLE.lines().enumerate() {
        let line_num = format!("{:>3}  ", i + 1);
        let row = FlexRow::new()
            .with_gap(0.0)
            .add(Box::new(
                Label::new(line_num, Arc::clone(&font))
                    .with_font_size(12.5)
                    .with_color(Color::rgba(1.0, 1.0, 1.0, 0.22)),
            ))
            .add(Box::new(
                Label::new(line, Arc::clone(&font))
                    .with_font_size(12.5)
                    .with_color(Color::rgba(0.85, 0.90, 0.95, 1.0)),
            ));
        col.push(Box::new(row), 0.0);
    }

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    // Editable single-line command bar at the bottom.
    let bar = FlexRow::new()
        .with_gap(8.0)
        .add(Box::new(
            Label::new(">", Arc::clone(&font))
                .with_font_size(13.0)
                .with_color(Color::rgb(0.4, 0.8, 0.4)),
        ))
        .add_flex(
            Box::new(
                SizedBox::new().with_height(28.0).with_child(Box::new(
                    TextField::new(Arc::clone(&font))
                        .with_font_size(13.0)
                        .with_placeholder("command…"),
                )),
            ),
            1.0,
        );
    col.push(Box::new(bar), 0.0);

    Box::new(ScrollView::new(Box::new(col)))
}
