//! agg-sharp's slider and drop-down look tests
//! (`Tests/Agg.Tests/Agg.UI/SliderAndComboRestyleTests.cs`,
//! `DropDownListChromeTests.cs`): agg-gui's `Slider` and `ComboBox` (C#'s
//! `DropDownList`) painted into a software framebuffer and sampled. One file
//! per C# test class, same test names.

#[path = "slider_combo_agg_sharp/support.rs"]
mod support;

#[path = "slider_combo_agg_sharp/drop_down_list_chrome_tests.rs"]
mod drop_down_list_chrome_tests;
#[path = "slider_combo_agg_sharp/slider_and_combo_restyle_tests.rs"]
mod slider_and_combo_restyle_tests;
