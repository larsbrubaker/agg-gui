//! Interactive Container demo — port of egui's `interactive_container.rs`.
//!
//! Demonstrates a *clickable container*: a canvas frame that increments a
//! click counter when its background is clicked, yet holds two real nested
//! buttons ("Reset" and "+ 100") whose clicks perform only their own action
//! and do **not** bump the counter.
//!
//! The whole point is click precedence.  agg-gui routes a pointer event to the
//! deepest widget under the cursor (see `widget::hit_test_subtree`) and then
//! bubbles the event leaf → root (see `widget::dispatch_event`).  A button that
//! is hit consumes the event, so bubbling stops before the container sees it;
//! a click that lands on the container background is not claimed by any child,
//! so it reaches the container's `on_event` and increments the count.  This is
//! the agg-gui equivalent of egui's `Ui::response` on a `Sense::click()`
//! container, adapted to this crate's retained widget tree.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::widget::paint_subtree;
use agg_gui::{
    Button, Color, DrawCtx, Event, EventResult, FlexColumn, Font, Label, MouseButton, Point, Rect,
    Separator, Size, SizedBox, Widget,
};

/// The clickable container: paints a canvas frame, holds the two action
/// buttons as real children, and increments `count` on a background click.
struct InteractiveContainer {
    bounds: Rect,
    /// `[reset_button, plus_button]` — real nested widgets for hit testing.
    children: Vec<Box<dyn Widget>>,
    /// Manually painted click-count label (non-interactive, so it is not a
    /// child: clicks that land on it fall through to the container).
    count_label: Label,
    count_label_rect: Rect,
    count: Rc<Cell<usize>>,
    hovered: bool,
    pressed: bool,
}

impl InteractiveContainer {
    const MARGIN: f64 = 10.0;
    const SPACING: f64 = 32.0;

    fn new(font: Arc<Font>) -> Self {
        let count = Rc::new(Cell::new(0_usize));

        let reset = {
            let count = Rc::clone(&count);
            Button::new("Reset", Arc::clone(&font))
                .with_font_size(13.0)
                .on_click(move || count.set(0))
        };
        let plus = {
            let count = Rc::clone(&count);
            Button::new("+ 100", Arc::clone(&font))
                .with_font_size(13.0)
                .on_click(move || count.set(count.get() + 100))
        };

        Self {
            bounds: Rect::default(),
            children: vec![Box::new(reset), Box::new(plus)],
            count_label: Label::new("0", font).with_font_size(32.0),
            count_label_rect: Rect::default(),
            count,
            hovered: false,
            pressed: false,
        }
    }

    #[cfg(test)]
    fn count_cell(&self) -> Rc<Cell<usize>> {
        Rc::clone(&self.count)
    }
}

impl Widget for InteractiveContainer {
    fn type_name(&self) -> &'static str {
        "InteractiveContainer"
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
        let m = Self::MARGIN;
        let spacing = Self::SPACING;
        let w = available.width.max(2.0 * m);

        // Buttons laid out at their natural size in a bottom row.
        let mut button_h = 0.0_f64;
        let mut x = m;
        let n = self.children.len();
        let mut positions = Vec::with_capacity(n);
        for child in &mut self.children {
            let bs = child.layout(Size::new(w - 2.0 * m, 32.0));
            positions.push((x, bs));
            x += bs.width + 8.0;
            button_h = button_h.max(bs.height);
        }

        // Count label reflects the shared counter.
        self.count_label.set_text(self.count.get().to_string());
        let ls = self.count_label.layout(Size::new(w - 2.0 * m, spacing));

        // Total height: top margin, spacing, number, spacing, buttons, bottom
        // margin.  Coordinates are Y-up (origin bottom-left).
        let h = m + spacing + ls.height + spacing + button_h + m;

        // Buttons sit on the bottom row.
        for (child, (bx, bs)) in self.children.iter_mut().zip(positions) {
            child.set_bounds(Rect::new(bx, m, bs.width, bs.height));
        }

        // Number centred horizontally, sitting `spacing` above the buttons.
        let ly = m + button_h + spacing;
        let lx = (w - ls.width) * 0.5;
        self.count_label_rect = Rect::new(lx, ly, ls.width, ls.height);
        self.count_label
            .set_bounds(Rect::new(0.0, 0.0, ls.width, ls.height));

