//! `MouseInteractionTests.RadioButtonSiblingsAreChildren`, split out of
//! `mouse_interaction_tests.rs` (the class's shared helpers live there).
//!
//! C#'s `FlowLayoutWidget(TopToBottom)` fitted to its children at the
//! window's top left is a fit-width [`FlexColumn`] laid out at its natural
//! size and placed there. C#'s click handler reads every button's `Checked`
//! through `buttonContainer.Children`; here each button mirrors its state in
//! a cell the handler reads, and a wrong state panics as C#'s throws.

use super::*;

use agg_gui::widgets::{FlexColumn, RadioButton};

#[test]
fn radio_button_siblings_are_children() {
    let button_count = 5;
    show_window_and_execute_tests(
        RunOptions {
            secs_to_test_failure: 30.0,
            ..RunOptions::default()
        },
        move || {
            let mut button_window = AutomationWindow::new(300.0, 200.0);

            let checked: Vec<Rc<Cell<bool>>> = (0..button_count)
                .map(|_| Rc::new(Cell::new(false)))
                .collect();
            let mut button_container = FlexColumn::new().with_fit_width(true);

            for i in 0..button_count {
                let index = i;
                let states = checked.clone();
                let radio_button = RadioButton::new(format!("Button {i}"), ui_font())
                    .with_state_cell(Rc::clone(&checked[i]))
                    .on_click(move || {
                        for (j, state) in states.iter().enumerate() {
                            if j == index {
                                if !state.get() {
                                    panic!("Button {j} should be checked");
                                }
                            } else if state.get() {
                                panic!("Button {j} should not be checked");
                            }
                        }
                    })
                    .with_name(format!("button {i}"));
                button_container.push(Box::new(radio_button), 0.0);
            }

            // HAnchor.Fit | Left, VAnchor.Fit | Top.
            let size = button_container.layout(Size::new(300.0, 200.0));
            button_container.set_bounds(Rect::new(
                0.0,
                200.0 - size.height,
                size.width,
                size.height,
            ));
            button_window.add_child(Box::new(button_container));
            (button_window, ())
        },
        move |test_runner, ()| {
            test_runner.config.time_to_move_mouse = 0.1;

            for i in 0..button_count {
                test_runner.click_by_name(&format!("button {i}"));
                test_runner.delay(0.5);
            }

            for i in (0..button_count).rev() {
                test_runner.click_by_name(&format!("button {i}"));
                test_runner.delay(0.5);
            }

            test_runner.mark_test_complete();
        },
    )
    .expect("checking a radio button unchecks its siblings");
}
