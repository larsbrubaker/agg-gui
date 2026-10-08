//! `TumbleCube` — the view-cube widget.
//!
//! Port of MatterCAD's `TumbleCubeControl` (`OnDraw`, `OnMouseDown`,
//! `OnMouseMove`, `OnMouseUp`), with AtomArtist's `tumble_cube/widget.rs`
//! as the agg-gui shell it was adapted from.  The camera is the host's,
//! behind [`TumbleCubeCamera`]; drawing goes through an optional
//! [`TumbleCubeGpuRenderer`] hook (agg-gui-wgpu provides one) and falls
//! back to the software ray-caster in [`super::cpu_render`].
//!
//! Interaction, as in the C#:
//! * any button down starts a camera rotate drag; moves while down
//!   continue it; up ends it;
//! * a left button released at exactly the press position is a click,
//!   which turns the view to the clicked face / edge / corner;
//! * with no button down, hovering highlights the tiles under the cursor,
//!   and leaving the cube clears them.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

use super::camera::TumbleCubeCamera;
use super::face_textures::{CubeFaces, FaceTexture, TumbleCubeStyle};
use super::math::Mat3;
use super::orient::target_rotation;
use super::view::CubeView;

/// Side of the cube widget in logical pixels (C# `100 * DeviceScale`).
pub const TUMBLE_CUBE_SIZE: f64 = 100.0;

/// Everything a renderer needs to draw one frame of the cube.
pub struct TumbleCubeFrame<'a> {
    /// Camera, rotation and widget size (logical pixels).
    pub view: CubeView,
    /// The six face textures in face-index order (`active` is what to draw).
    pub faces: &'a [FaceTexture],
    /// Bumps whenever any face's pixels change — re-upload when it moves.
    pub faces_version: u64,
}

/// A hardware renderer for the cube, installed with
/// [`TumbleCube::with_gpu_renderer`].  Lives outside agg-gui (e.g.
/// `agg_gui_wgpu::WgpuTumbleCubeRenderer`) because agg-gui cannot depend
/// on a GPU backend.
pub trait TumbleCubeGpuRenderer {
    /// Draw `frame` into the widget's `(0, 0, width, height)` rect.
    /// Return `false` when `ctx` is not a context this renderer can use;
    /// the cube then draws with the software renderer instead.
    fn paint(&mut self, ctx: &mut dyn DrawCtx, frame: &TumbleCubeFrame<'_>) -> bool;
}

/// The software render of the last frame, reused while nothing changed.
struct CpuCache {
    key: (Mat3, u64, u32, u32),
    pixels: Arc<Vec<u8>>,
}

/// The tumble (view) cube widget.
pub struct TumbleCube {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    camera: Rc<RefCell<dyn TumbleCubeCamera>>,
    faces: CubeFaces,
    style: TumbleCubeStyle,
    gpu: Option<Box<dyn TumbleCubeGpuRenderer>>,
    /// Where the current press started (C# `mouseDownPosition`); `Some`
    /// while a button is held on the cube.
    mouse_down: Option<Point>,
    /// The cursor is over the cube (C# `mouseOver`).
    mouse_over: bool,
    /// Hover overlay colour, resolved against the theme each paint.
    overlay: Color,
    cpu_cache: Option<CpuCache>,
}

impl TumbleCube {
    /// A cube driving `camera`, with the default (MatterCAD light) style.
    pub fn new(camera: Rc<RefCell<dyn TumbleCubeCamera>>) -> Self {
        let style = TumbleCubeStyle::default();
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new()
                .with_min_size(Size::new(TUMBLE_CUBE_SIZE, TUMBLE_CUBE_SIZE))
                .with_max_size(Size::new(TUMBLE_CUBE_SIZE, TUMBLE_CUBE_SIZE)),
            camera,
            faces: CubeFaces::new(style),
            style,
            gpu: None,
            mouse_down: None,
            mouse_over: false,
            overlay: crate::theme::AccentColor::Blue
                .color()
                .with_alpha(128.0 / 255.0),
            cpu_cache: None,
        }
    }

    pub fn with_style(mut self, style: TumbleCubeStyle) -> Self {
        self.style = style;
        self
    }

