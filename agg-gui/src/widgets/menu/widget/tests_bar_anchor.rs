//! `MenuBar::with_h_anchor`: the bar reports the horizontal anchor it was
//! given (C# `MenuBar { HAnchor = HAnchor.Stretch }`), so a parent column
//! stretches a fit-width bar across its whole row without a wrapper widget.

use crate::geometry::Size;
use crate::input_profile::{profile_test_lock, set_input_profile, InputProfile};
use crate::layout_props::HAnchor;
use crate::widget::Widget;
use crate::widgets::flex::FlexColumn;

use super::super::model::MenuItem;
use super::tests_conformance::test_font;
use super::{MenuBar, TopMenu};

fn fit_bar() -> MenuBar {
    MenuBar::new(
        test_font(),
        vec![TopMenu::new(
            "File",
            vec![MenuItem::action("New", "New").into()],
        )],
        |_| {},
    )
    .with_fit_width(true)
}

#[test]
fn a_menu_bar_fits_by_default() {
    assert_eq!(fit_bar().h_anchor(), HAnchor::FIT);
}

#[test]
fn a_stretched_menu_bar_reports_stretch_and_fills_its_column() {
    let _lock = profile_test_lock();
    set_input_profile(InputProfile::Desktop);
    let bar = fit_bar().with_h_anchor(HAnchor::STRETCH);
    assert_eq!(bar.h_anchor(), HAnchor::STRETCH);
    let mut column = FlexColumn::new().with_gap(0.0).add(Box::new(bar));
    column.layout(Size::new(400.0, 300.0));
    assert_eq!(column.children()[0].bounds().width, 400.0);

    // The same bar left at FIT keeps its titles' width.
    let mut column = FlexColumn::new().with_gap(0.0).add(Box::new(fit_bar()));
    column.layout(Size::new(400.0, 300.0));
    let width = column.children()[0].bounds().width;
    assert!(width > 0.0 && width < 400.0, "fit bar width {width}");
}
