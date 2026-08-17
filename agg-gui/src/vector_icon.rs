//! Host-registered vector icons — small, theme-aware line art that
//! widgets can name by id instead of carrying draw code or bitmaps.
//!
//! ## Why this exists
//!
//! Schema types such as [`EditorKind`](crate::widgets::EditorKind) are
//! cheap-to-clone *data*: they travel through property snapshots, get
//! compared, and cross crate boundaries many times per frame. An editor
//! that wants to show icons therefore names them with strings
//! (`"boolean.combine"`) and the *host* registers the artwork once at
//! startup through [`register_icon`]. The renderer looks the id up at
//! paint time and falls back to text when nothing is registered, so a
//! missing icon degrades instead of blanking the control.
//!
//! ## Shape of an icon
//!
//! A [`VectorIcon`] is a square `view_box` plus an ordered list of
//! filled [`IconPath`]s — painter's algorithm, first path at the back.
//! Each path stores already-flattened closed contours, so painting is
//! nothing but `move_to` / `line_to` / `fill` on any [`DrawCtx`]: no
//! closures, no captured state, and the geometry is inspectable from
//! tests. Contours are produced from ordinary SVG path data by
//! [`path_data::parse_path`], which lets a host paste the `d` attribute
//! of hand-authored artwork in verbatim.
//!
//! ## Colour roles
//!
//! Fills are [`IconColor`] *roles*, not colours, and resolve at paint
//! time: [`IconColor::Ink`] becomes the caller's ink colour (the theme
//! text colour, so outlines follow light/dark themes), while
//! [`IconColor::Literal`] passes through untouched for colours that
//! encode *state* rather than chrome. This mirrors MatterCAD's
//! `GrayToColor` recolouring rule, where a desaturated grey is chrome
//! and anything saturated is meaning.
//!
//! ## Coordinates
//!
//! Icon space is **SVG space**: origin top-left, +Y downward, `0..view_box`
//! on both axes. agg-gui is Y-up, so [`VectorIcon::paint`] flips Y while
//! mapping into the destination rect. Authoring stays copy-paste
//! compatible with SVG; nothing downstream has to think about it.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::{DrawCtx, FillRule};
use crate::geometry::Rect;

pub mod path_data;
mod registry;

#[cfg(test)]
mod tests;

pub use path_data::{parse_path, IconPathError};
pub use registry::{icon, icon_ids, register_icon};

/// One fill colour of an icon, resolved when the icon is painted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IconColor {
    /// UI chrome — outlines, glyphs. Painted in the caller's ink
    /// colour, which is normally the theme's text colour, so the
    /// linework follows the theme.
    Ink,
    /// A literal colour that must survive theming unchanged, because it
    /// encodes state (MatterCAD's "removed material" grey, "kept
    /// material" blue, "retained remover" red).
    Literal(Color),
}

impl IconColor {
    /// The colour to fill with, given the caller's ink colour.
    pub fn resolve(self, ink: Color) -> Color {
        match self {
            IconColor::Ink => ink,
            IconColor::Literal(c) => c,
        }
    }
}

/// One filled sub-shape of an icon: a set of closed contours in icon
/// space, a colour role, and the fill rule that combines the contours
/// (`EvenOdd` is how SVG artwork punches a hole through a ring).
#[derive(Clone, Debug, PartialEq)]
pub struct IconPath {
    pub contours: Vec<Vec<[f64; 2]>>,
    pub fill: IconColor,
    pub fill_rule: FillRule,
}

/// A resolution-independent icon: a square `view_box` and the filled
/// paths inside it, back to front.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorIcon {
    /// Side length of the (square) authoring box, e.g. `64.0` for a
    /// `viewBox="0 0 64 64"` SVG.
    pub view_box: f64,
    pub paths: Vec<IconPath>,
}

impl VectorIcon {
    /// An empty icon with the given square view box.
    pub fn new(view_box: f64) -> Self {
        Self {
            view_box,
            paths: Vec::new(),
        }
    }

