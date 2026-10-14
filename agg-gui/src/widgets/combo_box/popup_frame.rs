//! Where an open [`super::ComboBox`] list is placed, and the space its rows
//! are hit-tested in — ported from agg-sharp's `PopupLayoutEngine`
//! (`Gui/Menu/PopupWidget.cs`, `RecalculatePosition`) as a `DropDownList`
//! uses it.
//!
//! The list paints in root space at the window's scale, not the combo's: a
//! combo inside a zoomed parent (a node card in the zoomed node editor) opens
//! a list whose rows are as tall and whose text is as large as anywhere else.
//! The list is placed against the combo's **drawn** box (its local bounds
//! through every transform above it, read off the paint context):
//!
//! - left edges together, or right edges together when
//!   [`ComboBox::with_align_to_right_edge`] asks and the list then stays on
//!   the window, or when a left-aligned list would run off the window's right
//!   (C# `alignToRightEdge`);
//! - under the drawn bottom, or on the drawn top when it opens upward
//!   ([`ComboBox::with_opens_upward`] prefers that, C# `Direction.Up`; the
//!   room on screen can overrule either, as C# does);
//! - as wide as the drawn box, but never narrower than the combo's own width,
//!   so a list from a zoomed-out combo still fits its text. C# keeps the
//!   local width; a list matching its zoomed-in control is what a user
//!   expects to see.
//!
//! The popup frame ("F space") is root space shifted to the list's anchor
//! corner: `x` runs `0..width` across the list, `y` is `0` at the drawn box
//! edge the list hangs from. `geometry.rs` lays rows out in F space, and
//! every pointer position goes through [`ComboBox::to_popup`] before a row
//! is tested, so a press lands on the row drawn under it at any zoom.

use super::*;
use crate::TransAffine;

/// The open list's placement, from the last paint.
#[derive(Clone, Copy, Debug)]
pub(super) struct PopupFrame {
    /// The combo's local space to root logical space (unlifted).
    pub(super) local_to_root: TransAffine,
    /// F space's origin in root logical space.
    pub(super) origin: Point,
    /// The list's width.
    pub(super) width: f64,
}

impl ComboBox {
    /// Prefer opening the list above the box (C# `Direction.Up`). The room on
    /// screen still decides when the list fits only on the other side. The
    /// closed box's chevron points the way the list opens.
    pub fn with_opens_upward(mut self, up: bool) -> Self {
        self.prefer_up = up;
        self
    }

    /// Line the list's right edge up with the box's (C# `AlignToRightEdge`),
    /// unless that would push the list off the window's left.
    pub fn with_align_to_right_edge(mut self, right: bool) -> Self {
        self.align_to_right_edge = right;
        self
    }

    /// Limit the open list to `height` (C#'s `maxHeight`; `0` = no limit),
    /// rounded down to whole rows, at least one. A longer list scrolls.
    pub fn with_max_height(mut self, height: f64) -> Self {
        self.max_height = height.max(0.0);
        self
    }

    /// The open list's rectangle in root logical units as last painted, or
    /// `None` while the list is closed or not yet painted.
    pub fn open_list_rect(&self) -> Option<Rect> {
        let frame = self.open_frame()?;
        Some(Rect::new(
            frame.origin.x,
            frame.origin.y + self.popup_bottom(),
            frame.width,
            self.popup_h(),
        ))
    }

    /// Row `index`'s rectangle in root logical units as the open list lays
    /// it out — above or below [`Self::open_list_rect`] when the row is
    /// scrolled out of view. `None` while the list is closed or not yet
    /// painted, or past the last row.
    pub fn open_list_item_rect(&self, index: usize) -> Option<Rect> {
        let frame = self.open_frame()?;
        (index < self.options.len()).then(|| {
            let r = self.row_rect(index);
            Rect::new(
                frame.origin.x + r.x,
                frame.origin.y + r.y,
                r.width,
                r.height,
            )
        })
    }

    fn open_frame(&self) -> Option<PopupFrame> {
        self.popup_frame.filter(|_| self.open)
    }

    /// The list's width in F space.
    pub(super) fn popup_width(&self) -> f64 {
        self.popup_frame
            .map(|f| f.width)
            .unwrap_or(self.bounds.width)
    }

