//! agg-sharp's `FlowLayoutWidget` tests (`Tests/Agg.Tests/Agg Automation
//! Tests/FlowLayoutTests.cs`), ported against agg-gui's `FlexColumn` /
//! `FlexRow` inside `AbsoluteLayout` (C#'s plain `GuiWidget` parent). Same
//! test names, expected values and tolerances; the conversions are listed in
//! the class file's header.

#[path = "flow_layout_agg_sharp/flow_layout_tests.rs"]
mod flow_layout_tests;
#[path = "flow_layout_agg_sharp/flow_layout_tests_anchors.rs"]
mod flow_layout_tests_anchors;
#[path = "flow_layout_agg_sharp/support.rs"]
mod support;
