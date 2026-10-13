//! Inline images in `MarkdownView` table cells, as MarkdigAgg's
//! `AggTableCell` holds `ImageLinkSimpleX` widgets in its flow.
//!
//! `parse.rs` records each image of a cell with the byte offset of the
//! cell text it sits before ([`CellImage`]); `layout.rs` sizes them
//! ([`MarkdownView::layout_cell_images`]) the way it sizes a paragraph's
//! images (a loaded image at its natural size, capped at the view's width,
//! else an alt-text placeholder), widens the column and heightens the row to
//! hold them.  Positions within a cell are measured here
//! ([`MarkdownView::cell_text_x`], [`MarkdownView::cell_image_x`]) so that
//! painting (`paint.rs`), the selectable model (`selection.rs`) and
//! introspection (`introspect.rs`) agree.  Painting one image, in a row or a
//! cell, is [`MarkdownView::paint_inline_image`].

use crate::color::Color;
use crate::draw_ctx::DrawCtx;

use super::{emphasis, image_loader, is_rect_visible_in_root, ImageState, MarkdownView};

/// Space after each image in a cell, as after an image in a row.
pub(super) const CELL_IMAGE_GAP: f64 = 4.0;

/// Padding above and below an image in a table row.
pub(super) const CELL_IMAGE_PAD_Y: f64 = 4.0;

/// An image of a table cell, as parsed.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct CellImage {
    /// Byte offset in the cell's text the image sits before.
    pub(super) at: usize,
    pub(super) url: String,
    pub(super) alt: String,
    pub(super) link: Option<String>,
}

/// Every cell's images, row by row and cell by cell (like `CellStyles`).
pub(super) type CellImages = Vec<Vec<Vec<CellImage>>>;

/// A cell image with its laid-out size.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct LaidCellImage {
    pub(super) image: CellImage,
    /// Index into the view's image cache.
    pub(super) cache_idx: usize,
    pub(super) width: f64,
    pub(super) height: f64,
}

/// Every cell's laid-out images, row by row and cell by cell.
pub(super) type LaidCellImages = Vec<Vec<Vec<LaidCellImage>>>;

/// The images of cell `(row, col)`, empty when it has none.
pub(super) fn images_of<T>(all: &[Vec<Vec<T>>], row: usize, col: usize) -> &[T] {
    all.get(row)
        .and_then(|cells| cells.get(col))
        .map_or(&[], Vec::as_slice)
}

