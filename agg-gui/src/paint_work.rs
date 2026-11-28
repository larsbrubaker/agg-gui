//! Per-thread counts of the raster work painting does.
//!
//! Painting is mostly blitting cached pixels; the expensive part is filling
//! those caches. Two of them are counted here:
//!
//! - a widget's CPU backbuffer ([`crate::widget::BackbufferCache`]), rastered
//!   by `paint_subtree_backbuffered` in `widget/paint/offscreen.rs`;
//! - the text mask cache in `lcd_coverage/mask.rs`, which rasters a run of
//!   text into a coverage mask the first time it is drawn with a given font,
//!   size and typography style.
//!
//! Other raster work is not counted, so a difference of zero means "no widget
//! backbuffer and no text mask was rastered", not "nothing was rastered":
//! icon rasters and icon LCD composites (`icon_image.rs`), the private pixel
//! caches some widgets keep for themselves (the colour wheel picker's),
//! retained GL / FBO layer redraws, and text drawn by filling its outlines
//! rather than through the mask cache all go uncounted.
//!
//! [`paint_work`] returns how many times each counted cache has rastered on
//! the calling thread. A test or diagnostic reads it before and after a frame
//! (see [`PaintWork::since`]) to prove the frame did no first-time raster work
//! in those caches, for example that a widget warmed up ahead of time paints
//! its backbuffers and text from its caches.
//!
//! The counts are thread-local because the caches they describe are per UI
//! thread (and tests run on parallel threads). They only ever grow; nothing
//! resets them.

use std::cell::Cell;

thread_local! {
    static BACKBUFFER_RASTERS: Cell<u64> = const { Cell::new(0) };
    static TEXT_MASK_RASTERS: Cell<u64> = const { Cell::new(0) };
}

/// The raster work painting has done on one thread, as running totals.
///
/// Compare two readings with [`PaintWork::since`] to get the work done between
/// them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaintWork {
    /// Times a widget's CPU backbuffer was rastered: its first raster, and
    /// every re-raster after its cache went stale (dirty, resized, mode,
    /// theme, typography or async-state change). A band widget's dirty-strip
    /// update counts as one raster too. A paint that only blits the cached
    /// bitmap counts nothing; retained GL layers are not counted.
    pub backbuffer_rasters: u64,
    /// Times a text mask was rastered: each miss of the text mask cache
    /// (LCD or grayscale). A draw served from the cache counts nothing.
    pub text_mask_rasters: u64,
}

impl PaintWork {
    /// The work done between `earlier` and `self`, field by field. `earlier`
    /// must be an earlier reading on the same thread (the totals only grow);
    /// a field that went down saturates at zero.
    pub fn since(self, earlier: PaintWork) -> PaintWork {
        PaintWork {
            backbuffer_rasters: self
                .backbuffer_rasters
                .saturating_sub(earlier.backbuffer_rasters),
            text_mask_rasters: self
                .text_mask_rasters
                .saturating_sub(earlier.text_mask_rasters),
        }
    }
}

/// The calling thread's raster totals since the thread started.
pub fn paint_work() -> PaintWork {
    PaintWork {
        backbuffer_rasters: BACKBUFFER_RASTERS.with(Cell::get),
        text_mask_rasters: TEXT_MASK_RASTERS.with(Cell::get),
    }
}

/// Counts one widget backbuffer raster on this thread.
pub(crate) fn record_backbuffer_raster() {
    BACKBUFFER_RASTERS.with(|c| c.set(c.get() + 1));
}

/// Counts one text mask raster (a cache miss) on this thread.
pub(crate) fn record_text_mask_raster() {
    TEXT_MASK_RASTERS.with(|c| c.set(c.get() + 1));
}
