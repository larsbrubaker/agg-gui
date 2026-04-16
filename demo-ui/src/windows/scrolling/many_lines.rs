//! Many lines tab: row-count slider over a `RowList` that skips off-viewport
//! rows via the shared viewport cell.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    FlexColumn, FlexRow, Font, Rect, ScrollView, Separator, SizedBox,
    Slider, Widget,
};

use super::helpers::{label, wrapped_label, LiveLabel, RowList};

pub fn build(font: Arc<Font>) -> Box<dyn Widget> {
    let row_count = Rc::new(Cell::new(10_000_usize));
    let viewport  = Rc::new(Cell::new(Rect::default()));

    let mut col = FlexColumn::new().with_gap(6.0).with_padding(10.0);
    col.push(wrapped_label(Arc::clone(&font),
        "A lot of rows, but only the visible ones are painted — the row list \
         reads the ScrollView's viewport rect each frame and skips everything \
         outside it.  Even at 10 000 rows the per-frame text cost is constant.",
        11.0), 0.0);

    let count_cb = Rc::clone(&row_count);
    let count_for_label = Rc::clone(&row_count);
    col.push(Box::new(FlexRow::new().with_gap(8.0)
        .add(label(Arc::clone(&font), "Row count", 12.0))
        .add_flex(Box::new(
            Slider::new(10_000.0, 10.0, 100_000.0, Arc::clone(&font))
                .with_step(100.0)
                .on_change(move |v| count_cb.set(v.round() as usize))
        ), 1.0)
        .add(Box::new(SizedBox::new().with_width(8.0)))
        .add(Box::new(LiveLabel::new(
            Arc::clone(&font),
            Rc::new(move || format!("{}", count_for_label.get())),
        ).with_font_size(12.0)))), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    let count_for_row = Rc::clone(&row_count);
    let list = RowList::new(
        Arc::clone(&font),
        Rc::clone(&row_count),
        Rc::new(move |i| format!("This is row {}/{}", i + 1, count_for_row.get())),
    ).with_viewport_cell(Rc::clone(&viewport));

    let scroll = ScrollView::new(Box::new(list))
        .with_viewport_cell(Rc::clone(&viewport));
    col.push(Box::new(scroll), 1.0);

    Box::new(col)
}
