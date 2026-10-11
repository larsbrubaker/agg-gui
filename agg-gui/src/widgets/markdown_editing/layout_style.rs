//! The fonts, colours and spacing the rich editor lays text out with — a port
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
//! C#'s constructor reads the text, accent and background colours from an
//! optional `ThemeConfig`; agg-gui has no ThemeConfig, so the host hands in a
//! [`RichThemeColors`] (or `None` for C#'s defaults: black text, a #7AD7F0
//! accent on white).
//!
//! [`super::block_layout`] measures with the [`RichFace`]s this hands out;
//! [`super::block_painter`] paints with its colours.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::color::Color;
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

    /// The underline's `(position, thickness)` in pixels (C#
    /// `UnderlinePositionInPixels`, `UnderlineThicknessInPixels`).
    pub fn underline_metrics_px(&self) -> (f64, f64) {
        self.font.underline_metrics_px(self.em_px)
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

/// The theme colours a style is built from (the parts of C#'s `ThemeConfig`
/// `RichLayoutStyle` reads).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RichThemeColors {
    /// Body text (C# `ThemeConfig.TextColor`).
    pub text: Color,
    /// The accent links are drawn in (C# `ThemeConfig.PrimaryAccentColor`).
    pub accent: Color,
    /// What the text is drawn on (C# `ThemeConfig.BackgroundColor`).
    pub background: Color,
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
    /// The text colour.
    pub text_color: Color,
    /// The link colour: the theme's accent pushed to body-text contrast.
    pub link_color: Color,
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
    /// C#'s default colours (no theme); see [`Self::with_theme`].
    pub fn new(fonts: RichLayoutFonts, device_scale: Option<f64>) -> Self {
        Self::with_theme(fonts, None, device_scale)
    }

    /// A style over `fonts` with `theme`'s colours (C#'s defaults when
    /// `None`) at `device_scale` (the process device scale when `None`).
    pub fn with_theme(
        fonts: RichLayoutFonts,
        theme: Option<&RichThemeColors>,
        device_scale: Option<f64>,
    ) -> Self {
        let scale = device_scale.unwrap_or_else(crate::device_scale::device_scale);
        let text_color = theme.map_or(Color::black(), |t| t.text);
        // A link reads as a link, as in any editor: the theme's accent pushed
        // to body-text contrast (4.5) on the background, so it stays readable
        // on light and dark themes alike (Hyperlink's rule, at text contrast).
        let accent = theme.map_or(Color::from_rgb8(0x7A, 0xD7, 0xF0), |t| t.accent);
        let background = theme.map_or(Color::white(), |t| t.background);
        Self {
            fonts,
            faces: RefCell::new(HashMap::new()),
            scale,
            font_scale: scale * crate::font_settings::current_font_size_scale(),
            text_color,
            link_color: accent.with_contrast(background, 4.5),
            body_point_size: 10.0,
            code_point_size: 10.0,
            image_size: None,
            raw_block_size: None,
        }
    }

    /// The alpha (of 255) of the text colour that shades code blocks, inline
    /// code and striped table rows.  Derived from the text colour rather than
    /// a fixed grey, so the shading shows on a dark background as well as a
    /// light one.
    pub const SHADE_ALPHA: u8 = 24;

    /// The text colour at `alpha` of 255 (C# `new Color(TextColor, alpha)`).
    fn text_at_alpha(&self, alpha: u8) -> Color {
        self.text_color.with_alpha(f32::from(alpha) / 255.0)
    }

    /// The shading behind code blocks and inline code: the text colour at
    /// [`Self::SHADE_ALPHA`].
    pub fn code_background_color(&self) -> Color {
        self.text_at_alpha(Self::SHADE_ALPHA)
    }

    /// A table's grid lines: the text colour at the viewer's TableBorderAlpha
    /// (150).
    pub fn table_grid_color(&self) -> Color {
        self.text_at_alpha(150)
    }

    /// The shading of a table's striped body rows: the text colour at
    /// [`Self::SHADE_ALPHA`], the code shading (the viewer's
    /// ZebraStripeAlpha matches it, so Help and Edit stripe alike).
    pub fn table_stripe_color(&self) -> Color {
        self.text_at_alpha(Self::SHADE_ALPHA)
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

    /// Space between a table cell's text and its left and right grid lines
    /// (the viewer's 10 cell padding).
    pub fn table_cell_padding_x(&self) -> f64 {
        10.0 * self.scale
    }

    /// Space between a table cell's text and its top and bottom grid lines
    /// (the viewer's 2 cell padding); each line's own [`Self::line_gap`] adds
    /// to it, as in the viewer, whose rows hold padded text widgets.
    pub fn table_cell_padding_y(&self) -> f64 {
        2.0 * self.scale
    }

    /// A table grid line's thickness: whole device pixels
    /// ([`crate::grid_pixels::line_thickness`]), as the viewer's grid draws,
    /// so a line never lands half on a pixel.
    pub fn table_grid_line_width(&self) -> f64 {
        f64::from(crate::grid_pixels::line_thickness(self.scale))
    }

    /// The narrowest a table column's text gets when the table is squeezed to
    /// fit (a column whose text is narrower keeps its own width), so a
    /// squeezed column still shows a few characters rather than one.
    pub fn table_min_column_text_width(&self) -> f64 {
        40.0 * self.scale
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
