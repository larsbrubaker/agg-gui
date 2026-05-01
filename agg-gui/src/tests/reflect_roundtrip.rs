//! Phase 1 reflection sanity tests.
//!
//! Verify that `bevy_reflect`-derived value types can be inspected as
//! `dyn Reflect` and (where it makes sense) walked field-by-field.  These
//! tests are the floor: if they break, downstream inspector / persistence
//! code that relies on reflection has lost its foundation.

#![cfg(feature = "reflect")]

use bevy_reflect::{FromReflect, PartialReflect, Reflect, Struct};

use crate::{
    layout_props::Insets, AccentColor, Color, InspectorSavedState, OsWindowState, Point, Rect,
    Size, ThemePreference,
};

#[test]
fn reflect_color_exposes_named_fields() {
    let c = Color::rgba(0.25, 0.5, 0.75, 1.0);
    let s: &dyn Struct = (&c as &dyn Reflect).reflect_ref().as_struct().unwrap();
    let r = s
        .field("r")
        .and_then(|v| v.try_downcast_ref::<f32>())
        .copied()
        .unwrap();
    let g = s
        .field("g")
        .and_then(|v| v.try_downcast_ref::<f32>())
        .copied()
        .unwrap();
    assert!((r - 0.25).abs() < 1e-6, "r={r}");
    assert!((g - 0.5).abs() < 1e-6, "g={g}");
}

#[test]
fn reflect_value_types_implement_reflect() {
    fn assert_reflect<T: Reflect>() {}
    assert_reflect::<Color>();
    assert_reflect::<Point>();
    assert_reflect::<Size>();
    assert_reflect::<Rect>();
    assert_reflect::<Insets>();
    assert_reflect::<ThemePreference>();
    assert_reflect::<AccentColor>();
    assert_reflect::<OsWindowState>();
    assert_reflect::<InspectorSavedState>();
}

#[test]
fn slider_reflected_props_appear_in_inspector_node() {
    use crate::text::Font;
    use crate::widget::collect_inspector_nodes;
    use crate::Slider;
    use std::sync::Arc;

    const TEST_FONT: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let slider = Slider::new(0.42, 0.0, 1.0, font);

    let mut nodes = Vec::new();
    collect_inspector_nodes(&slider, 0, crate::Point::ORIGIN, &mut nodes);
    let n = &nodes[0];

    let names: Vec<&str> = n.properties.iter().map(|(k, _)| *k).collect();
    assert!(names.contains(&"value"), "missing value: {names:?}");
    assert!(names.contains(&"min"), "missing min: {names:?}");
    assert!(names.contains(&"max"), "missing max: {names:?}");

    let value_field = n
        .properties
        .iter()
        .find(|(k, _)| *k == "value")
        .map(|(_, v)| v.as_str())
        .unwrap();
    assert!(
        value_field.starts_with("0.42"),
        "value should reflect 0.42: got {value_field}"
    );
}

#[test]
fn reflect_partial_clone_color() {
    let original = Color::rgba(0.1, 0.2, 0.3, 0.4);
    let cloned: Box<dyn PartialReflect> = (&original as &dyn PartialReflect).to_dynamic();
    let back = Color::from_reflect(cloned.as_ref()).expect("from_reflect roundtrip");
    assert_eq!(original, back);
}
