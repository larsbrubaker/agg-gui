//! `ResponsiveImageWidget` — a picture that fills its width up to the
//! picture's own size and keeps its aspect ratio, a port of agg-sharp
//! `Gui/ResponsiveImageWidget.cs` (MatterCAD's store banners and cards).
//!
//! The host owns the picture: it hands in a [`SharedRgbaImage`] and fills or
//! replaces it whenever it likes (C# loaded the picture into the widget's
//! `ImageBuffer` later and listened to `ImageChanged`); the widget reads it at
//! every layout and paint, so a picture that arrives late resizes the widget
//! at the next frame.
//!
//! Units are logical: the picture's natural size is its pixel size in logical
//! units (C#'s `MaximumSize = Image.Width * DeviceScale` device pixels), so a
//! HiDPI screen shows it scaled up rather than shrunk.

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::screenshot::SharedRgbaImage;
use crate::widget::Widget;

/// C# `ResponsiveImageWidget`.
pub struct ResponsiveImageWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    image: SharedRgbaImage,
    /// C# `maxWidthSetExplicitly`: a maximum width the host set, kept over the
    /// picture's own width.
    max_width: Option<f64>,
    /// C# `RenderCheckerboard`: a white and light-gray checkerboard (10
    /// logical units a square) behind the picture, showing through its alpha.
    pub render_checkerboard: bool,
}

impl ResponsiveImageWidget {
    /// A widget showing `image` (C# `HAnchor.Stretch`).
    pub fn new(image: SharedRgbaImage) -> Self {
        let mut base = WidgetBase::new();
        base.h_anchor = HAnchor::STRETCH;
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base,
            image,
            max_width: None,
            render_checkerboard: false,
        }
    }

    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }

    /// C# `MaximumSize.X` set by the host: the widest the picture is drawn.
    pub fn with_max_width(mut self, width: f64) -> Self {
        self.max_width = Some(width);
        self
    }

    /// The picture the widget shows.
    pub fn image(&self) -> &SharedRgbaImage {
        &self.image
    }

    fn image_size(&self) -> Option<(f64, f64)> {
        let image = self.image.borrow();
        let (_, w, h) = image.as_ref()?;
        (*w > 0).then_some((f64::from(*w), f64::from(*h)))
    }
}

impl Widget for ResponsiveImageWidget {
    crate::widgets::widget_as_any!();
    fn type_name(&self) -> &'static str {
        "ResponsiveImageWidget"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn margin(&self) -> Insets {
        self.base.margin
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn h_anchor(&self) -> HAnchor {
        self.base.h_anchor
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }
    fn min_size(&self) -> Size {
        self.base.min_size
    }
    fn max_size(&self) -> Size {
        match self.image_size() {
            Some((w, h)) => Size::new(self.max_width.unwrap_or(w), h),
            None => self.base.max_size,
        }
    }

    /// C#'s `LocalBounds` setter: the width it is given, and the picture's
    /// height scaled by how much of its width fits (never above 1).
    fn layout(&mut self, available: Size) -> Size {
        match self.image_size() {
            Some((w, h)) => {
                let max_w = self.max_width.unwrap_or(w);
                let scale = (max_w.min(available.width) / w).min(1.0);
                Size::new(available.width, h * scale)
            }
            None => Size::new(available.width, 0.0),
        }
    }

    /// C# `OnDraw`: the checkerboard if asked, then the picture centred, no
    /// wider than the widget or the picture, across the full height.
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let (width, height) = (self.bounds.width, self.bounds.height);
        if self.render_checkerboard {
            let square: f64 = 10.0;
            let (white, gray) = (Color::white(), Color::rgba(0.827, 0.827, 0.827, 1.0));
            let mut y = 0.0;
            let mut row = 0;
            while y < height {
                let mut x = 0.0;
                let mut column = 0;
                while x < width {
                    ctx.set_fill_color(if (row + column) % 2 == 0 { white } else { gray });
                    ctx.begin_path();
                    ctx.rect(x, y, square.min(width - x), square.min(height - y));
                    ctx.fill();
                    x += square;
                    column += 1;
                }
                y += square;
                row += 1;
            }
        }
        let image = self.image.borrow();
        if let Some((pixels, w, h)) = image.as_ref() {
            if *w > 0 {
                let size_x = width.min(f64::from(*w));
                ctx.draw_image_rgba(pixels, *w, *h, (width - size_x) / 2.0, 0.0, size_x, height);
            }
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}
