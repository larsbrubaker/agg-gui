//! agg-sharp's layout tests (`Tests/Agg.Tests/Agg.UI`: anchors, flow padding,
//! borders, draw-on-top, text widget sizing), ported against agg-gui's
//! `AbsoluteLayout` (C# `GuiWidget` + `LayoutEngineSimpleAlign`),
//! `FlexColumn`/`FlexRow` (`FlowLayoutWidget`), `Container` and `Label`. One
//! file per C# test class, same test names, expected values and tolerances.

#[path = "layout_agg_sharp/support.rs"]
mod support;

#[path = "layout_agg_sharp/anchor_tests.rs"]
mod anchor_tests;
#[path = "layout_agg_sharp/border_tests.rs"]
mod border_tests;
#[path = "layout_agg_sharp/draw_on_top_of_siblings_tests.rs"]
mod draw_on_top_of_siblings_tests;
#[path = "layout_agg_sharp/flow_layout_padding_tests.rs"]
mod flow_layout_padding_tests;
#[path = "layout_agg_sharp/text_and_text_widget_tests.rs"]
mod text_and_text_widget_tests;
