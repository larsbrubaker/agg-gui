//! Widget names and typed downcasts (C# `GuiWidget.Name` and `is T`).
//!
//! `WidgetBase::name` is the per-instance name automation looks widgets up
//! by: [`Widget::id`] defaults to it, [`Widget::with_name`] sets it on any
//! widget that embeds a `WidgetBase`, and [`Named`] gives a name to a widget
//! that has none of its own.  `Widget::as_any` lets a caller holding a
//! `&dyn Widget` read the concrete widget's state.

use crate::widget::find_widget_by_id;
use crate::widgets::Named;
use crate::{Button, FlexColumn, Label, Rect, Size, SizedBox, TextField, Widget};

use super::TEST_FONT;
use std::sync::Arc;

fn font() -> Arc<crate::Font> {
    Arc::new(crate::Font::from_slice(TEST_FONT).expect("test font"))
}

#[test]
fn with_name_sets_the_default_id() {
    let button = Button::new("Go", font()).with_name("Go Button");
    assert_eq!(button.id(), Some("Go Button"));
    assert_eq!(
        button.widget_base().and_then(|b| b.name.as_deref()),
        Some("Go Button")
    );
}

#[test]
fn an_unnamed_widget_has_no_id() {
    let button = Button::new("Go", font());
    assert_eq!(button.id(), None);
}

#[test]
fn set_name_works_through_a_boxed_widget() {
    let mut boxed: Box<dyn Widget> = Box::new(SizedBox::new());
    boxed.set_name(Some("spacer".to_string()));
    assert_eq!(boxed.id(), Some("spacer"));
    boxed.set_name(None);
    assert_eq!(boxed.id(), None);
}

#[test]
fn named_widgets_are_found_in_the_tree() {
    let column = FlexColumn::new()
        .add(Box::new(Label::new("a", font()).with_name("first")))
        .add(Box::new(Label::new("b", font()).with_name("second")));
    let found = find_widget_by_id(&column, "second").expect("second is in the tree");
    assert_eq!(found.type_name(), "Label");
}

#[test]
fn named_wraps_a_widget_without_a_name_of_its_own() {
    let mut named = Named::new("wrapped", Box::new(SizedBox::new().with_width(30.0)));
    assert_eq!(named.id(), Some("wrapped"));
    assert_eq!(named.type_name(), "Named");
    let size = named.layout(Size::new(100.0, 100.0));
    assert_eq!(named.children().len(), 1);
    assert_eq!(
        named.children()[0].bounds(),
        Rect::new(0.0, 0.0, size.width, size.height)
    );
}

#[test]
fn named_reports_the_properties_it_is_given() {
    let named = Named::new("n", Box::new(SizedBox::new()))
        .with_properties(|| vec![("checked", "true".to_string())]);
    assert_eq!(named.properties(), vec![("checked", "true".to_string())]);
}

#[test]
fn as_any_downcasts_core_widgets() {
    let mut field: Box<dyn Widget> = Box::new(TextField::new(font()).with_text("hello"));
    let text = field
        .as_any()
        .and_then(|any| any.downcast_ref::<TextField>())
        .map(|f| f.text());
    assert_eq!(text.as_deref(), Some("hello"));
    assert!(field
        .as_any()
        .and_then(|any| any.downcast_ref::<Button>())
        .is_none());
    let field_mut = field
        .as_any_mut()
        .and_then(|any| any.downcast_mut::<TextField>())
        .expect("TextField downcasts mutably");
    field_mut.set_text("bye");
    assert_eq!(field.text_input_value().as_deref(), Some("bye"));
}

#[test]
fn as_any_defaults_to_none() {
    struct Bare {
        children: Vec<Box<dyn Widget>>,
    }
    impl Widget for Bare {
        fn bounds(&self) -> Rect {
            Rect::default()
        }
        fn set_bounds(&mut self, _bounds: Rect) {}
        fn children(&self) -> &[Box<dyn Widget>] {
            &self.children
        }
        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
            &mut self.children
        }
        fn layout(&mut self, available: Size) -> Size {
            available
        }
        fn paint(&mut self, _ctx: &mut dyn crate::DrawCtx) {}
        fn on_event(&mut self, _event: &crate::Event) -> crate::EventResult {
            crate::EventResult::Ignored
        }
    }
    let bare = Bare { children: vec![] };
    assert!(bare.as_any().is_none());
    assert_eq!(bare.id(), None);
}
