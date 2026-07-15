//! LCD subpixel sample-text preview.
//!
//! Formerly the visible half of the stand-alone "LCD Subpixel" demo window
//! (`truetype_lcd.rs`, now removed).  The demo's slider panel duplicated the
//! System window's typography controls, so only the valuable part — four
//! reference paragraphs rendered with the currently-selected system font —
//! survives here as a builder that the System window mounts as its
//! "Sample Text" tab (see `system.rs`).  The paragraphs re-measure and
//! re-raster whenever the shared `font_settings` globals change, so they
//! double as a live preview of every control on the adjacent "Font" tab.

use std::sync::Arc;

use agg_gui::{FlexColumn, Font, Label, ScrollView, Separator, Widget};

// ---------------------------------------------------------------------------
// C++ reference sample paragraphs (from AGG `truetype_test_02_win`)
// ---------------------------------------------------------------------------

const TEXT1: &str = "A single pixel on a color LCD is made of three colored elements \
ordered (on various displays) either as blue, green, and red (BGR), \
or as red, green, and blue (RGB). These pixel components, sometimes \
called sub-pixels, appear as a single color to the human eye because \
of blurring by the optics and spatial integration by nerve cells in the eye.";

const TEXT2: &str = "The components are easily visible, however, when viewed with \
a small magnifying glass, such as a loupe. Over a certain resolution \
range the colors in the sub-pixels are not visible, but the relative \
intensity of the components shifts the apparent position or orientation \
of a line. Methods that take this interaction between the display \
technology and the human visual system into account are called \
subpixel rendering algorithms.";

const TEXT3: &str = "The resolution at which colored sub-pixels go unnoticed differs, \
however, with each user some users are distracted by the colored \
\"fringes\" resulting from sub-pixel rendering. Subpixel rendering \
is better suited to some display technologies than others. The \
technology is well-suited to LCDs, but less so for CRTs. In a CRT \
the light from the pixel components often spread across pixels, \
and the outputs of adjacent pixels are not perfectly independent.";

const TEXT4: &str = "If a designer knew precisely a great deal about the display's \
electron beams and aperture grille, subpixel rendering might \
have some advantage. But the properties of the CRT components, \
coupled with the alignment variations that are part of the \
production process, make subpixel rendering less effective for \
these displays. The technique should have good application to \
organic light emitting diodes and other display technologies.";

/// Build the sample-text preview.  Rendered inside a `ScrollView` so the four
/// paragraphs scroll independently of the System window's other tabs.
pub fn sample_text_tab(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new().with_gap(10.0).with_padding(14.0);

    col.push(
        Box::new(
            Label::new(
                "Reference paragraphs rendered with the current system font.  \
                 Adjust the controls on the Font tab to preview their effect.",
                Arc::clone(&font),
            )
            .with_font_size(13.0)
            .with_wrap(true),
        ),
        0.0,
    );
    col.push(Box::new(Separator::horizontal()), 0.0);

    for text in [TEXT1, TEXT2, TEXT3, TEXT4] {
        col.push(
            Box::new(
                Label::new(text, Arc::clone(&font))
                    .with_font_size(14.0)
                    .with_wrap(true),
            ),
            0.0,
        );
    }

    Box::new(ScrollView::new(Box::new(col)))
}