    /// Draw with `renderer` when it accepts the context.
    pub fn with_gpu_renderer(mut self, renderer: Box<dyn TumbleCubeGpuRenderer>) -> Self {
        self.gpu = Some(renderer);
        self
    }

    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }

    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }

    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }

    /// The face textures (for tests and custom renderers).
    pub fn faces(&self) -> &CubeFaces {
        &self.faces
    }

    /// The cube camera for the current size and main-view rotation.
    pub fn view(&self) -> CubeView {
        let rot = self.camera.borrow().view_rotation();
        CubeView::new(&rot, self.bounds.width, self.bounds.height)
    }

    fn inside(&self, p: Point) -> bool {
        p.x >= 0.0 && p.y >= 0.0 && p.x <= self.bounds.width && p.y <= self.bounds.height
    }

    fn on_mouse_down(&mut self, pos: Point) -> EventResult {
        // Store the press so the release can tell a click from a drag.
        self.mouse_down = Some(pos);
        self.camera.borrow_mut().begin_rotate(pos);
        EventResult::Consumed
    }

    fn on_mouse_move(&mut self, pos: Point) -> EventResult {
        self.mouse_over = false;
        if self.mouse_down.is_some() {
            self.camera.borrow_mut().rotate(pos);
            crate::animation::request_draw();
            return EventResult::Consumed;
        }
        let hit = if self.inside(pos) {
            self.view().hit(pos.x, pos.y)
        } else {
            None
        };
        let changed = match hit {
            Some(hit) => {
                self.mouse_over = true;
                self.faces.highlight(hit, self.overlay)
            }
            // Off the cube (or off the widget — agg-gui's MouseLeave).
            None => self.faces.reset(),
        };
        if changed {
            crate::animation::request_draw();
        }
        // Hover never claims the move: the viewport below may want it.
        EventResult::Ignored
    }

    fn on_mouse_up(&mut self, pos: Point, button: MouseButton) -> EventResult {
        // A release with no press on the cube belongs to whoever owns the
        // drag (e.g. the viewport the cube floats over) — let it through.
        let Some(down) = self.mouse_down.take() else {
            return EventResult::Ignored;
        };
        self.camera.borrow_mut().end_rotate();
        if button == MouseButton::Left && down == pos {
            if let Some(target) = self
                .view()
                .hit(pos.x, pos.y)
                .as_ref()
                .and_then(target_rotation)
            {
                self.camera.borrow_mut().animate_rotation(target);
            }
        }
        crate::animation::request_draw();
        EventResult::Consumed
    }

    fn paint_cpu(&mut self, ctx: &mut dyn DrawCtx, view: &CubeView) {
        let scale = crate::device_scale::device_scale().max(1.0);
        let pw = (view.width * scale).ceil().max(1.0) as u32;
        let ph = (view.height * scale).ceil().max(1.0) as u32;
        let key = (view.rotation, self.faces.version(), pw, ph);
        let stale = !matches!(&self.cpu_cache, Some(c) if c.key == key);
        if stale {
            let pixels = super::cpu_render::render(view, &self.faces.faces, pw, ph);
            self.cpu_cache = Some(CpuCache {
                key,
                pixels: Arc::new(pixels),
            });
        }
        if let Some(cache) = &self.cpu_cache {
            ctx.draw_image_rgba_arc(&cache.pixels, pw, ph, 0.0, 0.0, view.width, view.height);
        }
    }
}

impl Widget for TumbleCube {
    crate::widgets::widget_as_any!();
    fn type_name(&self) -> &'static str {
        "TumbleCube"
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
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn margin(&self) -> Insets {
        self.base.margin
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
        self.base.max_size
    }

    fn layout(&mut self, _available: Size) -> Size {
        // Fixed size, like the C# control.
        let size = Size::new(TUMBLE_CUBE_SIZE, TUMBLE_CUBE_SIZE);
        self.bounds = Rect::new(self.bounds.x, self.bounds.y, size.width, size.height);
        size
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if self.bounds.width <= 0.0 || self.bounds.height <= 0.0 {
            return;
        }
        self.faces.refresh(self.style);
        self.overlay = self
            .style
            .hover
            .unwrap_or_else(|| ctx.visuals().accent.with_alpha(128.0 / 255.0));
        if !self.mouse_over {
            self.faces.reset();
        }
        let view = self.view();
        if let Some(gpu) = self.gpu.as_mut() {
            let frame = TumbleCubeFrame {
                view,
                faces: &self.faces.faces,
                faces_version: self.faces.version(),
            };
            if gpu.paint(ctx, &frame) {
                return;
            }
        }
        self.paint_cpu(ctx, &view);
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown { pos, .. } => self.on_mouse_down(*pos),
            Event::MouseMove { pos } => self.on_mouse_move(*pos),
            Event::MouseUp { pos, button, .. } => self.on_mouse_up(*pos, *button),
            _ => EventResult::Ignored,
        }
    }
}

#[cfg(test)]
#[path = "widget_tests.rs"]
mod widget_tests;
