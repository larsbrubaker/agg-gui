use std::sync::Arc;

use agg_gui::{find_widget_by_type, Event, Font, Modifiers, MouseButton, Point, Size};

#[test]
fn svg_test_keeps_header_fixed_above_bidirectional_scroll_area() {
    const BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(BYTES).expect("parse CascadiaCode.ttf"));
    let mut root = super::svg_test(font);

    root.layout(Size::new(520.0, 260.0));

    let children = root.children();
    assert_eq!(children[0].type_name(), "SvgProgressHeader");
    assert_eq!(children[1].type_name(), "ScrollView");
    assert_eq!(children[0].bounds().height, super::SVG_HEADER_H);
    assert_eq!(children[0].children().len(), 3);
    for button in children[0].children() {
        assert!(
            button.bounds().y > super::SVG_COLUMN_HEADER_H,
            "zoom buttons should sit above the fixed column header"
        );
    }

    let scroll = find_widget_by_type(root.as_ref(), "ScrollView").expect("SVG Test scroll view");
    let props = scroll.properties();
    assert_property(&props, "v_enabled", "true");
    assert_property(&props, "h_enabled", "true");
    assert_property(&props, "bar_visibility", "AlwaysVisible");
    assert_positive_property(&props, "max_scroll");
    assert_positive_property(&props, "h_max_scroll");
}

#[test]
fn svg_test_defaults_to_half_zoom() {
    const BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(BYTES).expect("parse CascadiaCode.ttf"));
    let mut root = super::svg_test(font);

    root.layout(Size::new(520.0, 260.0));

    let scroll = find_widget_by_type(root.as_ref(), "ScrollView").expect("SVG Test scroll view");
    let props = scroll.properties();
    let h_content = property_value(&props, "h_content").parse::<f64>().unwrap();
    assert!(
        h_content < 1400.0,
        "SVG Test should default to 50% zoom, got h_content={h_content}"
    );
}

#[test]
fn svg_zoom_buttons_change_to_their_own_targets() {
    const BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(BYTES).expect("parse CascadiaCode.ttf"));
    let mut root = super::svg_test(font);
    root.layout(Size::new(520.0, 260.0));

    let default_content_w = svg_scroll_property(&root, "h_content");
    click_header_button(&mut root, 1);
    root.layout(Size::new(520.0, 260.0));
    let zoom_100_content_w = svg_scroll_property(&root, "h_content");
    assert!(
        zoom_100_content_w > default_content_w,
        "100% button should increase content width"
    );

    click_header_button(&mut root, 0);
    root.layout(Size::new(520.0, 260.0));
    let zoom_50_content_w = svg_scroll_property(&root, "h_content");
    assert!(
        zoom_50_content_w < zoom_100_content_w,
        "50% button should restore the smaller half-zoom content width"
    );
}

#[test]
fn svg_test_includes_current_linear_gradient_capability_rows() {
    let names: Vec<&str> = super::SVG_SAMPLES
        .iter()
        .map(|sample| sample.name)
        .collect();
    for expected in [
        "paint-servers/linearGradient/gradientUnits=userSpaceOnUse.svg",
        "paint-servers/linearGradient/gradientTransform.svg",
        "paint-servers/linearGradient/spreadMethod=reflect.svg",
        "paint-servers/linearGradient/spreadMethod=repeat.svg",
        "paint-servers/linearGradient/many-stops.svg",
    ] {
        assert!(
            names.contains(&expected),
            "SVG Test should include capability row {expected}"
        );
    }
}

#[test]
fn svg_test_sample_rows_decode_and_render_for_bitmap_targets() {
    for sample in super::SVG_SAMPLES {
        let rendered = super::SvgSampleRender::new(sample);
        assert!(
            rendered.reference.is_ok(),
            "{} reference PNG should decode: {:?}",
            sample.name,
            rendered.reference.err()
        );
        assert!(
            rendered.rgba.is_ok(),
            "{} should render through RGBA target: {:?}",
            sample.name,
            rendered.rgba.err()
        );
        assert!(
            rendered.lcd.is_ok(),
            "{} should render through LCD target: {:?}",
            sample.name,
            rendered.lcd.err()
        );
    }
}

fn assert_property(props: &[(&'static str, String)], name: &str, expected: &str) {
    let actual = property_value(props, name);
    assert_eq!(actual, expected);
}

fn assert_positive_property(props: &[(&'static str, String)], name: &str) {
    let actual = property_value(props, name);
    let value = actual
        .parse::<f64>()
        .unwrap_or_else(|_| panic!("{name} should be a number, got {actual:?}"));
    assert!(value > 0.0, "{name} should be positive, got {value}");
}

fn property_value<'a>(props: &'a [(&'static str, String)], name: &str) -> &'a str {
    props
        .iter()
        .find_map(|(key, value)| (*key == name).then_some(value.as_str()))
        .unwrap_or_else(|| panic!("missing property {name}"))
}

fn click_header_button(root: &mut Box<dyn agg_gui::Widget>, index: usize) {
    let button = &mut root.children_mut()[0].children_mut()[index];
    let center = Point::new(button.bounds().width * 0.5, button.bounds().height * 0.5);
    let mods = Modifiers::default();
    button.on_event(&Event::MouseDown {
        pos: center,
        button: MouseButton::Left,
        modifiers: mods,
    });
    button.on_event(&Event::MouseUp {
        pos: center,
        button: MouseButton::Left,
        modifiers: mods,
    });
}

fn svg_scroll_property(root: &Box<dyn agg_gui::Widget>, name: &str) -> f64 {
    let scroll = find_widget_by_type(root.as_ref(), "ScrollView").expect("SVG Test scroll view");
    property_value(&scroll.properties(), name)
        .parse::<f64>()
        .unwrap_or_else(|_| panic!("{name} should be a number"))
}
