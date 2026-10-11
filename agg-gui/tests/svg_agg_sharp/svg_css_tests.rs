//! agg-sharp's `SvgCssTests` (agg/Svg, stage 5: `<style>` sheets and the
//! cascade they join), ported 1:1 where they render.
//!
//! The other six tests (`TypeClassAndIdSelectorsApplyByTheirSpecificity`,
//! `TheLaterOfTwoEqualRulesWinsAndTheStyleAttributeBeatsBoth`,
//! `ImportantRulesBeatTheStyleAttribute`,
//! `DescendantChildAndUniversalCombinatorsMatchTheirTrees`,
//! `OnlyPresentationPropertiesAreTakenFromStyles`,
//! `CommentsInAStyleAttributeAreIgnored`) read attributes back from
//! agg-sharp's own parsed document (`GetElementById(..)["fill"]`); agg-gui
//! parses with usvg, so they have no counterpart here.

use super::common::{render_document, Color};

#[test]
fn a_sheet_styles_the_render() {
    let image = render_document("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><style type=\"text/css\">.g { fill: #00ff00 }</style><rect class=\"g\" width=\"100\" height=\"100\" fill=\"red\"/></svg>", 10, 10);
    assert_eq!(image.get_pixel(5, 5), Color::new(0, 255, 0, 255));
}
