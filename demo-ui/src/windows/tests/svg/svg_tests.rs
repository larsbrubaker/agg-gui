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
fn svg_test_includes_current_paint_server_capability_rows() {
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
        "paint-servers/linearGradient/single-stop-with-opacity-used-by-stroke.svg",
        "paint-servers/radialGradient/gradientUnits=userSpaceOnUse.svg",
        "paint-servers/radialGradient/gradientTransform.svg",
        "paint-servers/radialGradient/focal-point-correction.svg",
        "paint-servers/radialGradient/spreadMethod=repeat.svg",
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

#[test]
fn svg_test_every_rgba_row_is_either_exact_or_tracked_as_incomplete() {
    let mut unexpected_mismatches = Vec::new();
    let mut completed_known_incomplete = Vec::new();

    for sample in super::SVG_SAMPLES {
        let rendered = super::SvgSampleRender::new(sample);
        let reference = rendered
            .reference
            .as_ref()
            .unwrap_or_else(|err| panic!("{} reference PNG should decode: {err}", sample.name));
        let rgba = rendered.rgba.as_ref().unwrap_or_else(|err| {
            panic!("{} should render through RGBA target: {err}", sample.name)
        });
        let diff = pixel_diff(rgba, reference);

        if KNOWN_INCOMPLETE_RGBA_ROWS.contains(&sample.name) {
            if diff.mismatched_pixels == 0 {
                completed_known_incomplete.push(sample.name);
            }
        } else if diff.mismatched_pixels > 0 {
            unexpected_mismatches.push((sample.name, diff));
        }
    }

    assert!(
        completed_known_incomplete.is_empty(),
        "these rows now match reference.png exactly; remove them from KNOWN_INCOMPLETE_RGBA_ROWS:\n{}",
        completed_known_incomplete.join("\n")
    );
    assert!(
        unexpected_mismatches.is_empty(),
        "these SVG Test rows do not match reference.png exactly and must either be fixed or added to KNOWN_INCOMPLETE_RGBA_ROWS:\n{}",
        unexpected_mismatches
            .iter()
            .map(|(name, diff)| format!(
                "{name} ({} mismatched pixels, max delta {})",
                diff.mismatched_pixels, diff.max_delta
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

const KNOWN_INCOMPLETE_RGBA_ROWS: &[&str] = &[
    // Every current SVG Test row is a progress row, not an exact-match
    // completion row. When a row becomes pixel-accurate, remove it from
    // this list; the classifier above will then require exact equality.
    "shapes/rect/simple-case.svg",
    "shapes/circle/simple-case.svg",
    "shapes/ellipse/simple-case.svg",
    "shapes/line/simple-case.svg",
    "shapes/line/with-transform.svg",
    "shapes/polygon/simple-case.svg",
    "shapes/polyline/simple-case.svg",
    "shapes/path/M-L-L-Z.svg",
    "shapes/path/M-C.svg",
    "shapes/path/M-C-S.svg",
    "shapes/path/M-Q.svg",
    "shapes/path/M-Q-T.svg",
    "shapes/path/M-A.svg",
    "shapes/path/M-L-Z-A.svg",
    "painting/fill/named-color.svg",
    "painting/fill/currentColor.svg",
    "painting/fill/rgb-color.svg",
    "painting/fill/hsl-with-alpha.svg",
    "painting/fill/linear-gradient-on-shape.svg",
    "painting/fill/radial-gradient-on-shape.svg",
    "painting/fill-rule/nonzero.svg",
    "painting/fill-rule/evenodd.svg",
    "painting/opacity/50percent.svg",
    "painting/opacity/group-opacity.svg",
    "painting/opacity/mixed-group-opacity.svg",
    "painting/stroke/line-as-curve-1.svg",
    "painting/stroke/line-as-curve-2.svg",
    "painting/stroke/linear-gradient.svg",
    "painting/stroke/radial-gradient.svg",
    "paint-servers/linearGradient/gradientUnits=userSpaceOnUse.svg",
    "paint-servers/linearGradient/gradientUnits=objectBoundingBox-with-percent.svg",
    "paint-servers/linearGradient/gradientTransform.svg",
    "paint-servers/linearGradient/gradientTransform-and-transform.svg",
    "paint-servers/linearGradient/spreadMethod=reflect.svg",
    "paint-servers/linearGradient/spreadMethod=repeat.svg",
    "paint-servers/linearGradient/many-stops.svg",
    "paint-servers/linearGradient/single-stop-with-opacity-used-by-stroke.svg",
    "paint-servers/radialGradient/gradientUnits=userSpaceOnUse.svg",
    "paint-servers/radialGradient/gradientUnits=objectBoundingBox-with-percent.svg",
    "paint-servers/radialGradient/gradientTransform.svg",
    "paint-servers/radialGradient/focal-point-correction.svg",
    "paint-servers/radialGradient/spreadMethod=reflect.svg",
    "paint-servers/radialGradient/spreadMethod=repeat.svg",
    "paint-servers/radialGradient/many-stops.svg",
    "paint-servers/pattern/simple-case.svg",
    "paint-servers/pattern/patternUnits=userSpaceOnUse-with-percent.svg",
    "paint-servers/pattern/patternContentUnits-with-viewBox.svg",
    "paint-servers/pattern/transform-and-patternTransform.svg",
    "structure/image/embedded-png.svg",
    "structure/image/embedded-jpeg-as-image-jpeg.svg",
    "structure/image/embedded-gif.svg",
    "structure/image/embedded-svg.svg",
    "structure/image/preserveAspectRatio=none.svg",
    "structure/image/raster-image-and-size-with-odd-numbers.svg",
    "text/text/simple-case.svg",
    "text/tspan/sequential.svg",
    "text/text-anchor/middle-on-text.svg",
    "text/text-decoration/underline.svg",
];

struct PixelDiff {
    mismatched_pixels: usize,
    max_delta: u8,
}

fn pixel_diff(a: &[u8], b: &[u8]) -> PixelDiff {
    let mut mismatched_pixels = usize::from(a.len() != b.len());
    let mut max_delta = 0_u8;
    for (a, b) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        let pixel_delta = a
            .iter()
            .zip(b.iter())
            .map(|(&a, &b)| a.abs_diff(b))
            .max()
            .unwrap_or(0);
        if pixel_delta > 0 {
            mismatched_pixels += 1;
            max_delta = max_delta.max(pixel_delta);
        }
    }
    PixelDiff {
        mismatched_pixels,
        max_delta,
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
