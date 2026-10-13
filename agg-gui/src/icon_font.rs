//! agg-gui's built-in icon font, named: the bundled Font Awesome 4.7 face
//! ([`crate::fonts::FONT_AWESOME_4_7`]) with a constant for each icon the
//! widgets draw by name, and [`render`] to draw one into a square image placed
//! in the font's em box.  Ports agg-sharp's `Gui/Fonts/IconFont.cs` and
//! `GlyphIcon.RenderInEmBox` (`Gui/Menu/GlyphIcon.cs`).
//!
//! agg-sharp embeds Font Awesome 6 Free Solid; agg-gui bundles 4.7.  Every
//! code point below is the one agg-sharp names, and each is in both faces
//! (FA 6 kept the FA 4 private-use code points for these icons), so the
//! constants mean the same icon in either.  [`NAMED`] lists them with their
//! names — what C#'s tests read by reflection — and `src/tests/icon_font_tests.rs`
//! checks every one against the face's cmap.
//!
//! The markdown format toolbar (`widgets/markdown_editing/format_toolbar.rs`)
//! names its buttons' glyphs from these constants, as agg-sharp's does from
//! `IconFont`, but draws vector icons of its own (`toolbar_icons`), whose
//! bars sit on whole device pixels at any scale.

use std::sync::{Arc, OnceLock};

use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::text::{measure_advance, Font};

pub const BOLD: char = '\u{F032}';
pub const ITALIC: char = '\u{F033}';
pub const STRIKETHROUGH: char = '\u{F0CC}';
pub const CODE: char = '\u{F121}';
pub const ALIGN_LEFT: char = '\u{F036}';
pub const ALIGN_CENTER: char = '\u{F037}';
pub const ALIGN_RIGHT: char = '\u{F038}';
pub const LIST_UL: char = '\u{F0CA}';
pub const LIST_OL: char = '\u{F0CB}';
pub const LIST_CHECK: char = '\u{F0AE}';
pub const QUOTE_LEFT: char = '\u{F10D}';
pub const LINK: char = '\u{F0C1}';
pub const FILE_CODE: char = '\u{F1C9}';
pub const TABLE: char = '\u{F0CE}';
pub const EYE: char = '\u{F06E}';
pub const EYE_SLASH: char = '\u{F070}';
pub const CHEVRON_DOWN: char = '\u{F078}';

// FA 6 keeps the FA 4 private use code points for these, so the names the
// GUI demo used still draw the same icon.  The FA 6 name is given where it
// differs.
pub const PLUS: char = '\u{F067}';
pub const FOLDER_OPEN: char = '\u{F07C}';
pub const ARROW_ROTATE_LEFT: char = '\u{F0E2}';
pub const UNDO: char = ARROW_ROTATE_LEFT;
pub const ARROW_ROTATE_RIGHT: char = '\u{F01E}';
pub const REDO: char = ARROW_ROTATE_RIGHT;
pub const COPY: char = '\u{F0C5}';
pub const SCISSORS: char = '\u{F0C4}';
pub const CUT: char = SCISSORS;
pub const CARET_RIGHT: char = '\u{F0DA}';
pub const COMMENT: char = '\u{F075}';
pub const ARROW_UP: char = '\u{F062}';
pub const ARROW_DOWN: char = '\u{F063}';
pub const MAGNIFYING_GLASS: char = '\u{F002}';
pub const SEARCH: char = MAGNIFYING_GLASS;
pub const XMARK: char = '\u{F00D}';
pub const CLEAR: char = XMARK;

