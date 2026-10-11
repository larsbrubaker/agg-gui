//! The fonts and spacing the rich editor lays text out with — the layout half
//! of agg-sharp's `RichLayoutStyle` (`MarkdigAgg/Editing/RichLayoutStyle.cs`).
//!
//! The numbers copy the markdown viewer's renderers (TextWordX, HeadingRowX,
//! ListX, QuoteBlockX, CodeBlockX) so a document looks the same in the Edit
//! tab as in Help.  Font sizes and spacing are scaled when the style is built,
//! the way a TextWidget scales its point size: rebuild the style when the
//! device scale or the text size setting changes.
//!
//! agg-sharp reads its faces from `AggContext.DefaultFont`,
//! `AggContext.DefaultFontBold` and `CodeBlockX.GetMonoTypeFace()`; agg-gui
//! never loads a font implicitly, so the host hands them in as
//! [`RichLayoutFonts`].  A host without a bold face gets the regular one faux
//! bolded (as `MarkdownView` does), and one without a monospace face draws
//! code in the regular face.  Italic is the face sheared
//! ([`Font::variant_with_faux_style`], `StyledTypeFace.FauxItalic`), which
//! keeps every advance.
//!
//! [`super::block_layout`] measures with the [`RichFace`]s this hands out.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::geometry::Size;
use crate::text::{measure_advance, Font};

use super::block::{RichBlock, RichBlockKind};
use super::inline::{InlineAtom, RichInline};

/// agg-sharp's `StyledTypeFace.PointsPerInch`.
const POINTS_PER_INCH: f64 = 72.0;
/// agg-sharp's `StyledTypeFace.PixelsPerInch`.
const PIXELS_PER_INCH: f64 = 96.0;

/// The faces the editor draws with.
#[derive(Clone)]
pub struct RichLayoutFonts {
    /// Body text (C# `AggContext.DefaultFont`).
    pub regular: Arc<Font>,
    /// Bold runs, headings and table headers (C# `AggContext.DefaultFontBold`);
    /// `None` faux-bolds `regular`.
    pub bold: Option<Arc<Font>>,
    /// Code blocks and inline code (C# `CodeBlockX.GetMonoTypeFace()`); `None`
    /// uses `regular`.
    pub mono: Option<Arc<Font>>,
}

/// One font at one pixel size — agg-sharp's `StyledTypeFace` as the layout
/// uses it.  Cheap to clone: the font is shared.
#[derive(Clone)]
pub struct RichFace {
    font: Arc<Font>,
    em_px: f64,
    underline: bool,
}

impl std::fmt::Debug for RichFace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RichFace")
            .field("font", &Arc::as_ptr(&self.font))
            .field("em_px", &self.em_px)
            .field("underline", &self.underline)
            .finish()
    }
}

impl PartialEq for RichFace {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.font, &other.font)
            && self.em_px == other.em_px
            && self.underline == other.underline
    }
}

impl RichFace {
    /// The font to draw with (already slanted or thickened as styled).
    pub fn font(&self) -> &Arc<Font> {
        &self.font
    }

    /// The em size in pixels (C# `EmSizeInPixels`), the size to draw at.
    pub fn em_px(&self) -> f64 {
        self.em_px
    }

    /// Whether text in this face is underlined (a link).
    pub fn underline(&self) -> bool {
        self.underline
    }

    /// Height above the baseline (C# `AscentInPixels`).
    pub fn ascent_px(&self) -> f64 {
        self.font.ascender_px(self.em_px)
    }

    /// Depth below the baseline, positive (C# `Math.Abs(DescentInPixels)`).
    pub fn descent_px(&self) -> f64 {
        self.font.descender_px(self.em_px)
    }

    /// C# `XHeightInPixels`.
    pub fn x_height_px(&self) -> f64 {
        self.font.x_height_px(self.em_px)
    }

    /// One character's advance (C# `GetAdvanceForCharacter`).  The faces do
    /// no kerning, so summing these gives a string's width.
    pub fn advance(&self, c: char) -> f64 {
        let mut buffer = [0u8; 4];
        measure_advance(&self.font, c.encode_utf8(&mut buffer), self.em_px)
    }

