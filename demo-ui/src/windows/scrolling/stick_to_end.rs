//! Stick to end tab: row count grows every layout frame, ScrollView
//! `with_stick_to_bottom(true)` keeps the bar glued to the tail unless the
//! user scrolls away manually.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{FlexColumn, Font, ScrollView, Widget};

use super::helpers::{wrapped_label, CounterTicker, RowList};

pub fn build(font: Arc<Font>) -> Box<dyn Widget> {
    let counter = Rc::new(Cell::new(20_usize));

    let mut col = FlexColumn::new().with_gap(6.0).with_padding(10.0);
    col.push(
        wrapped_label(
            Arc::clone(&font),
            "Rows enter from the bottom every layout pass; the scrollbar stays \
         glued to the end unless you scroll away.  Scroll up to detach; \
         return to the bottom to re-attach.",
            11.0,
        ),
        0.0,
    );

    // Ticker layouts before the ScrollView so the row count is current.
    col.push(Box::new(CounterTicker::new(Rc::clone(&counter))), 0.0);

    let list = RowList::new(
        Arc::clone(&font),
        Rc::clone(&counter),
        Rc::new(|i| format!("This is row {}", i + 1)),
    );
    let scroll = ScrollView::new(Box::new(list)).with_stick_to_bottom(true);
    col.push(Box::new(scroll), 1.0);

    Box::new(col)
}