    /// Append a filled path parsed from SVG path data (`M`, `L`, `H`,
    /// `V`, `C`, `A`, `Z`, absolute or relative).
    ///
    /// Returns the parse error rather than dropping the path: an icon
    /// that silently lost a contour would render as subtly wrong art
    /// with nothing to point at.
    pub fn with_svg_path(
        mut self,
        d: &str,
        fill: IconColor,
        fill_rule: FillRule,
    ) -> Result<Self, IconPathError> {
        let contours = parse_path(d)?;
        self.paths.push(IconPath {
            contours,
            fill,
            fill_rule,
        });
        Ok(self)
    }

    /// `with_svg_path` with the default non-zero fill rule.
    pub fn with_svg_path_nonzero(self, d: &str, fill: IconColor) -> Result<Self, IconPathError> {
        self.with_svg_path(d, fill, FillRule::NonZero)
    }

    /// Total number of points across every contour — a cheap "is there
    /// any art here?" probe for tests and diagnostics.
    pub fn point_count(&self) -> usize {
        self.paths
            .iter()
            .flat_map(|p| p.contours.iter())
            .map(|c| c.len())
            .sum()
    }

    /// Bounding box `[min_x, min_y, max_x, max_y]` in icon space, or
    /// `None` when the icon has no points.
    pub fn bounds(&self) -> Option<[f64; 4]> {
        let mut b: Option<[f64; 4]> = None;
        for p in &self.paths {
            for c in &p.contours {
                for pt in c {
                    b = Some(match b {
                        None => [pt[0], pt[1], pt[0], pt[1]],
                        Some(v) => [
                            v[0].min(pt[0]),
                            v[1].min(pt[1]),
                            v[2].max(pt[0]),
                            v[3].max(pt[1]),
                        ],
                    });
                }
            }
        }
        b
    }

    /// Paint the icon centred inside `rect`, scaled to fit (uniformly,
    /// aspect preserved) and flipped from SVG's Y-down space into
    /// agg-gui's Y-up space. `ink` resolves [`IconColor::Ink`] fills.
    pub fn paint(&self, ctx: &mut dyn DrawCtx, rect: Rect, ink: Color) {
        if self.view_box <= 0.0 || self.paths.is_empty() {
            return;
        }
        let side = rect.width.min(rect.height);
        if side <= 0.0 {
            return;
        }
        let k = side / self.view_box;
        let ox = rect.x + (rect.width - side) * 0.5;
        // Y flip: icon y = 0 is the *top* of the box, which is the
        // highest y in agg-gui's bottom-left origin space.
        let top = rect.y + (rect.height + side) * 0.5;

        for path in &self.paths {
            if path.contours.iter().all(|c| c.len() < 2) {
                continue;
            }
            ctx.set_fill_color(path.fill.resolve(ink));
            ctx.set_fill_rule(path.fill_rule);
            ctx.begin_path();
            for contour in &path.contours {
                if contour.len() < 2 {
                    continue;
                }
                for (i, pt) in contour.iter().enumerate() {
                    let x = ox + pt[0] * k;
                    let y = top - pt[1] * k;
                    if i == 0 {
                        ctx.move_to(x, y);
                    } else {
                        ctx.line_to(x, y);
                    }
                }
                ctx.close_path();
            }
            ctx.fill();
        }
        // Leave the shared context on the default rule; a later fill
        // that never asked for even-odd must not inherit it.
        ctx.set_fill_rule(FillRule::NonZero);
    }
}

/// Convenience for hosts: build an icon from `(path data, colour role,
/// fill rule)` triples in painter order.
pub fn icon_from_svg_paths(
    view_box: f64,
    paths: &[(&str, IconColor, FillRule)],
) -> Result<VectorIcon, IconPathError> {
    let mut icon = VectorIcon::new(view_box);
    for (d, fill, rule) in paths {
        icon = icon.with_svg_path(d, *fill, *rule)?;
    }
    Ok(icon)
}

/// Register `icon` under `id`, replacing any icon already registered
/// there. Sugar over [`register_icon`] for the build-then-register
/// flow.
pub fn register_svg_icon(
    id: impl Into<Arc<str>>,
    view_box: f64,
    paths: &[(&str, IconColor, FillRule)],
) -> Result<(), IconPathError> {
    let icon = icon_from_svg_paths(view_box, paths)?;
    register_icon(id, icon);
    Ok(())
}
