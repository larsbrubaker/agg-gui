//! Lazily resolved per-row fonts for [`super::ComboBox`]: a font-preview list
//! (MatterCAD's `FontSelectorField`) draws each row in the face it names, but
//! parsing every face while the combo is built costs whole font files for rows
//! nobody sees. [`ComboBox::with_item_font_fn`] instead asks for a row's font
//! the first time that row paints in the open list
//! (`combo_box/popup_paint.rs`), never for rows scrolled out of view, and keeps
//! the answer so it is asked once per row.
//!
//! The eager [`ComboBox::with_item_fonts`] wins over this hook for any row it
//! covers. The closed box keeps the combo's own font, as agg-sharp's
//! `DropDownList` label does.
//!
//! Split out of `combo_box.rs` to keep that file under the 800-line limit.

use super::*;

/// The resolver plus what it answered for each row asked so far.
pub(crate) struct LazyItemFonts {
    resolve: Box<dyn Fn(usize) -> Option<Arc<Font>>>,
    /// `resolved[i]` is `None` until row `i` first paints, then the
    /// resolver's answer (which may itself be `None`: keep the combo's font).
    resolved: RefCell<Vec<Option<Option<Arc<Font>>>>>,
}

impl LazyItemFonts {
    /// The font row `idx` should switch to, the first time it is asked for;
    /// `None` when it was already asked for or the resolver had no font.
    pub(crate) fn resolve_first(&self, idx: usize) -> Option<Arc<Font>> {
        {
            let resolved = self.resolved.borrow();
            if matches!(resolved.get(idx), Some(Some(_))) {
                return None;
            }
        }
        // Not borrowed across the call: the resolver is the host's code.
        let font = (self.resolve)(idx);
        let mut resolved = self.resolved.borrow_mut();
        if resolved.len() <= idx {
            resolved.resize(idx + 1, None);
        }
        resolved[idx] = Some(font.clone());
        font
    }

    fn resolved_font(&self, idx: usize) -> Option<Arc<Font>> {
        self.resolved.borrow().get(idx).cloned().flatten().flatten()
    }

    fn was_asked(&self, idx: usize) -> bool {
        matches!(self.resolved.borrow().get(idx), Some(Some(_)))
    }
}

/// Swap row `idx`'s label to `font` (ignoring the system font override, as
/// the eager per-item fonts do) and lay it out at once, so the row that just
/// resolved paints measured for its own face in this very frame.
pub(super) fn swap_row_font(label: &mut Label, font: Arc<Font>, font_size: f64, inner_w: f64) {
    let mut swapped = Label::new(label.text_str(), font)
        .with_font_size(font_size)
        .with_ignore_system_font(true);
    let size = swapped.layout(Size::new(inner_w, ITEM_H));
    let old = label.bounds();
    swapped.set_bounds(Rect::new(old.x, old.y, size.width, size.height));
    *label = swapped;
}

impl ComboBox {
    /// Draw each row of the open list in a font of its own, asking `resolve`
    /// for row `i`'s font the first time row `i` paints in the open list.
    /// Rows never scrolled into view are never asked about, and each row is
    /// asked at most once (the answer, `None` included, is kept). `None`
    /// keeps the combo's own font for that row.
    pub fn with_item_font_fn(
        mut self,
        resolve: impl Fn(usize) -> Option<Arc<Font>> + 'static,
    ) -> Self {
        self.lazy_item_fonts = Some(Rc::new(LazyItemFonts {
            resolve: Box::new(resolve),
            resolved: RefCell::new(Vec::new()),
        }));
        self
    }

    /// The font row `idx` draws in when it differs from the combo's own: the
    /// eager [`with_item_fonts`](Self::with_item_fonts) entry, or the lazy
    /// resolver's answer once the row has painted. `None` before a lazy row
    /// has painted.
    pub fn item_font(&self, idx: usize) -> Option<Arc<Font>> {
        if let Some(font) = self.item_fonts.as_ref().and_then(|f| f.get(idx)) {
            return Some(Arc::clone(font));
        }
        self.lazy_item_fonts.as_ref()?.resolved_font(idx)
    }

    /// Whether the lazy resolver has been asked for row `idx`'s font yet.
    pub fn item_font_requested(&self, idx: usize) -> bool {
        self.lazy_item_fonts
            .as_ref()
            .is_some_and(|lazy| lazy.was_asked(idx))
    }

    /// The lazy resolver for the popup paint pass; `None` when there is none
    /// or the eager per-item fonts replace it.
    pub(super) fn lazy_fonts_for_paint(&self) -> Option<Rc<LazyItemFonts>> {
        if self.item_fonts.is_some() {
            return None;
        }
        self.lazy_item_fonts.clone()
    }
}
