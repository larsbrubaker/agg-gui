//! Port of agg-sharp `Tests/Agg.Tests/Agg.UI/CheckRadioToggleStyleTests.cs`:
//! [`Checkbox`], [`RadioButton`] and [`ToggleSwitch`] drawn in agg-gui's look
//! at the classic size MatterCAD's layouts use (see `support.rs` for how C#'s
//! `ThemeConfig` maps onto agg-gui's `Visuals`). C#'s `[Arguments(true)]` /
//! `[Arguments(false)]` (dark, light) legs run in one test, both themes.
//!
//! The controls are laid out with C#'s face and `TextWidget` line box (one
//! em, `LineBox::Em`), so the 12 point check box is 16 high and its 12 box's
//! lower left is (HostPad + 2, HostPad + 2), as C# pins it.
//!
//! `SizesAndLabelPositionsAreUnchanged` prints
//! the logical sizes times the device scale, the device pixels C# reports.
//! agg-gui has no separate `MinimumSize` for a fit control, so "min" is the
//! laid-out size. Its four `ToggleSwitchView` rows are left out: agg-gui's
//! [`ToggleSwitch`] has no on/off text view (MatterCAD draws its own
//! `RoundedToggleSwitch`), so there is no label to place or measure.
//! `ThemeConfigCurrentFallsBackToADefaultOnNull` is not ported: agg-gui's
//! visuals cannot be null.

use agg_gui::event::Event;
use agg_gui::theme::Visuals;
use agg_gui::widgets::ToggleSwitchStyle;
use agg_gui::{Checkbox, Color, Point, RadioButton, ToggleSwitch, Widget};

use super::support::{
    font, is_white, layout, near, pixel, render, rgb8, use_classic_size, HOST_PAD, TWELVE_POINT,
};

/// The check box and radio lines of C#'s `MetricsBeforeRestyle`, at device
/// scale 1 and then 1.5.
const METRICS_BEFORE_RESTYLE: &str = "91.148x16 min 91.148x16 label 20,0\n\
85.219x14.667 min 85.219x14.667 label 20,0\n\
62.805x16 min 62.805x16 label 21,0\n\
59.321x14.667 min 59.321x14.667 label 21,0\n\
136.723x24 min 136.723x24 label 30,0\n\
127.829x22 min 127.829x22 label 30,0\n\
94.207x24 min 94.207x24 label 31.5,0\n\
88.981x22 min 88.981x22 label 31.5,0";