    /// The width of `text`: the sum of its characters' advances (C#
    /// `RichLayoutItems.Measure`).
    pub fn measure(&self, text: &str) -> f64 {
        text.chars().map(|c| self.advance(c)).sum()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum FaceKind {
    Regular,
    Bold,
    Mono,
}

/// A callback giving an image atom's size.
pub type RichImageSize = Box<dyn Fn(&InlineAtom) -> Size>;
/// A callback giving a Raw block's size from the block and the available width.
pub type RichRawBlockSize = Box<dyn Fn(&RichBlock, f64) -> Size>;

/// The fonts and spacing a document is laid out with (C# `RichLayoutStyle`).
pub struct RichLayoutStyle {
    fonts: RichLayoutFonts,
    faces: RefCell<HashMap<(FaceKind, u64, bool, bool), RichFace>>,
    scale: f64,
    font_scale: f64,
    /// Body text size in points.
    pub body_point_size: f64,
    /// Code text size in points.
    pub code_point_size: f64,
    /// The size an image atom takes.  The editor widget supplies loaded image
    /// sizes; while this is `None` an image is a placeholder square one body
    /// line tall.
    pub image_size: Option<RichImageSize>,
    /// The box a Raw block (shown rendered by a child widget) takes, given the
    /// block and the available width.  While this is `None` a Raw block is
    /// one body line across the width.
    pub raw_block_size: Option<RichRawBlockSize>,
}

impl RichLayoutStyle {
    /// A style over `fonts` at `device_scale` (the process device scale when
    /// `None`), with the text size setting
    /// ([`crate::font_settings::current_font_size_scale`]) applied.
    pub fn new(fonts: RichLayoutFonts, device_scale: Option<f64>) -> Self {
        let scale = device_scale.unwrap_or_else(crate::device_scale::device_scale);
        Self {
            fonts,
            faces: RefCell::new(HashMap::new()),
            scale,
            font_scale: scale * crate::font_settings::current_font_size_scale(),
            body_point_size: 10.0,
            code_point_size: 10.0,
            image_size: None,
            raw_block_size: None,
        }
    }

    /// The device scale every spacing value below is already multiplied by.
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// The bullet an unordered list item shows at `depth`: a filled dot, then
    /// a hollow one, then a small square, repeating.  The painter draws the
    /// shape itself; the saved markdown keeps the author's "-", "*" or "+".
    pub fn bullet_marker(&self, depth: usize) -> &'static str {
        match depth % 3 {
            0 => "\u{2022}",
            1 => "\u{25E6}",
            _ => "\u{25AA}",
        }
    }

    /// The width a drawn bullet takes in the gutter: the face's x-height.
    pub fn bullet_box_width(face: &RichFace) -> f64 {
        face.x_height_px()
    }

    /// How far each list depth moves the item right (the viewer's ListX margin).
    pub fn list_indent(&self) -> f64 {
        14.0 * self.scale
    }

    /// The fixed column a list marker sits in, so text lines up across items.
    pub fn marker_gutter(&self) -> f64 {
        18.0 * self.scale
    }

    /// The space between a list marker's right edge and the item text.
    pub fn marker_gap(&self) -> f64 {
        4.0 * self.scale
    }

    /// The width of a quote's bar.
    pub fn quote_bar_width(&self) -> f64 {
        2.0 * self.scale
    }

    /// Where quote text starts, measured from the bar's left edge.
    pub fn quote_indent(&self) -> f64 {
        12.0 * self.scale
    }

    /// Space inside a code block's shaded background, on every side.
    pub fn code_padding(&self) -> f64 {
        6.0 * self.scale
    }

    /// Space added to every line's ascent + descent, half above and half below.
    pub fn line_gap(&self) -> f64 {
        6.0 * self.scale
    }

    /// The caret's width.
    pub fn caret_width(&self) -> f64 {
        self.scale
    }

    /// The heading sizes HeadingRowX uses, in points.
    pub fn heading_point_size(level: u8) -> f64 {
        match level {
            1 => 20.0,
            2 => 17.0,
            3 => 15.0,
            4 => 13.0,
            5 => 12.0,
            _ => 11.0,
        }
    }

    /// Space `(before, after)` a block, matching the viewer's margins.  A
    /// list's last item (`ends_list`) takes a paragraph's 12 below.
    pub fn block_spacing(&self, block: &RichBlock, ends_list: bool) -> (f64, f64) {
        let (before, after) = match block.kind {
            RichBlockKind::Heading => (12.0, 4.0),
            RichBlockKind::ListItem => (0.0, if ends_list { 12.0 } else { 3.0 }),
            // The viewer's AggTable sits 12 below whatever is above it.
            RichBlockKind::Table => (12.0, 12.0),
            _ => (0.0, 12.0),
        };
        (before * self.scale, after * self.scale)
    }

    /// The face a block's unstyled text is drawn in; every line is at least
    /// as tall as this face, so an empty line still holds a caret.
    pub fn block_face(&self, block: &RichBlock) -> RichFace {
        match block.kind {
            RichBlockKind::Heading => self.face(
                FaceKind::Bold,
                Self::heading_point_size(block.heading_level),
                false,
                false,
            ),
            RichBlockKind::CodeBlock => {
                self.face(FaceKind::Mono, self.code_point_size, false, false)
            }
            _ => self.face(FaceKind::Regular, self.body_point_size, false, false),
        }
    }

    /// The face a run is drawn in within its block: italic shears, inline
    /// code is monospace, links are underlined, headings are bold.
    pub fn run_face(&self, block: &RichBlock, run: &RichInline) -> RichFace {
        let heading = block.kind == RichBlockKind::Heading;
        let points = if heading {
            Self::heading_point_size(block.heading_level)
        } else {
            self.body_point_size
        };
        let code = run.as_run().is_some_and(|r| r.code);
        let kind = if code {
            FaceKind::Mono
        } else if run.bold || heading {
            FaceKind::Bold
        } else {
            FaceKind::Regular
        };
        self.face(kind, points, run.italic, run.link_url.is_some())
    }

    /// A table cell's unstyled face: body text, bold in the header row.
    pub fn table_cell_face(&self, header: bool) -> RichFace {
        let kind = if header {
            FaceKind::Bold
        } else {
            FaceKind::Regular
        };
        self.face(kind, self.body_point_size, false, false)
    }

    /// The face a run in a table cell is drawn in: [`Self::run_face`]'s body
    /// rules, with every header run bold.
    pub fn table_run_face(&self, run: &RichInline, header: bool) -> RichFace {
        let code = run.as_run().is_some_and(|r| r.code);
        let kind = if code {
            FaceKind::Mono
        } else if run.bold || header {
            FaceKind::Bold
        } else {
            FaceKind::Regular
        };
        self.face(
            kind,
            self.body_point_size,
            run.italic,
            run.link_url.is_some(),
        )
    }

    fn face(&self, kind: FaceKind, points: f64, italic: bool, underline: bool) -> RichFace {
        let key = (kind, points.to_bits(), italic, underline);
        self.faces
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                let (base, faux_bold) = match kind {
                    FaceKind::Regular => (&self.fonts.regular, false),
                    FaceKind::Bold => match &self.fonts.bold {
                        Some(bold) => (bold, false),
                        None => (&self.fonts.regular, true),
                    },
                    FaceKind::Mono => (
                        self.fonts.mono.as_ref().unwrap_or(&self.fonts.regular),
                        false,
                    ),
                };
                // Kept in the cache so the text mask cache, keyed on the Arc,
                // sees one font per style across frames.
                let font = if italic || faux_bold {
                    Arc::new(base.variant_with_faux_style(italic, faux_bold))
                } else {
                    Arc::clone(base)
                };
                RichFace {
                    font,
                    em_px: points * self.font_scale / POINTS_PER_INCH * PIXELS_PER_INCH,
                    underline,
                }
            })
            .clone()
    }
}
