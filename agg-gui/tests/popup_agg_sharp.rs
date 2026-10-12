//! agg-sharp's popup and menu tests (`Tests/Agg.Tests/Agg.UI/Popup*Tests.cs`,
//! `Menu*Tests.cs`), ported against agg-gui's popup controller
//! (`widgets/popup`) and menu system (`widgets/menu`). One file per C# test
//! class, same test names, expected values and tolerances.

#[path = "popup_agg_sharp/anchored_popup_tests.rs"]
mod anchored_popup_tests;
#[path = "popup_agg_sharp/narrow_sub_menu_placement_tests.rs"]
mod narrow_sub_menu_placement_tests;
#[path = "popup_agg_sharp/popup_anchor_tests.rs"]
mod popup_anchor_tests;
#[path = "popup_agg_sharp/popup_direction_flip_tests.rs"]
mod popup_direction_flip_tests;
#[path = "popup_agg_sharp/popup_horizontal_clamp_tests.rs"]
mod popup_horizontal_clamp_tests;
#[path = "popup_agg_sharp/popup_menu_keyboard_scroll_tests.rs"]
mod popup_menu_keyboard_scroll_tests;
#[path = "popup_agg_sharp/popup_menu_overflow_scroll_tests.rs"]
mod popup_menu_overflow_scroll_tests;
#[path = "popup_agg_sharp/popup_sub_menu_scroll_tests.rs"]
mod popup_sub_menu_scroll_tests;