/// Every named constant with its C# name, in declaration order (C# reads
/// them by reflection).
pub const NAMED: &[(&str, char)] = &[
    ("Bold", BOLD),
    ("Italic", ITALIC),
    ("Strikethrough", STRIKETHROUGH),
    ("Code", CODE),
    ("AlignLeft", ALIGN_LEFT),
    ("AlignCenter", ALIGN_CENTER),
    ("AlignRight", ALIGN_RIGHT),
    ("ListUl", LIST_UL),
    ("ListOl", LIST_OL),
    ("ListCheck", LIST_CHECK),
    ("QuoteLeft", QUOTE_LEFT),
    ("Link", LINK),
    ("FileCode", FILE_CODE),
    ("Table", TABLE),
    ("Eye", EYE),
    ("EyeSlash", EYE_SLASH),
    ("ChevronDown", CHEVRON_DOWN),
    ("Plus", PLUS),
    ("FolderOpen", FOLDER_OPEN),
    ("ArrowRotateLeft", ARROW_ROTATE_LEFT),
    ("Undo", UNDO),
    ("ArrowRotateRight", ARROW_ROTATE_RIGHT),
    ("Redo", REDO),
    ("Copy", COPY),
    ("Scissors", SCISSORS),
    ("Cut", CUT),
    ("CaretRight", CARET_RIGHT),
    ("Comment", COMMENT),
    ("ArrowUp", ARROW_UP),
    ("ArrowDown", ARROW_DOWN),
    ("MagnifyingGlass", MAGNIFYING_GLASS),
    ("Search", SEARCH),
    ("Xmark", XMARK),
    ("Clear", CLEAR),
];

/// The icon face alone, with no fallback chain, parsed on first use (C#'s
/// lazy `IconFont.TypeFace`).  Without a fallback, [`Font::has_glyph`] answers
/// for this face only.
pub fn type_face() -> Arc<Font> {
    static FACE: OnceLock<Arc<Font>> = OnceLock::new();
    Arc::clone(FACE.get_or_init(|| {
        Arc::new(
            Font::from_slice(crate::fonts::FONT_AWESOME_4_7)
                .expect("bundled Font Awesome is valid"),
        )
    }))
}

/// Draws `glyph` from the icon face into a `size` pixel square image placed
/// in the face's em box, so a row of icons shares one baseline and one
/// height (C#'s `IconFont.Render`).  `None` when the size is not positive or
/// the face has no outline for the glyph.
pub fn render(glyph: char, color: Color, size: i32) -> Option<Framebuffer> {
    render_in_em_box(glyph, &type_face(), color, size)
}

/// `GlyphIcon.RenderInEmBox`: the box is the glyph's advance wide and the
/// face's ascent to descent tall, centred in the square and scaled down (never
/// up) until it fits.  The image is Y-up (row 0 the bottom), as agg-sharp's
/// `ImageBuffer`, with premultiplied colour.
pub fn render_in_em_box(
    glyph: char,
    font: &Arc<Font>,
    color: Color,
    size: i32,
) -> Option<Framebuffer> {
    if size <= 0 {
        return None;
    }
    let px = size as f64;
    // C#'s `StyledTypeFace(face, size * 72 / 96)` is an em of `size` pixels.
    let (x_min, y_min, x_max, y_max) = font.glyph_visual_rect(glyph, px)?;
    if x_max - x_min <= 0.0 || y_max - y_min <= 0.0 {
        return None;
    }

    // The outline's pen is at the origin and its baseline at y = 0; the
    // descent is negative, below the baseline.
    let advance = measure_advance(font, &glyph.to_string(), px);
    let box_width = if advance > 0.0 { advance } else { x_max };
    let mut box_bottom = -font.descender_px(px);
    let mut box_height = font.ascender_px(px) - box_bottom;
    if box_height <= 0.0 {
        box_bottom = y_min;
        box_height = y_max - y_min;
    }

    let scale = (px / box_width.max(box_height)).min(1.0);
    let mut image = Framebuffer::new(size as u32, size as u32);
    {
        let mut ctx = GfxCtx::new(&mut image);
        // C#: translate(-w/2, -(bottom + h/2)), then scale, then translate to
        // the square's centre, composed left to right on row vectors; the CTM
        // here applies its last call first.
        ctx.translate(px / 2.0, px / 2.0);
        ctx.scale(scale, scale);
        ctx.translate(-box_width / 2.0, -(box_bottom + box_height / 2.0));
        ctx.set_font(Arc::clone(font));
        ctx.set_font_size(px);
        ctx.set_fill_color(color);
        ctx.fill_text(&glyph.to_string(), 0.0, 0.0);
    }
    Some(image)
}
