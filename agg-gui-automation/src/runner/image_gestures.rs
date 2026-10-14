//! The runner's image gestures — the port of `ClickImage`, `DragImage`,
//! `DropImage` and `DragDropImage` (agg-sharp
//! `GuiAutomation/AutomationRunner.cs`), each taking its needle as pixels or
//! by file name ([`ImageNeedle`], resolved by the runner's [`ImageSource`],
//! `image_source.rs`).
//!
//! A gesture finds the needle in the search region (the whole window when
//! none is given) with the least-squares match `images.rs` uses, moves the
//! pointer there in `pointer.rs`'s stepped, paced moves, and presses
//! (`drag_image`), releases (`drop_image`) or clicks (`click_image`, with
//! C#'s press, `HoldButton`, release). It answers whether the needle was
//! found; a needle that is not there leaves the pointer alone.
//!
//! The point is C#'s: the match's lower-left corner plus `offset` (needle
//! pixels, Y-up), plus half the needle with [`ClickOrigin::Center`]. Where C#
//! is wrong this port is not:
//! - C# drops the region's left edge and adds the match row to the region's
//!   Y-down *bottom* before flipping, so even a whole-window search lands
//!   above the window. Here the match is placed in the region: x from its
//!   left edge, y up from its bottom edge.
//! - C# ignores `secondsToWait` (one look). Here a gesture waits for the
//!   needle as `wait_for_image` does, so a needle that is still being drawn
//!   is found.
//!
//! `DoubleClickImage` and `MoveToImage` throw `NotImplementedException` in
//! C# and are left out.

use agg_gui::{clock, Framebuffer, MouseButton};

use super::{AutomationRunner, ClickOrigin, DEFAULT_WIDGET_WAIT_SECONDS};
use crate::driver::UiDriver;
use crate::image_match::find_least_squares_match;
use crate::image_source::{ImageNeedle, ImageSource};
use crate::input::{MouseAction, Point2D};
use crate::search_region::SearchRegion;

/// How long an image gesture lets the UI run between looks (C# `Delay(.05)`,
/// as `WaitForImage`).
const IMAGE_POLL_SECONDS: f64 = 0.05;

/// C#'s optional parameters of `ClickImage`, `DragImage` and `DropImage`,
/// with C#'s defaults.
#[derive(Clone, Copy)]
pub struct ImageOpts<'a> {
    /// How long to wait for the needle to show up (C# `secondsToWait`).
    pub secs_to_wait: f64,
    /// Where to look; the whole window when `None`.
    pub search_region: Option<&'a SearchRegion>,
    /// Needle pixels, Y-up, from the match's lower-left corner — or, with
    /// [`ClickOrigin::Center`], from its center.
    pub offset: Point2D,
    pub origin: ClickOrigin,
    /// The button pressed (C# `mouseButtons`; `DragImage`/`DropImage` always
    /// use the left one, as here).
    pub button: MouseButton,
}

impl Default for ImageOpts<'_> {
    fn default() -> Self {
        Self {
            secs_to_wait: DEFAULT_WIDGET_WAIT_SECONDS,
            search_region: None,
            offset: Point2D::ZERO,
            origin: ClickOrigin::Center,
            button: MouseButton::Left,
        }
    }
}

/// C#'s optional parameters of `DragDropImage`: one wait and region for both
/// ends, and an offset and origin for each.
#[derive(Clone, Copy)]
pub struct ImageDragDropOpts<'a> {
    pub secs_to_wait: f64,
    pub search_region: Option<&'a SearchRegion>,
    pub offset_drag: Point2D,
    pub origin_drag: ClickOrigin,
    pub offset_drop: Point2D,
    pub origin_drop: ClickOrigin,
}

impl Default for ImageDragDropOpts<'_> {
    fn default() -> Self {
        Self {
            secs_to_wait: DEFAULT_WIDGET_WAIT_SECONDS,
            search_region: None,
            offset_drag: Point2D::ZERO,
            origin_drag: ClickOrigin::Center,
            offset_drop: Point2D::ZERO,
            origin_drop: ClickOrigin::Center,
        }
    }
}

impl<D: UiDriver> AutomationRunner<D> {
    /// Use `source` to load needles given by name (C#'s `imagesDirectory`;
    /// an application passes its own static-data loader).
    pub fn set_image_source(&mut self, source: impl ImageSource + 'static) {
        self.images = Box::new(source);
    }

    /// C# `LoadImageFromSourceFolder`: the needle's pixels, `None` when a
    /// named needle does not load.
    pub fn load_needle(&self, needle: ImageNeedle) -> Option<Framebuffer> {
        match needle {
            ImageNeedle::Name(name) => self.images.load(name),
            ImageNeedle::Buffer(image) => Some(super::images::copy_framebuffer(image)),
        }
    }

