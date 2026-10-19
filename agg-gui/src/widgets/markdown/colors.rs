//! The colours a `MarkdownView` can be given instead of the app's visuals
//! (agg-sharp's `MarkdownWidget` takes its colours from a `ThemeConfig`; agg-gui
//! has no ThemeConfig, so an owner that is themed apart from the app, such as
//! the rich editor's Raw block hosts, hands the view a [`MarkdownColors`]).
//!
//! A view with no colours draws in `DrawCtx::visuals()` exactly as before.
//! [`MarkdownColors::resolve`] merges the two for one paint: `paint.rs`,
//! `cell_images.rs` and the table accessors in `introspect.rs` read the
//! resolved set, so each colour has one source.

use crate::color::Color;
use crate::theme::Visuals;

use super::MarkdownView;

/// The theme colours of one `MarkdownView` (set with
/// [`MarkdownView::with_colors`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkdownColors {
    /// Body text, code text, table text and the strikethrough line.  Table
    /// grid lines and stripes are this colour at the viewer's alphas.
    pub text: Color,
    /// Link text and its underline.
    pub link: Color,
    /// The shading behind code blocks and inline code.
    pub code_background: Color,
    /// Horizontal rules, quote bars and the lines under H1 and H2.
    pub rule: Color,
    /// What the view sits on.  `Some` fills the view's box with it before the
    /// content draws (a standalone themed view); `None` leaves the box
    /// transparent, for a view hosted on a surface its owner paints (the rich
    /// editor paints its selection beneath its hosts, so they draw no fill).
    pub background: Option<Color>,
}

/// The colours one paint uses: the view's [`MarkdownColors`] where it has
/// them, the visuals' otherwise.
#[derive(Clone, Copy)]
pub(super) struct ResolvedColors {
    pub text: Color,
    pub dim_text: Color,
    pub link: Color,
    pub rule: Color,
    /// The shading behind a code block / a code line / inline code, when the
    /// view overrides the visuals' fixed greys.
    pub code_background: Option<Color>,
    pub background: Option<Color>,
}

impl MarkdownColors {
    /// The colours a paint reads, `visuals` filling in what the view was not
    /// given.
    pub(super) fn resolve(colors: Option<&MarkdownColors>, visuals: &Visuals) -> ResolvedColors {
        match colors {
            Some(c) => ResolvedColors {
                text: c.text,
                dim_text: c.text.with_alpha(180.0 / 255.0),
                link: c.link,
                rule: c.rule,
                code_background: Some(c.code_background),
                background: c.background,
            },
            None => ResolvedColors {
                text: visuals.text_color,
                dim_text: visuals.text_dim,
                link: visuals.accent,
                rule: visuals.separator,
                code_background: None,
                background: None,
            },
        }
    }
}

impl MarkdownView {
    /// Draws with `colors` instead of the app's visuals (`colors.rs`); a view
    /// given none draws in the visuals, as it always has.
    pub fn with_colors(mut self, colors: MarkdownColors) -> Self {
        self.colors = Some(colors);
        self
    }

    /// Sets or clears (`None`: back to the visuals) the view's colours.
    pub fn set_colors(&mut self, colors: Option<MarkdownColors>) {
        if self.colors != colors {
            self.colors = colors;
            crate::animation::request_draw();
        }
    }

    /// Draws emphasised (`*italic*`) text in `color` instead of the body text
    /// colour, so a secondary line such as a tooltip's help text reads as
    /// secondary. Links keep the link colour. A view given none draws
    /// emphasis in the text colour, as it always has.
    pub fn with_emphasis_color(mut self, color: Color) -> Self {
        self.emphasis_color = Some(color);
        self
    }

    /// The colour emphasised text draws in, if the view was given one.
    pub fn emphasis_color(&self) -> Option<Color> {
        self.emphasis_color
    }

    /// The view's own colours, if it was given any.
    pub fn colors(&self) -> Option<MarkdownColors> {
        self.colors
    }

    /// The colours this view paints with: its own where it has them, else
    /// `visuals`'.
    pub(super) fn resolved_colors(&self, visuals: &crate::theme::Visuals) -> ResolvedColors {
        MarkdownColors::resolve(self.colors.as_ref(), visuals)
    }
}
