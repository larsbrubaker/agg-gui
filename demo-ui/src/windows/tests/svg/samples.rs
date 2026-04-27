pub(super) struct SvgSample {
    pub(super) name: &'static str,
    pub(super) svg: &'static [u8],
    pub(super) reference_png: &'static [u8],
}

pub(super) const SVG_SAMPLES: &[SvgSample] = &[
    SvgSample {
        name: "shapes/rect/simple-case.svg",
        svg: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/shapes/rect/simple-case.svg"
        ),
        reference_png: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/shapes/rect/simple-case.png"
        ),
    },
    SvgSample {
        name: "shapes/path/M-L-L-Z.svg",
        svg: include_bytes!("../../../../../tests/resvg-test-suite/tests/shapes/path/M-L-L-Z.svg"),
        reference_png: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/shapes/path/M-L-L-Z.png"
        ),
    },
    SvgSample {
        name: "painting/stroke/line-as-curve-1.svg",
        svg: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/painting/stroke/line-as-curve-1.svg"
        ),
        reference_png: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/painting/stroke/line-as-curve-1.png"
        ),
    },
    SvgSample {
        name: "structure/image/embedded-png.svg",
        svg: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/structure/image/embedded-png.svg"
        ),
        reference_png: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/structure/image/embedded-png.png"
        ),
    },
    SvgSample {
        name: "paint-servers/linearGradient/gradientUnits=userSpaceOnUse.svg",
        svg: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/linearGradient/gradientUnits=userSpaceOnUse.svg"
        ),
        reference_png: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/linearGradient/gradientUnits=userSpaceOnUse.png"
        ),
    },
    SvgSample {
        name: "paint-servers/linearGradient/gradientTransform.svg",
        svg: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/linearGradient/gradientTransform.svg"
        ),
        reference_png: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/linearGradient/gradientTransform.png"
        ),
    },
    SvgSample {
        name: "paint-servers/linearGradient/spreadMethod=reflect.svg",
        svg: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/linearGradient/spreadMethod=reflect.svg"
        ),
        reference_png: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/linearGradient/spreadMethod=reflect.png"
        ),
    },
    SvgSample {
        name: "paint-servers/linearGradient/spreadMethod=repeat.svg",
        svg: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/linearGradient/spreadMethod=repeat.svg"
        ),
        reference_png: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/linearGradient/spreadMethod=repeat.png"
        ),
    },
    SvgSample {
        name: "paint-servers/linearGradient/many-stops.svg",
        svg: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/linearGradient/many-stops.svg"
        ),
        reference_png: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/linearGradient/many-stops.png"
        ),
    },
    SvgSample {
        name: "paint-servers/linearGradient/single-stop-with-opacity-used-by-stroke.svg",
        svg: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/linearGradient/single-stop-with-opacity-used-by-stroke.svg"
        ),
        reference_png: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/linearGradient/single-stop-with-opacity-used-by-stroke.png"
        ),
    },
    SvgSample {
        name: "paint-servers/radialGradient/gradientUnits=userSpaceOnUse.svg",
        svg: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/radialGradient/gradientUnits=userSpaceOnUse.svg"
        ),
        reference_png: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/radialGradient/gradientUnits=userSpaceOnUse.png"
        ),
    },
    SvgSample {
        name: "paint-servers/radialGradient/gradientTransform.svg",
        svg: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/radialGradient/gradientTransform.svg"
        ),
        reference_png: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/radialGradient/gradientTransform.png"
        ),
    },
    SvgSample {
        name: "paint-servers/radialGradient/focal-point-correction.svg",
        svg: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/radialGradient/focal-point-correction.svg"
        ),
        reference_png: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/radialGradient/focal-point-correction.png"
        ),
    },
    SvgSample {
        name: "paint-servers/radialGradient/spreadMethod=repeat.svg",
        svg: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/radialGradient/spreadMethod=repeat.svg"
        ),
        reference_png: include_bytes!(
            "../../../../../tests/resvg-test-suite/tests/paint-servers/radialGradient/spreadMethod=repeat.png"
        ),
    },
];
