//! agg-sharp's SVG rendering tests (`Tests/Agg.Tests/Agg/Svg*Tests.cs`), ported
//! 1:1 against agg-gui's SVG renderer, which plays the part of agg-sharp's
//! `SvgDocument.RenderToImage`. One file per C# test class, same test names,
//! expected values and tolerances; `common` holds the shared render helpers.

#[path = "svg_agg_sharp/common.rs"]
mod common;
#[path = "svg_agg_sharp/svg_clip_mask_tests.rs"]
mod svg_clip_mask_tests;
#[path = "svg_agg_sharp/svg_context_paint_tests.rs"]
mod svg_context_paint_tests;
#[path = "svg_agg_sharp/svg_gradient_tests.rs"]
mod svg_gradient_tests;
#[path = "svg_agg_sharp/svg_marker_tests.rs"]
mod svg_marker_tests;
#[path = "svg_agg_sharp/svg_module_tests.rs"]
mod svg_module_tests;
#[path = "svg_agg_sharp/svg_pattern_tests.rs"]
mod svg_pattern_tests;
#[path = "svg_agg_sharp/svg_structure_tests.rs"]
mod svg_structure_tests;
