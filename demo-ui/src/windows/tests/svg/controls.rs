use super::*;

impl SvgZoomButton {
    pub(super) fn new(
        label: &'static str,
        target_zoom: Option<f64>,
        font: Arc<Font>,
        samples: Arc<Vec<SvgSampleRender>>,
        zoom: Rc<Cell<f64>>,
        v_offset: Rc<Cell<f64>>,
        v_max: Rc<Cell<f64>>,
        h_offset: Rc<Cell<f64>>,
        h_max: Rc<Cell<f64>>,
    ) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            label,
            target_zoom,
            font,
            samples,
            zoom,
            v_offset,
            v_max,
            h_offset,
            h_max,
            pressed: false,
            hovered: false,
        }
    }

    fn active(&self) -> bool {
        match self.target_zoom {
            Some(target) => is_zoom_level(self.zoom.get(), target),
            None => !is_zoom_level(self.zoom.get(), 0.5) && !is_zoom_level(self.zoom.get(), 1.0),
        }
    }

    fn contains(&self, pos: Point) -> bool {
        pos.x >= 0.0 && pos.x <= self.bounds.width && pos.y >= 0.0 && pos.y <= self.bounds.height
    }
}

impl Widget for SvgZoomButton {
    fn type_name(&self) -> &'static str {
        "SvgZoomButton"
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

    fn layout(&mut self, _: Size) -> Size {
        Size::new(if self.label == "Custom" { 58.0 } else { 48.0 }, 22.0)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let active = self.active();
        let fill = if active {
            v.accent
        } else if self.pressed {
            v.accent_pressed
        } else if self.hovered {
            v.widget_bg_hovered
        } else {
            v.widget_bg
        };
        let text_color = if active { Color::white() } else { v.text_color };

        ctx.set_fill_color(fill);
        ctx.set_stroke_color(if active { v.accent } else { v.widget_stroke });
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(
            0.5,
            0.5,
            self.bounds.width - 1.0,
            self.bounds.height - 1.0,
            6.0,
        );
        ctx.fill_and_stroke();

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(10.5);
        ctx.set_fill_color(text_color);
        let metrics = ctx.measure_text(self.label);
        let text_w = metrics
            .as_ref()
            .map(|m| m.width)
            .unwrap_or(self.label.len() as f64 * 6.0);
        let baseline_y = metrics
            .as_ref()
            .map(|m| m.centered_baseline_y(self.bounds.height))
            .unwrap_or(7.0);
        ctx.fill_text(self.label, (self.bounds.width - text_w) * 0.5, baseline_y);
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let hovered = self.contains(*pos);
                if self.hovered != hovered {
                    self.hovered = hovered;
                    agg_gui::animation::request_draw();
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } if self.contains(*pos) => {
                self.pressed = true;
                agg_gui::animation::request_draw();
                EventResult::Consumed
            }
            Event::MouseUp {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                let was_pressed = self.pressed;
                self.pressed = false;
                if was_pressed && self.contains(*pos) {
                    if let Some(target_zoom) = self.target_zoom {
                        zoom_svg_around_viewport_center(
                            &self.samples,
                            &self.zoom,
                            &self.v_offset,
                            &self.v_max,
                            &self.h_offset,
                            &self.h_max,
                            target_zoom,
                        );
                    } else {
                        agg_gui::animation::request_draw();
                    }
                    EventResult::Consumed
                } else if was_pressed {
                    agg_gui::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }
}
