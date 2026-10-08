//! `ScreenRectangle` and `SearchRegion` — the port of agg-sharp
//! `GuiAutomation/SearchRegion.cs`.
//!
//! A search region is an area of the window, in Y-down window pixels (upper
//! left is 0,0), that name and image searches are limited to.  Its image is
//! captured lazily: the first image search that needs it takes the current
//! screen through the capture function the runner supplies.

use std::cell::OnceCell;

use agg_gui::Framebuffer;

/// A classic screen rect: upper left is (0, 0), lower right is (width, height).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScreenRectangle {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl ScreenRectangle {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// The overlap of `rect_a` and `rect_b`, or `None` when they do not
    /// overlap (C# returns `false` with the clipped rect in `result`).
    pub fn intersection(rect_a: ScreenRectangle, rect_b: ScreenRectangle) -> Option<Self> {
        let mut result = rect_a;
        if result.left < rect_b.left {
            result.left = rect_b.left;
        }
        if result.top < rect_b.top {
            result.top = rect_b.top;
        }
        if result.right > rect_b.right {
            result.right = rect_b.right;
        }
        if result.bottom > rect_b.bottom {
            result.bottom = rect_b.bottom;
        }

        if result.left < result.right && result.top < result.bottom {
            Some(result)
        } else {
            None
        }
    }
}

/// An area of the window that searches are limited to, with the screen
/// image they search (captured on first use).
pub struct SearchRegion {
    pub screen_rect: ScreenRectangle,
    image_contents: OnceCell<Framebuffer>,
}

impl SearchRegion {
    /// A region whose image is captured the first time a search needs it.
    pub fn new(screen_rect: ScreenRectangle) -> Self {
        Self {
            screen_rect,
            image_contents: OnceCell::new(),
        }
    }

    /// A region over an image that has already been captured.
    pub fn with_image(image_contents: Framebuffer, screen_bounds: ScreenRectangle) -> Self {
        Self {
            screen_rect: screen_bounds,
            image_contents: OnceCell::from(image_contents),
        }
    }

    /// The region's image, taking it with `capture_current_screen` (the
    /// runner's `get_current_screen`) the first time it is asked for.
    pub fn image(&self, capture_current_screen: impl FnOnce() -> Framebuffer) -> &Framebuffer {
        self.image_contents.get_or_init(capture_current_screen)
    }

    /// The image if it has been captured (or was given) already.
    pub fn captured_image(&self) -> Option<&Framebuffer> {
        self.image_contents.get()
    }
}