/// C#'s `{0:0.###}`.
fn fmt3(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// C#'s `Metrics`, in device pixels: the laid-out logical geometry times the
/// device scale.
fn metrics(widget: &mut dyn Widget) -> String {
    let s = layout(widget);
    let d = agg_gui::device_scale();
    let label = widget.children()[0].bounds();
    let (w, h) = (fmt3(s.width * d), fmt3(s.height * d));
    format!(
        "{w}x{h} min {w}x{h} label {},{}",
        fmt3(label.x * d),
        fmt3(label.y * d)
    )
}

fn all_metrics() -> String {
    let mut parts = Vec::new();
    for scale in [1.0, 1.5] {
        agg_gui::set_device_scale(scale);
        let points = |p: f64| p * 4.0 / 3.0;
        parts.push(metrics(
            &mut Checkbox::new("Checkbox", font(), false).with_font_size(points(12.0)),
        ));
        parts.push(metrics(
            &mut Checkbox::new("Checkbox", font(), false).with_font_size(points(11.0)),
        ));
        parts.push(metrics(
            &mut RadioButton::new("Radio", font()).with_font_size(points(12.0)),
        ));
        parts.push(metrics(
            &mut RadioButton::new("Radio", font()).with_font_size(points(11.0)),
        ));
    }
    parts.join("\n")
}

#[test]
fn sizes_and_label_positions_are_unchanged() {
    use_classic_size();
    let saved = agg_gui::device_scale();
    let metrics = all_metrics();
    agg_gui::set_device_scale(saved);
    assert_eq!(metrics, METRICS_BEFORE_RESTYLE);
}

fn themes() -> [(bool, Visuals); 2] {
    [(true, Visuals::dark()), (false, Visuals::light())]
}

fn check_box() -> Checkbox {
    let mut cb = Checkbox::new("Checkbox", font(), false).with_font_size(TWELVE_POINT);
    layout(&mut cb);
    cb
}

/// The box's lower left in the image.
fn box_origin(cb: &Checkbox) -> (u32, u32) {
    let r = cb.box_rect();
    (HOST_PAD + r.x as u32, HOST_PAD + r.y as u32)
}

/// C#'s hover: the mouse over the widget's left end, where the mark is.
fn hover(widget: &mut dyn Widget) {
    let h = widget.bounds().height;
    widget.on_event(&Event::MouseMove {
        pos: Point::new(6.0, h / 2.0),
    });
}

#[test]
fn check_box_is_accent_filled_with_a_white_tick_when_checked() {
    use_classic_size();
    for (dark, theme) in themes() {
        let mut unchecked_box = check_box();
        let (left, bottom) = box_origin(&unchecked_box);
        assert_eq!(
            unchecked_box.box_rect().width,
            12.0,
            "the classic box is 12"
        );
        assert_eq!((left, bottom), (HOST_PAD + 2, HOST_PAD + 2));
        let unchecked_image = render(&mut unchecked_box, &theme);
        let mut checked_box = check_box();
        checked_box.set_checked(true);
        let checked_image = render(&mut checked_box, &theme);
        let mut hovered_box = check_box();
        hover(&mut hovered_box);
        let hovered = render(&mut hovered_box, &theme);
        let mut disabled_box = check_box().with_enabled(false);
        disabled_box.set_checked(true);
        let disabled = render(&mut disabled_box, &theme);

        let accent = rgb8(theme.accent);
        // the top left of the box, clear of the tick
        let accent_pixel = pixel(&checked_image, left + 3, bottom + 9);
        assert!(
            near(accent_pixel, accent, 12),
            "dark {dark}: checked fills the box with the accent, got {accent_pixel:?}"
        );

        // the tick's lower vertex, at 42% across and 28% up
        let tick_pixel = pixel(&checked_image, left + 5, bottom + 3);
        assert!(
            is_white(tick_pixel),
            "dark {dark}: the tick is white, got {tick_pixel:?}"
        );

        let unchecked_pixel = pixel(&unchecked_image, left + 6, bottom + 6);
        let expected_fill = rgb8(if dark {
            theme.window_fill
        } else {
            theme.widget_bg
        });
        assert!(
            near(unchecked_pixel, expected_fill, 2),
            "dark {dark}: unchecked shows the neutral fill, got {unchecked_pixel:?}"
        );
        let border_pixel = pixel(&unchecked_image, left + 6, bottom);
        assert!(
            !near(border_pixel, expected_fill, 10),
            "dark {dark}: unchecked draws a border, got {border_pixel:?}"
        );

        let hover_pixel = pixel(&hovered, left + 6, bottom + 6);
        assert!(
            !near(hover_pixel, unchecked_pixel, 2),
            "dark {dark}: hover shades the box, got {hover_pixel:?}"
        );

        let disabled_pixel = pixel(&disabled, left + 3, bottom + 9);
        assert!(
            !near(disabled_pixel, accent, 12),
            "dark {dark}: disabled fades the accent, got {disabled_pixel:?}"
        );
    }
}

/// Indeterminate (egui's tri-state) keeps the accent fill but swaps the tick
/// for a white dash.
#[test]
fn indeterminate_check_box_is_accent_filled_with_a_white_dash() {
    use_classic_size();
    let theme = Visuals::light();
    let mut cb = Checkbox::new("Checkbox", font(), false)
        .with_font_size(TWELVE_POINT)
        .with_indeterminate_fn(|| true);
    layout(&mut cb);
    let (left, bottom) = box_origin(&cb);
    let image = render(&mut cb, &theme);
    let accent = rgb8(theme.accent);

    let accent_pixel = pixel(&image, left + 3, bottom + 9);
    assert!(
        near(accent_pixel, accent, 12),
        "indeterminate fills the box with the accent, got {accent_pixel:?}"
    );

    // the dash crosses the middle of the 12 box; it is 1.5 thick so no row is
    // fully covered
    let dash_pixel = pixel(&image, left + 6, bottom + 6);
    assert!(
        i32::from(dash_pixel[0]) > i32::from(accent[0]) + 100,
        "the dash is white, got {dash_pixel:?}"
    );

    // where the tick's lower vertex would be is still accent - no tick
    let no_tick_pixel = pixel(&image, left + 5, bottom + 3);
    assert!(
        near(no_tick_pixel, accent, 12),
        "no tick is drawn, got {no_tick_pixel:?}"
    );
}

/// The radio circle is 11 square at the view's left, centred vertically; its
/// radius is 5.
#[test]
fn radio_is_an_accent_circle_with_an_inner_dot_when_selected() {
    use_classic_size();
    for (dark, theme) in themes() {
        let mut radio = RadioButton::new("Radio", font())
            .with_font_size(TWELVE_POINT)
            .with_checked(true);
        layout(&mut radio);
        assert_eq!(
            radio.circle_radius(),
            5.0,
            "the classic circle's radius is 5"
        );
        let image = render(&mut radio, &theme);
        let c = radio.circle_center();
        let (cx, cy) = (c.x + f64::from(HOST_PAD), c.y + f64::from(HOST_PAD));

        let ring = pixel(&image, (cx + 3.0) as u32, cy as u32);
        assert!(
            near(ring, rgb8(theme.accent), 12),
            "dark {dark}: a selected radio is accent, got {ring:?}"
        );
        let dot = pixel(&image, cx as u32, cy as u32);
        assert!(
            near(dot, rgb8(theme.widget_bg), 12),
            "dark {dark}: with an inner widget background dot, got {dot:?}"
        );

        let mut off_radio = RadioButton::new("Radio", font()).with_font_size(TWELVE_POINT);
        layout(&mut off_radio);
        let off = render(&mut off_radio, &theme);
        let off_pixel = pixel(&off, (cx + 3.0) as u32, cy as u32);
        assert!(
            !near(off_pixel, rgb8(theme.accent), 12),
            "dark {dark}: an unselected radio is not accent, got {off_pixel:?}"
        );
    }
}

/// C#'s `Toggle("", "", 40, 20, onColor)`: a 40 x 20 pill with agg-sharp's
/// knob, inset 2.5 of an 18 high pill (`SelectionControlStyle.DrawSwitch`).
fn toggle(on: bool, on_color: Color) -> ToggleSwitch {
    let (width, height) = (40.0, 20.0);
    let radius = height / 2.0;
    let mut sw = ToggleSwitch::new(on).with_style(ToggleSwitchStyle {
        track_on: Some(on_color),
        track_width: Some(width),
        track_height: Some(height),
        knob_radius: Some(radius - 2.5 * height / 18.0),
        ..ToggleSwitchStyle::default()
    });
    layout(&mut sw);
    sw
}

/// The switch is 40 x 20 at the host's pad; its knob sits at one end or the
/// other. agg-gui's switch keeps a one unit halo margin round its pill
/// (`toggle_switch.rs`'s `PILL_HALO`), so the pill, and every probe on it,
/// sits one unit further in than C#'s.
#[test]
fn toggle_knob_is_white_at_the_end_matching_its_state() {
    let halo = 1;
    for (dark, theme) in themes() {
        let on_color = Color::rgba(0.0, 90.0 / 255.0, 220.0 / 255.0, 1.0);
        let origin = HOST_PAD + halo;
        let middle = origin + 10;

        let off_image = render(&mut toggle(false, on_color), &theme);
        let on_image = render(&mut toggle(true, on_color), &theme);

        assert!(
            is_white(pixel(&off_image, origin + 10, middle)),
            "dark {dark}: off puts the knob at the left"
        );
        assert!(
            !is_white(pixel(&off_image, origin + 30, middle)),
            "dark {dark}: off leaves the right end track coloured"
        );

        assert!(
            is_white(pixel(&on_image, origin + 30, middle)),
            "dark {dark}: on puts the knob at the right"
        );
        let track = pixel(&on_image, origin + 10, middle);
        assert!(
            near(track, rgb8(on_color), 12),
            "dark {dark}: on fills the track with the on colour, got {track:?}"
        );

        // the pill's corner is rounded: its very corner shows the background
        let corner = pixel(&on_image, origin, origin);
        assert!(
            near(corner, rgb8(theme.window_fill), 4),
            "dark {dark}: the track is a pill, got {corner:?}"
        );
    }
}