    /// C# `ClickImage`: find the needle, move onto it and click
    /// `opts.button` there. Returns whether it was found.
    pub fn click_image(&mut self, needle: ImageNeedle, opts: &ImageOpts) -> bool {
        let Some(screen_position) = self.image_target(needle, opts) else {
            return false;
        };
        self.set_mouse_cursor_position(screen_position.x, screen_position.y);
        self.send_mouse(MouseAction::down(opts.button), screen_position, 0);
        self.hold_button();
        self.send_mouse(MouseAction::up(opts.button), screen_position, 0);
        true
    }

    /// C# `DragImage`: find the needle, move onto it and press the left
    /// button there. Returns whether it was found.
    pub fn drag_image(&mut self, needle: ImageNeedle, opts: &ImageOpts) -> bool {
        self.image_button(needle, opts, MouseAction::LeftDown)
    }

    /// C# `DropImage`: find the needle, move onto it and release the left
    /// button there. Returns whether it was found.
    pub fn drop_image(&mut self, needle: ImageNeedle, opts: &ImageOpts) -> bool {
        self.image_button(needle, opts, MouseAction::LeftUp)
    }

    /// C# `DragDropImage`: [`drag_image`](Self::drag_image) on one needle,
    /// then (if it was found) [`drop_image`](Self::drop_image) on the other.
    /// Both needles must load before anything moves, as in C#.
    pub fn drag_drop_image(
        &mut self,
        drag_needle: ImageNeedle,
        drop_needle: ImageNeedle,
        opts: &ImageDragDropOpts,
    ) -> bool {
        let (Some(drag), Some(drop)) =
            (self.load_needle(drag_needle), self.load_needle(drop_needle))
        else {
            return false;
        };
        let end = |offset, origin| ImageOpts {
            secs_to_wait: opts.secs_to_wait,
            search_region: opts.search_region,
            offset,
            origin,
            button: MouseButton::Left,
        };
        self.drag_image(
            ImageNeedle::Buffer(&drag),
            &end(opts.offset_drag, opts.origin_drag),
        ) && self.drop_image(
            ImageNeedle::Buffer(&drop),
            &end(opts.offset_drop, opts.origin_drop),
        )
    }

    /// `DragImage`/`DropImage`'s shared body: move onto the needle and send
    /// `action` there.
    fn image_button(&mut self, needle: ImageNeedle, opts: &ImageOpts, action: MouseAction) -> bool {
        let Some(screen_position) = self.image_target(needle, opts) else {
            return false;
        };
        self.set_mouse_cursor_position(screen_position.x, screen_position.y);
        self.send_mouse(action, screen_position, 0);
        true
    }

    /// Where a gesture on `needle` goes, as a pointer position (physical
    /// pixels, Y-down): wait up to `opts.secs_to_wait` for the needle, then
    /// place the match in its region (module docs). `None` when the needle
    /// does not load or is not found.
    fn image_target(&mut self, needle: ImageNeedle, opts: &ImageOpts) -> Option<Point2D> {
        self.check_not_timed_out();
        let needle = self.load_needle(needle)?;
        let mut offset = opts.offset;
        if opts.origin == ClickOrigin::Center {
            // C# `imageNeedle.Width / 2`: integer division.
            offset = offset + Point2D::new(needle.width() as i32 / 2, needle.height() as i32 / 2);
        }

        let waited_from = clock::now();
        loop {
            if let Some(position) = self.find_needle(&needle, opts.search_region) {
                let (left, bottom) = position;
                return Some(Point2D::new(left + offset.x, bottom - offset.y));
            }
            if clock::since(waited_from).as_secs_f64() >= opts.secs_to_wait {
                return None;
            }
            self.delay(IMAGE_POLL_SECONDS);
        }
    }

    /// One look for `needle`: the pointer position of the match's lower-left
    /// corner, or `None`. The region's image covers its rectangle (a region
    /// without one captures it now, `images.rs`); the whole window is a
    /// fresh capture.
    fn find_needle(
        &self,
        needle: &Framebuffer,
        search_region: Option<&SearchRegion>,
    ) -> Option<(i32, i32)> {
        let match_limit = self.config.match_limit as f64;
        let (found, (x, y), rect) = match search_region {
            Some(region) => {
                let haystack = region.image(|| self.capture_region(region.screen_rect));
                let m = find_least_squares_match(haystack, needle, match_limit);
                (m.found, m.best_position, region.screen_rect)
            }
            None => {
                let region = self.get_screen_region();
                let haystack = region.image(|| self.get_current_screen());
                let m = find_least_squares_match(haystack, needle, match_limit);
                (m.found, m.best_position, region.screen_rect)
            }
        };
        found.then_some((rect.left + x, rect.bottom - y))
    }
}
