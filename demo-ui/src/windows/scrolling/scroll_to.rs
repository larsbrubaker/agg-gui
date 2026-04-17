//! Scroll to tab: programmatic scroll by index / offset / delta with alignment.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, DragValue, FlexColumn, FlexRow, Font, Rect,
    ScrollView, Separator, SizedBox, Slider, Widget,
};

use super::helpers::{
    label, wrapped_label, LiveLabel, MaxScrollWatcher, OffsetReadout, RowList, SegRow,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScrollAlign { Top, Center, Bottom }

pub fn build(font: Arc<Font>) -> Box<dyn Widget> {
    let num_items   = 500usize;
    let row_height  = 18.0f64;
    let track_item  = Rc::new(Cell::new(25_usize));
    let align       = Rc::new(Cell::new(ScrollAlign::Center));
    let scroll_off  = Rc::new(Cell::new(0.0f64));
    let max_scroll  = Rc::new(Cell::new(0.0f64));
    let highlight   = Rc::new(Cell::new(Some(24_usize)));
    let delta_px    = Rc::new(Cell::new(64.0f64));

    let mut col = FlexColumn::new().with_gap(6.0).with_padding(10.0);

    col.push(wrapped_label(Arc::clone(&font),
        "Scroll to a specific item index or pixel offset.  The tracked item \
         is highlighted; moving the slider or changing the alignment re-scrolls \
         the list so the item lands in the chosen position.", 11.0), 0.0);

    // Build recompute closure once; both slider + align reuse it.
    let ti_cb = Rc::clone(&track_item);
    let hl_cb = Rc::clone(&highlight);
    let so_cb = Rc::clone(&scroll_off);
    let ms_cb = Rc::clone(&max_scroll);
    let al_cb = Rc::clone(&align);
    let recompute = Rc::new(move || {
        let i = ti_cb.get().saturating_sub(1);
        hl_cb.set(Some(i));
        let content_h = (num_items as f64) * row_height;
        let max       = ms_cb.get();
        let viewport  = (content_h - max).max(row_height);
        let item_top  = (i as f64) * row_height;
        let target = match al_cb.get() {
            ScrollAlign::Top    => item_top,
            ScrollAlign::Center => item_top - (viewport - row_height) * 0.5,
            ScrollAlign::Bottom => item_top - viewport + row_height,
        };
        so_cb.set(target.clamp(0.0, max));
    });

    // ── Track item slider ──
    {
        let r = Rc::clone(&recompute);
        let ti = Rc::clone(&track_item);
        let ti_for_readout = Rc::clone(&track_item);
        col.push(Box::new(FlexRow::new().with_gap(8.0)
            .add(label(Arc::clone(&font), "Track item", 12.0))
            .add_flex(Box::new(
                Slider::new(25.0, 1.0, num_items as f64, Arc::clone(&font))
                    .with_step(1.0)
                    .on_change(move |v| {
                        ti.set(v.round() as usize);
                        r();
                    })
            ), 1.0)
            .add(Box::new(SizedBox::new().with_width(8.0)))
            .add(Box::new(LiveLabel::new(
                Arc::clone(&font),
                Rc::new(move || format!("{}", ti_for_readout.get())),
            ).with_font_size(12.0)))), 0.0);
    }

    // ── Alignment segmented ──
    {
        let r = Rc::clone(&recompute);
        col.push(Box::new(FlexRow::new().with_gap(8.0)
            .add(label(Arc::clone(&font), "Align", 12.0))
            .add_flex(Box::new(
                SegRow::new(
                    Arc::clone(&font),
                    vec![
                        ("Top",    ScrollAlign::Top),
                        ("Center", ScrollAlign::Center),
                        ("Bottom", ScrollAlign::Bottom),
                    ],
                    Rc::clone(&align),
                ).on_change(move || r())
            ), 1.0)), 0.0);
    }

    // ── Offset DragValue ──
    {
        let so = Rc::clone(&scroll_off);
        let dv = DragValue::new(so.get(), 0.0, 1e9, Arc::clone(&font))
            .with_font_size(12.0)
            .with_speed(2.0)
            .with_decimals(0)
            .on_change(move |v| so.set(v));
        col.push(Box::new(FlexRow::new().with_gap(8.0)
            .add(label(Arc::clone(&font), "Scroll offset (px)", 12.0))
            .add(Box::new(dv))), 0.0);
    }

    // ── Top / Bottom buttons ──
    {
        let top_c = Rc::clone(&scroll_off);
        let bot_c = Rc::clone(&scroll_off);
        let ms_c  = Rc::clone(&max_scroll);
        col.push(Box::new(FlexRow::new().with_gap(8.0)
            .add(Box::new(
                Button::new("Scroll to top", Arc::clone(&font))
                    .on_click(move || top_c.set(0.0))
            ))
            .add(Box::new(
                Button::new("Scroll to bottom", Arc::clone(&font))
                    .on_click(move || bot_c.set(ms_c.get()))
            ))), 0.0);
    }

    // ── Scroll by delta buttons ──
    {
        let d = Rc::clone(&delta_px);
        let so_up = Rc::clone(&scroll_off);
        let so_dn = Rc::clone(&scroll_off);
        let d_up  = Rc::clone(&d);
        let d_dn  = Rc::clone(&d);
        let ms_up = Rc::clone(&max_scroll);

        let dv = DragValue::new(d.get(), 1.0, 1000.0, Arc::clone(&font))
            .with_font_size(12.0)
            .with_decimals(0)
            .on_change(move |v| d.set(v));

        col.push(Box::new(FlexRow::new().with_gap(8.0)
            .add(label(Arc::clone(&font), "Scroll by", 12.0))
            .add(Box::new(dv))
            .add(Box::new(
                Button::new("\u{2193}", Arc::clone(&font))  // down arrow
                    .on_click(move || {
                        let target = (so_dn.get() + d_dn.get()).clamp(0.0, ms_up.get());
                        so_dn.set(target);
                    })
            ))
            .add(Box::new(
                Button::new("\u{2191}", Arc::clone(&font))  // up arrow
                    .on_click(move || {
                        let target = (so_up.get() - d_up.get()).max(0.0);
                        so_up.set(target);
                    })
            ))), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── Readout ──
    col.push(Box::new(OffsetReadout {
        bounds: Rect::default(), children: Vec::new(),
        font: Arc::clone(&font),
        offset: Rc::clone(&scroll_off),
        max: Rc::clone(&max_scroll),
    }), 0.0);

    // ── Scroll area ──
    let list = RowList::new(
        Arc::clone(&font),
        Rc::new(Cell::new(num_items)),
        Rc::new(|i| format!("This is item {}", i + 1)),
    )
    .with_row_height(row_height)
    .with_highlight_cell(Rc::clone(&highlight));

    let scroll = ScrollView::new(Box::new(list))
        .with_offset_cell(Rc::clone(&scroll_off))
        .with_max_scroll_cell(Rc::clone(&max_scroll));
    col.push(Box::new(scroll), 1.0);

    // Sync target offset once `max_scroll` is known (for initial Center).
    col.push(Box::new(MaxScrollWatcher::new(
        Rc::clone(&max_scroll),
        recompute,
    )), 0.0);

    Box::new(col)
}
