//! agg-sharp's device-scale tests (`Tests/Agg.Tests/Agg.UI/*DeviceScaleTests.cs`):
//! widgets laid out and painted at device scale 1 and 2 keep their design-unit
//! geometry, so in device pixels everything grows with the scale. One file per
//! C# test class, same test names, expected values and tolerances.

#[path = "device_scale_agg_sharp/support.rs"]
mod support;

#[path = "device_scale_agg_sharp/design_unit_leftover_device_scale_tests.rs"]
mod design_unit_leftover_device_scale_tests;
#[path = "device_scale_agg_sharp/gui_device_scale_tests.rs"]
mod gui_device_scale_tests;
#[path = "device_scale_agg_sharp/margin_units_device_scale_tests.rs"]
mod margin_units_device_scale_tests;
#[path = "device_scale_agg_sharp/popup_menu_icon_device_scale_tests.rs"]
mod popup_menu_icon_device_scale_tests;
#[path = "device_scale_agg_sharp/slider_device_scale_tests.rs"]
mod slider_device_scale_tests;
#[path = "device_scale_agg_sharp/sub_menu_arrow_device_scale_tests.rs"]
mod sub_menu_arrow_device_scale_tests;