impl MarkdownView {
    /// Size every cell image, loading it into the image cache on first use.
    pub(super) fn layout_cell_images(&mut self, images: &CellImages, max_w: f64) -> LaidCellImages {
        images
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| {
                        cell.iter()
                            .map(|image| {
                                let cache_idx = self.get_or_load_image(&image.url);
                                let (width, height) =
                                    self.inline_image_size(cache_idx, &image.alt, max_w);
                                LaidCellImage {
                                    image: image.clone(),
                                    cache_idx,
                                    width,
                                    height,
                                }
                            })
                            .collect()
                    })
                    .collect()
            })
            .collect()
    }

    /// Offset from a cell's content start to byte `offset` of its text: the
    /// text before it in its faces, plus every image sitting at or before it.
    pub(super) fn cell_text_x(
        &self,
        text: &str,
        spans: &[emphasis::CellSpan],
        images: &[LaidCellImage],
        offset: usize,
    ) -> f64 {
        let shift: f64 = images
            .iter()
            .filter(|image| image.image.at <= offset)
            .map(|image| image.width + CELL_IMAGE_GAP)
            .sum();
        self.cell_prefix_width(text, spans, offset) + shift
    }

    /// Offset from a cell's content start to its image `index`.
    pub(super) fn cell_image_x(
        &self,
        text: &str,
        spans: &[emphasis::CellSpan],
        images: &[LaidCellImage],
        index: usize,
    ) -> f64 {
        let before: f64 = images[..index]
            .iter()
            .map(|image| image.width + CELL_IMAGE_GAP)
            .sum();
        self.cell_prefix_width(text, spans, images[index].image.at) + before
    }

    /// Width of a cell's content: its text and its images.
    pub(super) fn cell_content_width(
        &self,
        text: &str,
        spans: &[emphasis::CellSpan],
        images: &[LaidCellImage],
    ) -> f64 {
        let images_w: f64 = images.iter().map(|image| image.width).sum();
        let gaps = images.len().saturating_sub(1) as f64 * CELL_IMAGE_GAP;
        let text_w = self.cell_prefix_width(text, spans, text.len());
        // A gap separates text from an image after it only when there is text.
        let trailing = if text_w > 0.0 && !images.is_empty() {
            CELL_IMAGE_GAP
        } else {
            0.0
        };
        text_w + images_w + gaps + trailing
    }

    /// The pieces of a cell's text between its images: byte ranges split at
    /// every image offset, empty pieces left out.
    pub(super) fn cell_text_pieces(
        text: &str,
        images: &[LaidCellImage],
    ) -> Vec<std::ops::Range<usize>> {
        let mut cuts: Vec<usize> = images.iter().map(|image| image.image.at).collect();
        cuts.push(text.len());
        let mut pieces = Vec::new();
        let mut start = 0;
        for cut in cuts {
            let cut = cut.min(text.len());
            if cut > start {
                pieces.push(start..cut);
                start = cut;
            }
        }
        pieces
    }

    /// Draw image `cache_idx` in the rect at `(x, y)` (bottom-left, Y up):
    /// the decoded pixels when ready, else a shaded box with the alt text
    /// (after the placeholder icon, when the view has one).
    /// Starts a remote load through the view's remote image loader the first
    /// time the rect is on screen.  A
    /// placeholder leaves the font size at `restore_font_size` and the fill
    /// at the text colour.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint_inline_image(
        &self,
        ctx: &mut dyn DrawCtx,
        cache_idx: usize,
        alt: &str,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        restore_font_size: f64,
    ) {
        let Some(entry) = self.image_cache.get(cache_idx) else {
            return;
        };
        let visible = is_rect_visible_in_root(ctx, x, y, width, height);
        if visible {
            let pending = entry
                .state
                .lock()
                .is_ok_and(|state| matches!(*state, ImageState::RemotePending));
            if pending {
                // With no loader (the view's or the process default) this
                // makes no request and the image stays pending: a loader
                // installed later loads it on a later paint.
                image_loader::load_remote_image(
                    self.remote_image_loader.clone(),
                    entry.load_url.clone(),
                    std::sync::Arc::clone(&entry.state),
                );
            }
        }
        let ready = entry
            .state
            .lock()
            .ok()
            .and_then(|mut state| match &mut *state {
                ImageState::Ready { image, seen } => {
                    *seen = true;
                    Some(image.clone())
                }
                _ => None,
            });
        let colors = self.resolved_colors(&ctx.visuals());
        if let Some(image) = ready {
            // An animated image draws the frame its playback clock is on
            // (`animated_image.rs`), starting the clock on this first paint.
            let (data, iw, ih) = match &image.sequence {
                Some(sequence) => {
                    let index = entry.animation.frame_for_paint(sequence, visible);
                    let frame = &sequence.frames()[index];
                    (
                        std::sync::Arc::clone(&frame.data),
                        frame.width,
                        frame.height,
                    )
                }
                None => (image.data, image.width, image.height),
            };
            ctx.draw_image_rgba(data.as_slice(), iw, ih, x, y, width, height);
        } else {
            ctx.set_fill_color(Color::rgba(0.5, 0.5, 0.5, 0.15));
            ctx.begin_path();
            ctx.rounded_rect(x, y, width, height, 3.0);
            ctx.fill();
            let icon_room = self.paint_placeholder_icon(ctx, x, y, height, colors.text);
            ctx.set_fill_color(colors.dim_text);
            ctx.set_font_size(self.font_size * 0.85);
            let label = if alt.is_empty() { "image" } else { alt };
            ctx.fill_text(label, x + 8.0 + icon_room, y + height * 0.5);
            ctx.set_font_size(restore_font_size);
            ctx.set_fill_color(colors.text);
        }
    }
}
