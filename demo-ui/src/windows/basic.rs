//! Basic interactive widget demos: tooltips.
//!
//! Each function returns a `Box<dyn Widget>` ready to be placed inside a
//! floating `Window`.  Section-header labels carry no explicit color so they
//! follow `ctx.visuals().text_color` and remain readable in both dark and
//! light mode.
//!
//! The Sliders, TextEdit, and Code Editor demos moved to `sliders_demo.rs`,
//! `text_edit_demo.rs`, and `code_editor_demo.rs`; they are re-exported from
//! the `windows` module so `windows::sliders` / `windows::text_edit` /
//! `windows::code_editor` still resolve.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, Checkbox, FlexColumn, FlexRow, Font, HAnchor, Hyperlink, Label, ScrollView, Separator,
    SizedBox, Splitter, Tooltip, Widget,
};

// ---------------------------------------------------------------------------
// Tooltips demo
// ---------------------------------------------------------------------------

const TOOLTIP_SOURCE_URL: &str =
    "https://github.com/larsbrubaker/agg-gui/blob/main/demo-ui/src/windows/basic.rs";

/// Build the Tooltips demo — mirrors egui's tooltip stress/demo window.
pub fn tooltips(font: Arc<Font>) -> Box<dyn Widget> {
    let enabled = Rc::new(Cell::new(true));

    let left = tooltip_misc_tests(Arc::clone(&font), Rc::clone(&enabled));
    let right = tooltip_scroll_test(font);
    Box::new(
        Splitter::new(left, right)
            .with_ratio(0.62)
            .with_divider_width(4.0),
    )
}

fn tooltip_misc_tests(font: Arc<Font>, enabled: Rc<Cell<bool>>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(14.0)
        .with_panel_bg();

    col.push(source_link(Arc::clone(&font)), 0.0);

    col.push(
        tooltip_label(
            "All labels in this demo have tooltips.",
            "Yes, even this one.",
            Arc::clone(&font),
        ),
        0.0,
    );

    col.push(
        Box::new(
            Tooltip::new(
                Box::new(Label::new(
                    "Some widgets have multiple tooltips!",
                    Arc::clone(&font),
                )),
                "The first tooltip.",
                Arc::clone(&font),
            )
            .with_text("The second tooltip."),
        ),
        0.0,
    );

    // Interactive tooltip: hosts a real widget tree (a label + a hyperlink),
    // and the hyperlink itself carries its own nested tooltip — mirroring
    // egui's `on_hover_ui` + `hyperlink_to(...).on_hover_text(...)`.
    col.push(
        Box::new(
            Tooltip::new(
                Box::new(Label::new(
                    "Tooltips can contain interactive widgets.",
                    Arc::clone(&font),
                )),
                "unused (interactive content follows)",
                Arc::clone(&font),
            )
            .with_interactive_content(interactive_link_tip(Arc::clone(&font))),
        ),
        0.0,
    );

    col.push(
        tooltip_label(
            "You can put selectable text in tooltips too.",
            "You can select this text.",
            Arc::clone(&font),
        ),
        0.0,
    );

    col.push(
        Box::new(
            Tooltip::new(
                Box::new(Label::new(
                    "This tooltip shows at the mouse cursor.",
                    Arc::clone(&font),
                )),
                "Move me around!!",
                Arc::clone(&font),
            )
            .at_pointer(),
        ),
        0.0,
    );

    col.push(Box::new(Separator::horizontal()), 0.0);

    col.push(
        tooltip_label(
            "You can have different tooltips depending on whether or not a widget is enabled:",
            "Check the tooltip of the button below, and see how it changes depending on whether or not it is enabled.",
            Arc::clone(&font),
        ),
        0.0,
    );

    let mut row = FlexRow::new().with_gap(8.0);
    row.push(
        Box::new(Tooltip::new(
            Box::new(
                Checkbox::new("Enabled", Arc::clone(&font), enabled.get())
                    .with_state_cell(Rc::clone(&enabled)),
            ),
            "Controls whether or not the following button is enabled.",
            Arc::clone(&font),
        )),
        0.0,
    );

    let enabled_for_button = Rc::clone(&enabled);
    let enabled_for_tip = Rc::clone(&enabled);
    row.push(
        Box::new(
            Tooltip::new(
                Box::new(
                    Button::new("Sometimes clickable", Arc::clone(&font))
                        .with_font_size(13.0)
                        .with_enabled_fn(move || enabled_for_button.get())
                        .on_click(|| {}),
                ),
                "This tooltip was created with",
                Arc::clone(&font),
            )
            .with_code_line(".on_hover_ui(...)")
            .with_disabled_text(
                "A different tooltip when widget is disabled.\nThis tooltip was created with\n.on_disabled_hover_ui(...)",
                move || !enabled_for_tip.get(),
            ),
        ),
        0.0,
    );
    col.push(Box::new(row), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

fn tooltip_scroll_test(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(10.0)
        .with_panel_bg();
    col.push(
        Box::new(Tooltip::new(
            Box::new(
                Label::new(
                    "The scroll area below has many labels with interactive tooltips. The purpose is to test that the tooltips close when you scroll.",
                    Arc::clone(&font),
                )
                .with_font_size(12.0)
                .with_wrap(true),
            ),
            "Try hovering a label below, then scroll!",
            Arc::clone(&font),
        )),
        0.0,
    );

    let mut lines = FlexColumn::new().with_gap(2.0);
    for i in 0..1000 {
        lines.push(
            Box::new(
                Tooltip::new(
                    Box::new(Label::new(format!("This is line {i}"), Arc::clone(&font))),
                    "This tooltip is interactive, because the text in it is selectable.",
                    Arc::clone(&font),
                )
                .with_margin(agg_gui::Insets::from_sides(0.0, 0.0, 1.0, 1.0)),
            ),
            0.0,
        );
    }
    col.push(Box::new(ScrollView::new(Box::new(lines))), 1.0);
    Box::new(col)
}

/// Content for the interactive tooltip: a label plus a hyperlink that itself
/// carries a nested tooltip ("The tooltip has a tooltip in it!").
fn interactive_link_tip(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new().with_gap(4.0);
    col.push(
        Box::new(
            Label::new("This tooltip contains a link:", Arc::clone(&font)).with_font_size(12.0),
        ),
        0.0,
    );
    let link = Box::new(
        Hyperlink::new("github.com/larsbrubaker/agg-gui", Arc::clone(&font))
            .with_font_size(12.0)
            .on_click(|| crate::url::open_url("https://github.com/larsbrubaker/agg-gui")),
    );
    col.push(
        Box::new(Tooltip::new(link, "The tooltip has a tooltip in it!", font).at_widget()),
        0.0,
    );
    Box::new(col)
}

fn tooltip_label(label: &'static str, tip: &'static str, font: Arc<Font>) -> Box<dyn Widget> {
    Box::new(Tooltip::new(
        Box::new(Label::new(label, Arc::clone(&font))),
        tip,
        font,
    ))
}

fn source_link(font: Arc<Font>) -> Box<dyn Widget> {
    Box::new(
        Hyperlink::new("(source code)", font)
            .with_font_size(11.0)
            .with_h_anchor(HAnchor::CENTER)
            .on_click(|| crate::url::open_url(TOOLTIP_SOURCE_URL)),
    )
}