        self.bounds = Rect::new(0.0, 0.0, w, h);
        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // Canvas frame: fill derived from the interact background, brightened
        // on hover / press, with a matching stroke — the agg-gui reading of
        // egui's `Frame::canvas` + `style.interact(&response)`.
        let bg = if self.pressed {
            v.accent_pressed
        } else if self.hovered {
            v.widget_bg_hovered
        } else {
            v.widget_bg
        };
        ctx.set_fill_color(Color::rgba(bg.r, bg.g, bg.b, 0.3));
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 6.0);
        ctx.fill();

        ctx.set_stroke_color(if self.hovered || self.pressed {
            v.accent
        } else {
            v.widget_stroke
        });
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 6.0);
        ctx.stroke();

        // The big number, painted through its backbuffered Label.
        self.count_label.set_color(v.text_color);
        let lr = self.count_label_rect;
        ctx.save();
        ctx.translate(lr.x, lr.y);
        paint_subtree(&mut self.count_label, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let was = self.hovered;
                // egui's `response.hovered()` is false while the pointer is over
                // a nested button (that is `contains_pointer`, not `hovered`), so
                // the container frame must not light up over Reset / + 100.
                self.hovered = self.hit_test(*pos) && !self.point_over_child(*pos);
                if self.hovered != was {
                    // The frame lights up on hover, so the cached bitmap is now
                    // stale — invalidate so the next frame repaints.
                    agg_gui::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseDown {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                // Only reachable when the click was not consumed by a nested
                // button, i.e. it landed on the container background.
                if self.hit_test(*pos) {
                    self.pressed = true;
                    // Press darkens the frame fill — repaint needed.
                    agg_gui::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseUp {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                if self.pressed {
                    self.pressed = false;
                    if self.hit_test(*pos) {
                        self.count.set(self.count.get() + 1);
                    }
                    // Releasing clears the press state, and a background click
                    // just changed the count label — either way the bitmap is
                    // stale and must repaint.  This is the fix for the reported
                    // bug where the big count label did not update on click.
                    agg_gui::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        local_pos.x >= 0.0
            && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0
            && local_pos.y <= self.bounds.height
    }
}

impl InteractiveContainer {
    /// True when `pos` (container-local) falls inside one of the nested button
    /// rects.  Used to keep the container's `hovered` flag false over a child,
    /// matching egui's `contains_pointer` vs `hovered` split.
    fn point_over_child(&self, pos: Point) -> bool {
        self.children.iter().any(|child| {
            let b = child.bounds();
            pos.x >= b.x && pos.x <= b.x + b.width && pos.y >= b.y && pos.y <= b.y + b.height
        })
    }
}

/// Build the Interactive Container demo window content.
pub fn interactive_container(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(16.0)
        .with_panel_bg();

    // Explanatory text, adapted from egui's wording to agg-gui naming.
    col.push(
        Box::new(
            Label::new(
                "This demo shows how to use Widget::on_event to build interactive \
                 container widgets that may contain other widgets. Click the container \
                 to count clicks; the nested buttons run only their own action.",
                Arc::clone(&font),
            )
            .with_font_size(12.0)
            .with_wrap(true),
        ),
        0.0,
    );

    col.push(Box::new(InteractiveContainer::new(Arc::clone(&font))), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(
            Label::new(
                "Clicking the background increments the count; clicking Reset or + 100 \
                 does not.",
                Arc::clone(&font),
            )
            .with_font_size(11.0)
            .with_wrap(true),
        ),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agg_gui::widget::{dispatch_event, hit_test_subtree};
    use agg_gui::{Modifiers, MouseButton};

    fn test_font() -> Arc<Font> {
        const BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");
        Arc::new(Font::from_slice(BYTES).expect("parse CascadiaCode.ttf"))
    }

    /// Dispatch a full left click (down then up) at `pos` in the root's local
    /// coordinate space, routing through the same hit-test + bubble path the
    /// app uses at runtime.
    fn click(root: &mut Box<dyn Widget>, pos: Point) {
        for make in [
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            },
            Event::MouseUp {
                pos,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            },
        ] {
            let path = hit_test_subtree(root.as_ref(), pos).unwrap_or_default();
            dispatch_event(root, &path, &make, pos);
        }
    }

    fn build() -> (Box<dyn Widget>, Rc<Cell<usize>>) {
        let container = InteractiveContainer::new(test_font());
        let count = container.count_cell();
        let mut root: Box<dyn Widget> = Box::new(container);
        root.layout(Size::new(240.0, 200.0));
        (root, count)
    }

    fn mouse_move(w: &mut InteractiveContainer, pos: Point) {
        w.on_event(&Event::MouseMove { pos });
    }

    #[test]
    fn hover_is_false_over_nested_buttons() {
        let mut c = InteractiveContainer::new(test_font());
        c.layout(Size::new(240.0, 200.0));

        // Over the empty background near the top: the frame lights up.
        let top = Point::new(120.0, c.bounds().height - 6.0);
        mouse_move(&mut c, top);
        assert!(c.hovered, "background hover should set hovered");

        // Over the "+ 100" button (children[1]): hovered must clear, matching
        // egui's response.hovered() being false over a child.
        let plus = c.children()[1].bounds();
        mouse_move(
            &mut c,
            Point::new(plus.x + plus.width * 0.5, plus.y + plus.height * 0.5),
        );
        assert!(!c.hovered, "hover must be false while pointer is over a button");
    }

    #[test]
    fn background_click_increments_count() {
        let (mut root, count) = build();
        // A point near the top of the container is over the number / empty
        // background, never a button.
        let top = Point::new(120.0, root.bounds().height - 6.0);
        click(&mut root, top);
        assert_eq!(count.get(), 1);
        click(&mut root, top);
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn background_click_requests_draw() {
        // Regression: a background click bumped the counter but never called
        // request_draw, so the big count label did not repaint until some
        // unrelated event forced a redraw.  Every on_event path that mutates
        // paint-affecting state must request a draw.
        let mut c = InteractiveContainer::new(test_font());
        c.layout(Size::new(240.0, 200.0));
        let top = Point::new(120.0, c.bounds().height - 6.0);

        // Press over the background arms `pressed` (frame darkens) → draw.
        agg_gui::animation::clear_draw_request();
        assert_eq!(
            c.on_event(&Event::MouseDown {
                pos: top,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            }),
            EventResult::Consumed
        );
        assert!(
            agg_gui::animation::wants_draw(),
            "press must request a draw (frame press state changed)"
        );

        // Release over the background increments the count → the label text
        // changes and must repaint.
        agg_gui::animation::clear_draw_request();
        assert_eq!(
            c.on_event(&Event::MouseUp {
                pos: top,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            }),
            EventResult::Consumed
        );
        assert!(
            agg_gui::animation::wants_draw(),
            "background click increment must request a draw"
        );
    }

    #[test]
    fn hover_change_requests_draw() {
        // The frame lights up on hover, so a hover transition is paint-affecting
        // and must invalidate.
        let mut c = InteractiveContainer::new(test_font());
        c.layout(Size::new(240.0, 200.0));
        let top = Point::new(120.0, c.bounds().height - 6.0);

        agg_gui::animation::clear_draw_request();
        assert_eq!(
            c.on_event(&Event::MouseMove { pos: top }),
            EventResult::Consumed
        );
        assert!(
            agg_gui::animation::wants_draw(),
            "hover onset must request a draw"
        );

        // Moving off the container clears hover — another paint-affecting change.
        agg_gui::animation::clear_draw_request();
        assert_eq!(
            c.on_event(&Event::MouseMove {
                pos: Point::new(-10.0, -10.0),
            }),
            EventResult::Consumed
        );
        assert!(
            agg_gui::animation::wants_draw(),
            "hover offset must request a draw"
        );
    }

    #[test]
    fn nested_button_click_does_not_increment_count() {
        let (mut root, count) = build();

        // Locate the "+ 100" button (children[1]) and click its centre.
        let plus_bounds = root.children()[1].bounds();
        let plus_center = Point::new(
            plus_bounds.x + plus_bounds.width * 0.5,
            plus_bounds.y + plus_bounds.height * 0.5,
        );
        click(&mut root, plus_center);
        // +100 from the button, and crucially NOT +1 from the container.
        assert_eq!(count.get(), 100);

        // The Reset button (children[0]) zeroes the count without a container
        // increment either.
        let reset_bounds = root.children()[0].bounds();
        let reset_center = Point::new(
            reset_bounds.x + reset_bounds.width * 0.5,
            reset_bounds.y + reset_bounds.height * 0.5,
        );
        click(&mut root, reset_center);
        assert_eq!(count.get(), 0);
    }
}
