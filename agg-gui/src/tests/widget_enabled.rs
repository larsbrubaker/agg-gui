//! `Widget::is_enabled` (C# `GuiWidget.Enabled`, the widget's own flag):
//! true by default, and the core widgets that can be disabled report their
//! live state through it, so code holding a `&dyn Widget` (GUI automation's
//! `WaitForWidgetEnabled`) can ask.

use super::*;
use crate::widgets::ChevronWidget;
use crate::SegmentedControl;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::text::Font;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

#[test]
fn a_widget_without_an_enabled_state_is_enabled() {
    let plain: Box<dyn Widget> = Box::new(SizedBox::new());
    assert!(plain.is_enabled());
}

#[test]
fn a_button_reports_its_enabled_predicate() {
    let enabled = Rc::new(Cell::new(true));
    let flag = Rc::clone(&enabled);
    let button: Box<dyn Widget> =
        Box::new(Button::new("ok", font()).with_enabled_fn(move || flag.get()));

    assert!(button.is_enabled());
    enabled.set(false);
    assert!(!button.is_enabled());
}

#[test]
fn a_segmented_control_reports_its_enabled_predicate() {
    let enabled = Rc::new(Cell::new(false));
    let flag = Rc::clone(&enabled);
    let control: Box<dyn Widget> = Box::new(
        SegmentedControl::new(vec!["a", "b"], Rc::new(Cell::new(0)), font())
            .with_enabled_fn(move || flag.get()),
    );

    assert!(!control.is_enabled());
    enabled.set(true);
    assert!(control.is_enabled());
}

#[test]
fn a_chevron_reports_its_shared_enabled_cell() {
    let enabled = Rc::new(Cell::new(true));
    let chevron: Box<dyn Widget> = Box::new(
        ChevronWidget::new(Rc::new(Cell::new(false))).with_enabled_cell(Rc::clone(&enabled)),
    );

    assert!(chevron.is_enabled());
    enabled.set(false);
    assert!(!chevron.is_enabled());
}
