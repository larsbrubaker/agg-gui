//! Where [`NodeEditor`]'s [`CanvasPalette`] comes from: agg-gui's visuals
//! (the default), a fixed palette the host set, or a host function that
//! derives one from the visuals on every layout and paint.
//!
//! The palette is resolved afresh by `paint.rs` (canvas, noodles), by
//! `rebuild_children` in `widget/mod.rs` (simplified node cards) and by
//! `hosted.rs` (hosted card chrome); each asks [`NodeEditor::resolve_palette`]
//! so a host override survives both repaint and relayout. A derived palette
//! takes part in the paint fingerprint (`fingerprint.rs`), so a change in
//! what the host's function answers repaints the cards.

use agg_gui::theme::Visuals;

use crate::draw::CanvasPalette;

use super::NodeEditor;

/// A host function building the palette from agg-gui's current visuals.
pub type PaletteFn = Box<dyn Fn(&Visuals) -> CanvasPalette>;

/// The editor's palette source.
#[derive(Default)]
pub(crate) enum PaletteSource {
    /// [`CanvasPalette::from_visuals`]: light / dark mode flows through.
    #[default]
    Visuals,
    /// The host's palette, kept until the host sets another.
    Fixed(CanvasPalette),
    /// The host's function, asked on every layout and paint.
    Derived(PaletteFn),
}

impl NodeEditor {
    /// Draw with `palette` from now on, through every repaint and relayout,
    /// in place of the palette built from agg-gui's visuals. A host whose
    /// theme changes sets the palette again (or uses
    /// [`Self::set_palette_fn`], which follows the theme by itself).
    pub fn set_palette(&mut self, palette: CanvasPalette) {
        self.palette_source = PaletteSource::Fixed(palette);
        self.palette_changed();
    }

    /// Builder form of [`Self::set_palette`].
    pub fn with_palette(mut self, palette: CanvasPalette) -> Self {
        self.set_palette(palette);
        self
    }

    /// Build the palette with `f` from agg-gui's current visuals on every
    /// layout and paint, so a theme change reaches the canvas and the cards
    /// with no further call. A host typically starts from
    /// [`CanvasPalette::from_visuals`] and replaces the fields its theme
    /// owns (MatterCAD: the card border, the card body and the canvas).
    pub fn set_palette_fn(&mut self, f: impl Fn(&Visuals) -> CanvasPalette + 'static) {
        self.palette_source = PaletteSource::Derived(Box::new(f));
        self.palette_changed();
    }

    /// Builder form of [`Self::set_palette_fn`].
    pub fn with_palette_fn(mut self, f: impl Fn(&Visuals) -> CanvasPalette + 'static) -> Self {
        self.set_palette_fn(f);
        self
    }

    /// Go back to the palette built from agg-gui's visuals.
    pub fn clear_palette(&mut self) {
        self.palette_source = PaletteSource::Visuals;
        self.palette_changed();
    }

    /// The palette the editor last painted with.
    pub fn palette(&self) -> &CanvasPalette {
        &self.palette
    }

    /// The palette for `visuals` from the current source.
    pub(super) fn resolve_palette(&self, visuals: &Visuals) -> CanvasPalette {
        match &self.palette_source {
            PaletteSource::Visuals => CanvasPalette::from_visuals(visuals),
            PaletteSource::Fixed(p) => p.clone(),
            PaletteSource::Derived(f) => f(visuals),
        }
    }

    /// Repaint the canvas and rebuild the cards in the new colours.
    fn palette_changed(&mut self) {
        self.palette = self.resolve_palette(&agg_gui::current_visuals());
        self.last_paint_fingerprint = None;
        self.backbuffer.invalidate();
        // A layout too: it rebuilds the cards' chrome in the new colours.
        agg_gui::animation::request_layout();
    }
}

/// Hash every colour of `p` (the paint fingerprint's palette part).
pub(super) fn hash_palette<H: std::hash::Hasher>(p: &CanvasPalette, h: &mut H) {
    use std::hash::Hash;
    for c in [
        p.canvas_bg,
        p.canvas_grid,
        p.node_body,
        p.node_body_selected,
        p.node_border,
        p.node_border_selected,
        p.node_title_fallback,
        p.label_text,
        p.node_error,
        p.node_warning,
    ] {
        for v in [c.r, c.g, c.b, c.a] {
            v.to_bits().hash(h);
        }
    }
}
