//! Port of agg-sharp `TextAndTextWidgetTests.cs`: a text widget (agg-gui
//! `Label`, C# `TextWidget`; with wrapping, C# `WrappedTextWidget`) sizes to
//! its text and shrinks back, and draws its text where it sits.
//!
//! `TextWidgetVisibleTest`'s first leg finds the widget's text in the
//! parent's render with C#'s `FindLeastSquaresMatch(textOnly, 1)`; the
//! reference is the same `Label` rendered alone at (1, 1), since agg-gui has
//! no separate `DrawString` path with the Label's metrics. Its second leg
//! matched a C# `TextEditWidget`, which draws no frame, against bare text; an
//! agg-gui `TextField` draws a frame, so that leg is a behaviour test: the
//! field's text shows up in the render (it differs from the same field
//! empty).

use agg_gui::widgets::AbsoluteLayout;
use agg_gui::{Framebuffer, HAnchor, Label, Size, TextField, VAnchor, Widget};

use super::support::{bounds_at, font, layout_at, render};

/// C# `ImageBuffer.FindLeastSquaresMatch`: whether `find` appears somewhere
/// in `image` with a summed squared byte error of at most `max_error`.
fn find_least_squares_match(image: &Framebuffer, find: &Framebuffer, max_error: f64) -> bool {
    let (iw, ih) = (image.width() as usize, image.height() as usize);
    let (fw, fh) = (find.width() as usize, find.height() as usize);
    if iw < fw || ih < fh {
        return false;
    }
    let (a, b) = (image.pixels(), find.pixels());
    let mut best = f64::MAX;
    for my in 0..=ih - fh {
        for mx in 0..=iw - fw {
            let mut err = 0.0;
            'rows: for y in 0..fh {
                for x in 0..fw {
                    let ia = ((my + y) * iw + mx + x) * 4;
                    let ib = (y * fw + x) * 4;
                    for k in 0..4 {
                        let d = a[ia + k] as f64 - b[ib + k] as f64;
                        err += d * d;
                    }
                    if err > max_error {
                        break 'rows;
                    }
                }
            }
            best = best.min(err);
            if best == 0.0 {
                return true;
            }
        }
    }
    best <= max_error
}

fn label_size(label: &mut Label) -> Size {
    label.layout(Size::new(1000.0, 1000.0))
}

#[test]
fn text_widget_auto_size_test() {
    // resize works on text widgets
    {
        let mut text_item = Label::new("test Item", font());
        let orig_width = label_size(&mut text_item).width;
        text_item.set_text("test Items");
        let newline_width = label_size(&mut text_item).width;
        assert!(newline_width > orig_width);

        text_item.set_text("test Item");
        let back_to_orig_width = label_size(&mut text_item).width;
        assert!(back_to_orig_width == orig_width);

        let orig_height = label_size(&mut text_item).height;
        text_item.set_text("test\nItem");
        let _newline_height = label_size(&mut text_item).height;
        text_item.set_text("test Item");
        let back_to_orig_height = label_size(&mut text_item).height;
        assert!(back_to_orig_height == orig_height);
    }

    // make sure text widget gets smaller vertically when it needs to
    {
        let holder = AbsoluteLayout::new()
            .with_h_anchor(HAnchor::ABSOLUTE)
            .with_v_anchor(VAnchor::FIT)
            .add(Box::new(
                Label::new("some very long text that can wrap", font())
                    .with_wrap(true)
                    .with_h_anchor(HAnchor::STRETCH),
            ));
        let mut container = AbsoluteLayout::new().add(Box::new(holder));
        let set_holder_width = |c: &mut AbsoluteLayout, w: f64| {
            let base = c.children_mut()[0].widget_base_mut().expect("WidgetBase");
            base.min_size = Size::new(w, 0.0);
            base.max_size = Size::new(w, f64::MAX);
        };
        let text_size = |c: &mut AbsoluteLayout| {
            layout_at(c, Size::new(640.0, 480.0));
            let b = bounds_at(c, &[0, 0]);
            Size::new(b.width, b.height)
        };

        set_holder_width(&mut container, 500.0);
        let orig_size = text_size(&mut container);
        assert!(orig_size.width > 10.0);
        set_holder_width(&mut container, 100.0);
        let big_size = text_size(&mut container);
        assert!(big_size.width < orig_size.width);
        assert!(big_size.height > orig_size.height);

        set_holder_width(&mut container, 500.0);
        let back_to_orig_size = text_size(&mut container);
        assert!(back_to_orig_size.width == orig_size.width);
        assert!(back_to_orig_size.height == orig_size.height);

        let orig_height = text_size(&mut container).height;
        let set_text = |c: &mut AbsoluteLayout, t: &str| {
            c.children_mut()[0].children_mut()[0]
                .as_any_mut()
                .and_then(|a| a.downcast_mut::<Label>())
                .expect("a Label")
                .set_text(t);
        };
        set_text(&mut container, "test\nItem");
        let _newline_height = text_size(&mut container).height;
        set_text(&mut container, "test Item");
        let back_to_orig_height = text_size(&mut container).height;
        assert!(back_to_orig_height == orig_height);
    }
}

#[test]
fn text_widget_visible_test() {
    {
        let mut rectangle_widget = AbsoluteLayout::new().add(Box::new(
            Label::new("test Item", font()).with_origin(10.0, 10.0),
        ));
        layout_at(&mut rectangle_widget, Size::new(100.0, 50.0));
        let back_buffer = render(&mut rectangle_widget, 100, 50);

        let mut text_only = AbsoluteLayout::new().add(Box::new(
            Label::new("test Item", font()).with_origin(1.0, 1.0),
        ));
        layout_at(&mut text_only, Size::new(75.0, 20.0));
        let text_only = render(&mut text_only, 75, 20);

        assert!(find_least_squares_match(&back_buffer, &text_only, 1.0));
    }

    {
        let field = |text: &str| {
            let mut w = AbsoluteLayout::new().add(Box::new(
                TextField::new(font())
                    .with_text(text)
                    .with_origin(10.0, 10.0),
            ));
            layout_at(&mut w, Size::new(100.0, 50.0));
            let fb = render(&mut w, 100, 50);
            (fb, bounds_at(&w, &[0]))
        };
        let (with_text, b) = field("test Item");
        let (empty, _) = field("");
        let inside = |fb: &Framebuffer| {
            let mut px = Vec::new();
            for y in b.y as u32..(b.y + b.height).min(50.0) as u32 {
                for x in b.x as u32..(b.x + b.width).min(100.0) as u32 {
                    let i = ((y * 100 + x) * 4) as usize;
                    px.extend_from_slice(&fb.pixels()[i..i + 4]);
                }
            }
            px
        };
        assert!(
            inside(&with_text) != inside(&empty),
            "the field's text is drawn inside it"
        );
    }
}