    /// A point in the combo's local space, in F space. Before the first
    /// paint of an opening the list is assumed unzoomed at the box.
    pub(super) fn to_popup(&self, p: Point) -> Point {
        match self.popup_frame {
            Some(frame) => {
                let (mut x, mut y) = (p.x, p.y);
                frame.local_to_root.transform(&mut x, &mut y);
                Point::new(x - frame.origin.x, y - frame.origin.y)
            }
            None => {
                let edge = if self.popup_opens_up {
                    self.closed_h()
                } else {
                    0.0
                };
                Point::new(p.x, p.y - edge)
            }
        }
    }

    /// An F-space rectangle in the combo's local space (automation's row
    /// targets), through the inverse of the painted transform.
    pub(super) fn popup_rect_to_local(&self, r: Rect) -> Rect {
        match self.popup_frame {
            Some(frame) => {
                let mut to_local = frame.local_to_root;
                to_local.invert();
                let root = Rect::new(
                    r.x + frame.origin.x,
                    r.y + frame.origin.y,
                    r.width,
                    r.height,
                );
                crate::widget::event_root::transform_rect(&to_local, root)
            }
            None => {
                let edge = if self.popup_opens_up {
                    self.closed_h()
                } else {
                    0.0
                };
                Rect::new(r.x, r.y + edge, r.width, r.height)
            }
        }
    }

    /// Place the open list against the drawn box (C# `RecalculatePosition`)
    /// and queue it for the popup drain.
    pub(super) fn submit_open_popup(&mut self, ctx: &mut dyn DrawCtx) {
        // The popup queue is drained while App's effective (device × UX)
        // scale and keyboard lift are still active on the ctx, so the
        // request coords must be root logical (unlifted). Otherwise on
        // HiDPI / UX-zoomed mobile the popup paints at a scale²-magnified
        // position while hit-testing (purely logical) stays on the box.
        let local_to_root = crate::widget::logical_root_transform(ctx);
        let drawn = crate::widget::event_root::transform_rect(
            &local_to_root,
            Rect::new(0.0, 0.0, self.bounds.width, self.closed_h()),
        );
        // Open up or down by the room ON SCREEN (`drawn` is unlifted).
        let vis = current_combo_viewport()
            .map(crate::widget::visible_root_rect)
            .unwrap_or(Rect::new(0.0, 0.0, f64::MAX / 4.0, f64::MAX / 4.0));
        self.configure_popup_geometry(drawn.y - vis.y, drawn.y + drawn.height - vis.y, vis.height);

        let width = self.bounds.width.max(drawn.width);
        let left = drawn.x;
        let right = drawn.x + drawn.width - width;
        let x = if (self.align_to_right_edge && right >= vis.x) || left + width > vis.x + vis.width
        {
            right
        } else {
            left
        };
        let y = if self.popup_opens_up {
            drawn.y + drawn.height
        } else {
            drawn.y
        };
        self.popup_frame = Some(PopupFrame {
            local_to_root,
            origin: Point::new(x, y),
            width,
        });

        let style = self.popup_scroll_style();
        let visibility = current_scroll_visibility();
        let viewport = self.popup_scroll_viewport();
        let geom = self.scrollbar_geometry(style);
        let scrollbar = self
            .scrollbar
            .prepare_paint(viewport, style, visibility, geom)
            .map(|bar| bar.translated(x, y));
        submit_combo_popup(ComboPopupRequest {
            x,
            y,
            width,
            popup_h: self.popup_h(),
            opens_up: self.popup_opens_up,
            first_item: self.scroll_offset,
            scroll_frac: self.scroll_frac,
            visible_count: self.popup_visible_count,
            selected: self.has_selection.then_some(self.selected),
            hovered_item: self.hovered_item,
            scrollbar,
            item_count: self.options.len(),
            item_labels: Rc::clone(&self.item_labels),
            hover_tooltip: self.hovered_item.and_then(|idx| {
                let text = self.item_tooltips.as_ref()?.get(idx)?;
                if text.is_empty() {
                    return None;
                }
                Some((text.clone(), Arc::clone(&self.font)))
            }),
            state_style: self.state_style,
            lazy_fonts: self.lazy_fonts_for_paint(),
            font_size: self.font_size,
            label_w: (self.bounds.width - PAD_X * 2.0 - ARROW_W).max(0.0),
            disabled_items: self.disabled_items.clone(),
            disabled_label_alpha: self.disabled_style.label_alpha,
        });
    }
}
