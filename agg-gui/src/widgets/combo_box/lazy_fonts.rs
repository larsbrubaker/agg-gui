//! Lazily resolved per-row fonts for [`super::ComboBox`]: a font-preview list
//! (MatterCAD's `FontSelectorField`) draws each row in the face it names, but
//! parsing every face while the combo is built costs whole font files for rows
//! nobody sees. [`ComboBox::with_item_font_fn`] instead asks for a row's font
//! the first time that row paints in the open list
//! (`combo_box/popup_paint.rs`), never for rows scrolled out of view, and keeps
//! the answer so it is asked once per row.
//!
//! A resolver's answer can change: in the browser a face's file downloads the
//! first time a row asks for it, so the first answer is a fallback (or `None`)
//! and the real face arrives later. The host then calls
//! [`ComboBox::refresh_item_fonts`], or [`ComboBoxFontRefresh::refresh`] on the
//! handle from [`ComboBox::item_font_refresh_handle`] when the combo lives in a
//! widget tree it no longer holds. A refresh bumps a generation the combo
//! shares with its handles and requests a draw; the next open-list paint sees
//! the new generation (`ComboBox::lazy_fonts_for_paint`) and marks every row
//! unasked, so each row is asked once more as it paints and its label switches
//! to the new answer, or back to the combo's own font when the answer is now
//! `None`. [`ComboBox::set_options`] instead forgets the answers outright: its
//! rows are new labels in the combo's font.
//!
//! The eager [`ComboBox::with_item_fonts`] wins over this hook for any row it
//! covers. The closed box keeps the combo's own font, as agg-sharp's
//! `DropDownList` label does.
//!
//! Split out of `combo_box.rs` to keep that file under the 800-line limit.

use super::*;

/// What one lazily resolved row knows.
#[derive(Clone, Default)]
struct RowFont {
    /// Asked since the answers were last forgotten or refreshed.
    asked: bool,
    /// The face the row's label was switched to; `None` while it shows the
    /// combo's own font.
    font: Option<Arc<Font>>,
}

/// The resolver plus what it answered for each row asked so far.
pub(crate) struct LazyItemFonts {
    resolve: Box<dyn Fn(usize) -> Option<Arc<Font>>>,
    rows: RefCell<Vec<RowFont>>,
    /// The [`ComboBoxFontRefresh`] generation the answers belong to.
    generation: Cell<u64>,
}

/// How a row's label changes after its font is asked for.
pub(super) enum RowFontChange {
    /// Draw the row in this face.
    To(Arc<Font>),
    /// The resolver no longer has a face: back to the combo's own font.
    Back,
}

impl LazyItemFonts {
    /// The change row `idx`'s label needs, asking the resolver when the row
    /// has not been asked since the last refresh; `None` when it was already
    /// asked or its label already shows the answer.
    pub(super) fn resolve(&self, idx: usize) -> Option<RowFontChange> {
        let previous = {
            let rows = self.rows.borrow();
            match rows.get(idx) {
                Some(row) if row.asked => return None,
                Some(row) => row.font.clone(),
                None => None,
            }
        };
        // Not borrowed across the call: the resolver is the host's code.
        let font = (self.resolve)(idx);
        {
            let mut rows = self.rows.borrow_mut();
            if rows.len() <= idx {
                rows.resize(idx + 1, RowFont::default());
            }
            rows[idx] = RowFont {
                asked: true,
                font: font.clone(),
            };
        }
        match (font, previous) {
            (Some(font), _) => Some(RowFontChange::To(font)),
            (None, Some(_)) => Some(RowFontChange::Back),
            (None, None) => None,
        }
    }

    /// Forget every answer: the rows now name other things
    /// ([`ComboBox::set_options`]), so each is asked again when it paints.
    pub(super) fn forget(&self) {
        self.rows.borrow_mut().clear();
    }

    /// Mark every row unasked when the host refreshed since the last paint,
    /// keeping the face each label shows so a `None` answer can switch back.
    fn sync_generation(&self, generation: u64) {
        if self.generation.get() == generation {
            return;
        }
        self.generation.set(generation);
        for row in self.rows.borrow_mut().iter_mut() {
            row.asked = false;
        }
    }

    fn resolved_font(&self, idx: usize) -> Option<Arc<Font>> {
        self.rows.borrow().get(idx).and_then(|row| row.font.clone())
    }

    fn was_asked(&self, idx: usize) -> bool {
        self.rows.borrow().get(idx).is_some_and(|row| row.asked)
    }
}

/// A handle that tells a [`ComboBox`] its lazily resolved row fonts
/// ([`ComboBox::with_item_font_fn`]) may have changed. Get it with
/// [`ComboBox::item_font_refresh_handle`] before handing the combo to a
/// widget tree; clones share the combo. UI thread only, like the combo.
#[derive(Clone, Debug, Default)]
pub struct ComboBoxFontRefresh {
    generation: Rc<Cell<u64>>,
}

