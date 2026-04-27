pub(super) struct SvgSample {
    pub(super) name: &'static str,
    pub(super) svg: &'static [u8],
    pub(super) reference_png: &'static [u8],
}

macro_rules! sample {
    ($path:literal) => {
        SvgSample {
            name: concat!($path, ".svg"),
            svg: include_bytes!(concat!(
                "../../../../../tests/resvg-test-suite/tests/",
                $path,
                ".svg"
            )),
            reference_png: include_bytes!(concat!(
                "../../../../../tests/resvg-test-suite/tests/",
                $path,
                ".png"
            )),
        }
    };
}

pub(super) const SVG_SAMPLES: &[SvgSample] = &[
    sample!("shapes/rect/simple-case"),
    sample!("shapes/circle/simple-case"),
    sample!("shapes/ellipse/simple-case"),
    sample!("shapes/line/simple-case"),
    sample!("shapes/line/with-transform"),
    sample!("shapes/polygon/simple-case"),
    sample!("shapes/polyline/simple-case"),
    sample!("shapes/path/M-L-L-Z"),
    sample!("shapes/path/M-C"),
    sample!("shapes/path/M-C-S"),
    sample!("shapes/path/M-Q"),
    sample!("shapes/path/M-Q-T"),
    sample!("shapes/path/M-A"),
    sample!("shapes/path/M-L-Z-A"),
    sample!("painting/fill/named-color"),
    sample!("painting/fill/currentColor"),
    sample!("painting/fill/rgb-color"),
    sample!("painting/fill/hsl-with-alpha"),
    sample!("painting/fill/linear-gradient-on-shape"),
    sample!("painting/fill/radial-gradient-on-shape"),
    sample!("painting/fill-rule/nonzero"),
    sample!("painting/fill-rule/evenodd"),
    sample!("painting/opacity/50percent"),
    sample!("painting/opacity/group-opacity"),
    sample!("painting/opacity/mixed-group-opacity"),
    sample!("painting/stroke/line-as-curve-1"),
    sample!("painting/stroke/line-as-curve-2"),
    sample!("painting/stroke/linear-gradient"),
    sample!("painting/stroke/radial-gradient"),
    sample!("paint-servers/linearGradient/gradientUnits=userSpaceOnUse"),
    sample!("paint-servers/linearGradient/gradientUnits=objectBoundingBox-with-percent"),
    sample!("paint-servers/linearGradient/gradientTransform"),
    sample!("paint-servers/linearGradient/gradientTransform-and-transform"),
    sample!("paint-servers/linearGradient/spreadMethod=reflect"),
    sample!("paint-servers/linearGradient/spreadMethod=repeat"),
    sample!("paint-servers/linearGradient/many-stops"),
    sample!("paint-servers/linearGradient/single-stop-with-opacity-used-by-stroke"),
    sample!("paint-servers/radialGradient/gradientUnits=userSpaceOnUse"),
    sample!("paint-servers/radialGradient/gradientUnits=objectBoundingBox-with-percent"),
    sample!("paint-servers/radialGradient/gradientTransform"),
    sample!("paint-servers/radialGradient/focal-point-correction"),
    sample!("paint-servers/radialGradient/spreadMethod=reflect"),
    sample!("paint-servers/radialGradient/spreadMethod=repeat"),
    sample!("paint-servers/radialGradient/many-stops"),
    sample!("paint-servers/pattern/simple-case"),
    sample!("paint-servers/pattern/patternUnits=userSpaceOnUse-with-percent"),
    sample!("paint-servers/pattern/patternContentUnits-with-viewBox"),
    sample!("paint-servers/pattern/transform-and-patternTransform"),
    sample!("structure/image/embedded-png"),
    sample!("structure/image/embedded-jpeg-as-image-jpeg"),
    sample!("structure/image/embedded-gif"),
    sample!("structure/image/embedded-svg"),
    sample!("structure/image/preserveAspectRatio=none"),
    sample!("structure/image/raster-image-and-size-with-odd-numbers"),
];
