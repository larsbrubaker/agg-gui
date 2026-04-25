//! Basic dialog-style demos: Undo/Redo and Window Options.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, Checkbox, DoUndoActions, FlexColumn, FlexRow, Font, Label, Separator, SizedBox,
    TextField, UndoBuffer, Widget,
};

/// Build the Undo Redo demo — a TextField plus usage instructions.
/// (TextField manages its own internal undo history via Ctrl+Z / Ctrl+Y.)
pub fn undo_redo(font: Arc<Font>) -> Box<dyn Widget> {
    let checkbox_value = Rc::new(Cell::new(false));
    let undoer = Rc::new(RefCell::new(UndoBuffer::new()));

    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(
        Box::new(Label::new("Undo Redo", Arc::clone(&font)).with_font_size(13.0)),
        0.0,
    );

    {
        let value_for_change = Rc::clone(&checkbox_value);
        let undoer_for_change = Rc::clone(&undoer);
        col.push(
            Box::new(
                Checkbox::new(
                    "Checkbox with undo/redo",
                    Arc::clone(&font),
                    checkbox_value.get(),
                )
                .with_font_size(13.0)
                .with_state_cell(Rc::clone(&checkbox_value))
                .on_change(move |new_value| {
                    let old_value = !new_value;
                    let redo_value = Rc::clone(&value_for_change);
                    let undo_value = Rc::clone(&value_for_change);
                    undoer_for_change
                        .borrow_mut()
                        .add(Box::new(DoUndoActions::new(
                            "toggle checkbox",
                            move || redo_value.set(new_value),
                            move || undo_value.set(old_value),
                        )));
                }),
            ),
            0.0,
        );
    }

    col.push(
        Box::new(
            SizedBox::new().with_height(34.0).with_child(Box::new(
                TextField::new(Arc::clone(&font))
                    .with_font_size(13.0)
                    .with_text("Text with undo/redo"),
            )),
        ),
        0.0,
    );

    let mut buttons = FlexRow::new().with_gap(8.0);
    {
        let undoer_for_enabled = Rc::clone(&undoer);
        let undoer_for_click = Rc::clone(&undoer);
        buttons.push(
            Box::new(
                Button::new("Undo", Arc::clone(&font))
                    .with_font_size(12.0)
                    .with_enabled_fn(move || undoer_for_enabled.borrow().can_undo())
                    .on_click(move || {
                        undoer_for_click.borrow_mut().undo();
                        agg_gui::animation::request_tick();
                    }),
            ),
            0.0,
        );
    }
    {
        let undoer_for_enabled = Rc::clone(&undoer);
        let undoer_for_click = Rc::clone(&undoer);
        buttons.push(
            Box::new(
                Button::new("Redo", Arc::clone(&font))
                    .with_font_size(12.0)
                    .with_enabled_fn(move || undoer_for_enabled.borrow().can_redo())
                    .on_click(move || {
                        undoer_for_click.borrow_mut().redo();
                        agg_gui::animation::request_tick();
                    }),
            ),
            0.0,
        );
    }
    col.push(Box::new(buttons), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    col.push(
        Box::new(Label::new("Keyboard shortcuts:", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );

    for line in [
        "Ctrl+Z         — undo last edit",
        "Ctrl+Y         — redo",
        "Ctrl+Shift+Z   — redo (alternate)",
        "Ctrl+A         — select all",
        "Ctrl+C / X / V — clipboard",
    ] {
        col.push(
            Box::new(Label::new(line, Arc::clone(&font)).with_font_size(12.0)),
            0.0,
        );
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(
            Label::new(
                "The buttons use agg-gui's shared UndoBuffer for the checkbox. The text field \
         keeps its own edit history for Ctrl+Z / Ctrl+Y, matching the command-history pattern \
         egui demonstrates with Undoer<State>.",
                Arc::clone(&font),
            )
            .with_font_size(11.0),
        ),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

/// Build the Window Options demo — checkboxes reflecting window capabilities.
pub fn window_options(font: Arc<Font>) -> Box<dyn Widget> {
    let resizable = Rc::new(Cell::new(true));
    let collapsible = Rc::new(Cell::new(true));
    let auto_sized = Rc::new(Cell::new(false));
    let anchored = Rc::new(Cell::new(false));

    let mut col = FlexColumn::new()
        .with_gap(14.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(
        Box::new(Label::new("Window options", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );

    for (label, cell) in [
        ("Resizable", Rc::clone(&resizable)),
        ("Collapsible", Rc::clone(&collapsible)),
        ("Auto-sized", Rc::clone(&auto_sized)),
        ("Anchored", Rc::clone(&anchored)),
    ] {
        let value = Rc::clone(&cell);
        col.push(
            Box::new(
                Checkbox::new(label, Arc::clone(&font), cell.get())
                    .with_font_size(13.0)
                    .on_change(move |b| value.set(b)),
            ),
            0.0,
        );
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(
            Label::new("Current window size: 360 \u{00d7} 290", Arc::clone(&font))
                .with_font_size(12.0),
        ),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}