impl ComboBoxFontRefresh {
    /// Ask each row of the combo's open list for its font again the next time
    /// it paints, and request a draw so an open list updates without input.
    /// Rows never painted are still not asked. Harmless after the combo is
    /// dropped or when it has no lazy resolver.
    pub fn refresh(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        crate::animation::request_draw();
    }

    fn generation(&self) -> u64 {
        self.generation.get()
    }
}

/// Apply `change` to row `idx`'s label (a face of its own ignores the system
/// font override, as the eager per-item fonts do; the combo's own font follows
/// it, as the labels it builds do) and lay it out at once, so the row paints
/// measured for its new face in this very frame.
pub(super) fn apply_row_font(
    label: &mut Label,
    change: RowFontChange,
    combo_font: &Arc<Font>,
    font_size: f64,
    inner_w: f64,
) {
    let (font, own_face) = match change {
        RowFontChange::To(font) => (font, true),
        RowFontChange::Back => (Arc::clone(combo_font), false),
    };
    let mut swapped = Label::new(label.text_str(), font)
        .with_font_size(font_size)
        .with_ignore_system_font(own_face);
    let size = swapped.layout(Size::new(inner_w, ITEM_H));
    let old = label.bounds();
    swapped.set_bounds(Rect::new(old.x, old.y, size.width, size.height));
    *label = swapped;
}

impl ComboBox {
    /// Draw each row of the open list in a font of its own, asking `resolve`
    /// for row `i`'s font the first time row `i` paints in the open list.
    /// Rows never scrolled into view are never asked about, and each row is
    /// asked once (the answer, `None` included, is kept) until
    /// [`refresh_item_fonts`](Self::refresh_item_fonts) or a
    /// [`ComboBoxFontRefresh`] says the answers may have changed; then each
    /// row is asked once more as it next paints. `None` keeps the combo's own
    /// font for that row.
    pub fn with_item_font_fn(
        mut self,
        resolve: impl Fn(usize) -> Option<Arc<Font>> + 'static,
    ) -> Self {
        self.lazy_item_fonts = Some(Rc::new(LazyItemFonts {
            resolve: Box::new(resolve),
            rows: RefCell::new(Vec::new()),
            generation: Cell::new(self.item_font_refresh.generation()),
        }));
        self
    }

    /// Say that the [`with_item_font_fn`](Self::with_item_font_fn) resolver's
    /// answers may have changed (a face finished loading): each row is asked
    /// again the next time it paints in the open list and redraws in the new
    /// answer. Requests a draw. [`item_font_refresh_handle`](Self::item_font_refresh_handle)
    /// does the same for a host that no longer holds the combo.
    pub fn refresh_item_fonts(&self) {
        self.item_font_refresh.refresh();
    }

    /// A handle whose [`ComboBoxFontRefresh::refresh`] is
    /// [`refresh_item_fonts`](Self::refresh_item_fonts) on this combo, for a
    /// host that hands the combo to a widget tree.
    pub fn item_font_refresh_handle(&self) -> ComboBoxFontRefresh {
        self.item_font_refresh.clone()
    }

    /// The font row `idx` draws in when it differs from the combo's own: the
    /// eager [`with_item_fonts`](Self::with_item_fonts) entry, or the lazy
    /// resolver's latest answer once the row has painted. `None` before a lazy
    /// row has painted.
    pub fn item_font(&self, idx: usize) -> Option<Arc<Font>> {
        if let Some(font) = self.item_fonts.as_ref().and_then(|f| f.get(idx)) {
            return Some(Arc::clone(font));
        }
        self.lazy_item_fonts.as_ref()?.resolved_font(idx)
    }

    /// Whether the lazy resolver has been asked for row `idx`'s font since
    /// the options were set or the row fonts last refreshed.
    pub fn item_font_requested(&self, idx: usize) -> bool {
        self.lazy_item_fonts.as_ref().is_some_and(|lazy| {
            lazy.generation.get() == self.item_font_refresh.generation() && lazy.was_asked(idx)
        })
    }

    /// The lazy resolver for the popup paint pass, its answers marked stale
    /// when the host refreshed since the last paint; `None` when there is
    /// none or the eager per-item fonts replace it.
    pub(super) fn lazy_fonts_for_paint(&self) -> Option<Rc<LazyItemFonts>> {
        if self.item_fonts.is_some() {
            return None;
        }
        let lazy = self.lazy_item_fonts.clone()?;
        lazy.sync_generation(self.item_font_refresh.generation());
        Some(lazy)
    }

    /// The face row `idx`'s label draws with, for tests that check what the
    /// list really paints.
    #[cfg(test)]
    pub(crate) fn item_label_font(&self, idx: usize) -> Option<Arc<Font>> {
        self.item_labels.borrow().get(idx).map(Label::active_font)
    }
}
