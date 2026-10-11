//! Loading an SVG document referenced by an `<image>` element.
//!
//! An SVG image draws none of its own `<image>` elements, as usvg did up to
//! 0.45 and as agg-sharp does (its SvgImageTests' "an SVG image draws no images
//! of its own"; resvg's structure/image/recursive-2, where a document including
//! itself shows once with an empty frame inside). usvg 0.46+ still loads a
//! nested document's data-URL images, so `parse_svg` (in `svg.rs`) installs the
//! resolver built here, which parses every nested SVG with an image resolver
//! that resolves nothing. Raster images go through usvg's own resolvers.

use std::path::Path;
use std::sync::Arc;

/// usvg's image resolvers, except that an SVG image is parsed with images off.
pub(super) fn image_href_resolver() -> usvg::ImageHrefResolver<'static> {
    let default_data = usvg::ImageHrefResolver::default_data_resolver();
    let default_string = usvg::ImageHrefResolver::default_string_resolver();
    usvg::ImageHrefResolver {
        resolve_data: Box::new(
            move |mime: &str, data: Arc<Vec<u8>>, opts: &usvg::Options| {
                if mime == "image/svg+xml" {
                    return load_sub_svg(&data, opts);
                }
                // `text/plain` data that is no raster format is taken as SVG.
                match default_data(mime, Arc::clone(&data), opts)? {
                    usvg::ImageKind::SVG(_) => load_sub_svg(&data, opts),
                    raster => Some(raster),
                }
            },
        ),
        resolve_string: Box::new(move |href: &str, opts: &usvg::Options| {
            match default_string(href, opts)? {
                usvg::ImageKind::SVG(_) => {
                    let data = std::fs::read(opts.get_abs_path(Path::new(href))).ok()?;
                    load_sub_svg(&data, opts)
                }
                raster => Some(raster),
            }
        }),
    }
}

/// Parses a nested SVG document with the parent's settings and no images
/// (usvg 0.45's `load_sub_svg`).
fn load_sub_svg(data: &[u8], opts: &usvg::Options) -> Option<usvg::ImageKind> {
    let nested = usvg::Options {
        resources_dir: None,
        dpi: opts.dpi,
        font_family: opts.font_family.clone(),
        font_size: opts.font_size,
        languages: opts.languages.clone(),
        shape_rendering: opts.shape_rendering,
        text_rendering: opts.text_rendering,
        image_rendering: opts.image_rendering,
        default_size: opts.default_size,
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        fontdb: Arc::clone(&opts.fontdb),
        ..usvg::Options::default()
    };
    usvg::Tree::from_data(data, &nested)
        .ok()
        .map(usvg::ImageKind::SVG)
}
