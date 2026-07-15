use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::widget::paint_subtree;
use agg_gui::{
    Button, Color, ComboBox, DrawCtx, Event, EventResult, FlexColumn, FlexRow, Font, Key, Label,
    MouseButton, Point, Rect, Size, SizedBox, TextField, Widget,
};

mod basic;
pub use basic::{undo_redo, window_options};

// ---------------------------------------------------------------------------
// Modals demo
// ---------------------------------------------------------------------------

#[derive(Default)]
struct ModalState {
    user_open: Cell<bool>,
    save_open: Cell<bool>,
    save_progress: Cell<Option<f64>>,
    name: Rc<RefCell<String>>,
    role: Rc<Cell<usize>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModalLayer {
    User,
    Save,
    Progress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModalFocus {
    Name,
}

/// Inline modal overlay: shown while any modal layer is open.
struct ModalOverlay {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    state: Rc<ModalState>,
    font: Arc<Font>,
    name_field: TextField,
    role_combo: ComboBox,
    focus: Option<ModalFocus>,
}

impl ModalOverlay {
    fn new(font: Arc<Font>, state: Rc<ModalState>) -> Self {
        let name_field = TextField::new(Arc::clone(&font))
            .with_font_size(12.0)
            .with_text_cell(Rc::clone(&state.name));
        let role_combo = ComboBox::new(vec!["user", "admin"], state.role.get(), Arc::clone(&font))
            .with_selected_cell(Rc::clone(&state.role));
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            state,
            font,
            name_field,
            role_combo,
            focus: None,
        }
    }

    fn top_layer(&self) -> Option<ModalLayer> {
        if self.state.save_progress.get().is_some() {
            Some(ModalLayer::Progress)
        } else if self.state.save_open.get() {
            Some(ModalLayer::Save)
        } else if self.state.user_open.get() {
            Some(ModalLayer::User)
        } else {
            None
        }
    }

    fn close_top(&self) {
        match self.top_layer() {
            Some(ModalLayer::Progress) => self.state.save_progress.set(None),
            Some(ModalLayer::Save) => self.state.save_open.set(false),
            Some(ModalLayer::User) => self.state.user_open.set(false),
            None => {}
        }
    }

    fn modal_rect(&self, layer: ModalLayer) -> Rect {
        let (dw, dh): (f64, f64) = match layer {
            ModalLayer::User => (250.0, 142.0),
            ModalLayer::Save => (220.0, 112.0),
            ModalLayer::Progress => (120.0, 82.0),
        };
        let viewport = agg_gui::current_viewport();
        let area_w = viewport.width.max(self.bounds.width);
        let area_h = viewport.height.max(self.bounds.height);
        let w = dw.min(area_w - 24.0).max(80.0);
        let h = dh.min(area_h - 24.0).max(64.0);
        Rect::new((area_w - w) * 0.5, (area_h - h) * 0.5, w, h)
    }

    fn button_rects(&self, layer: ModalLayer) -> Vec<(&'static str, Rect)> {
        match layer {
            ModalLayer::User => vec![
                ("Save", Rect::new(102.0, 14.0, 58.0, 24.0)),
                ("Cancel", Rect::new(166.0, 14.0, 70.0, 24.0)),
            ],
            ModalLayer::Save => vec![
                ("Yes Please", Rect::new(42.0, 14.0, 86.0, 24.0)),
                ("No Thanks", Rect::new(134.0, 14.0, 78.0, 24.0)),
            ],
            ModalLayer::Progress => Vec::new(),
        }
    }

    fn name_rect(&self, modal: Rect) -> Rect {
        Rect::new(
            54.0,
            modal.height - 64.0,
            (modal.width - 70.0).max(60.0),
            26.0,
        )
    }

    fn role_rect(&self, modal: Rect) -> Rect {
        Rect::new(
            54.0,
            modal.height - 96.0,
            (modal.width - 70.0).max(60.0),
            24.0,
        )
    }

    fn prepare_user_controls(&mut self, modal: Rect) {
        let name_rect = self.name_rect(modal);
        self.name_field
            .layout(Size::new(name_rect.width, name_rect.height));
        self.name_field
            .set_bounds(Rect::new(0.0, 0.0, name_rect.width, name_rect.height));

        let role_rect = self.role_rect(modal);
        self.role_combo
            .layout(Size::new(role_rect.width, role_rect.height));
        self.role_combo
            .set_bounds(Rect::new(0.0, 0.0, role_rect.width, role_rect.height));
    }

    fn draw_text(
        &self,
        ctx: &mut dyn DrawCtx,
        text: &str,
        x: f64,
        y: f64,
        size: f64,
        color: Color,
    ) {
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(size);
        ctx.set_fill_color(color);
        ctx.fill_text(text, x, y);
    }

    fn draw_button(&self, ctx: &mut dyn DrawCtx, rect: Rect, label: &str) {
        let mut button = SizedBox::new()
            .with_width(rect.width)
            .with_height(rect.height)
            .with_child(Box::new(
                Button::new(label, Arc::clone(&self.font)).with_font_size(11.0),
            ));
        button.layout(Size::new(rect.width, rect.height));
        button.set_bounds(Rect::new(0.0, 0.0, rect.width, rect.height));

        ctx.save();
        ctx.translate(rect.x, rect.y);
        paint_subtree(&mut button, ctx);
        ctx.restore();
    }

    fn draw_modal(&mut self, ctx: &mut dyn DrawCtx, layer: ModalLayer) {
        let v = ctx.visuals();
        let rect = self.modal_rect(layer);
        if layer == ModalLayer::User {
            self.prepare_user_controls(rect);
        }
        ctx.set_fill_color(v.window_fill);
        ctx.begin_path();
        ctx.rounded_rect(rect.x, rect.y, rect.width, rect.height, 8.0);
        ctx.fill();
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(rect.x, rect.y, rect.width, rect.height, 8.0);
        ctx.stroke();

        ctx.save();
        ctx.translate(rect.x, rect.y);
        match layer {
            ModalLayer::User => {
                self.draw_text(
                    ctx,
                    "Edit User",
                    12.0,
                    rect.height - 24.0,
                    14.0,
                    v.text_color,
                );
                self.draw_text(ctx, "Name:", 12.0, rect.height - 52.0, 11.5, v.text_dim);
                let name_rect = self.name_rect(rect);
                ctx.save();
                ctx.translate(name_rect.x, name_rect.y);
                paint_subtree(&mut self.name_field, ctx);
                ctx.restore();

                self.draw_text(ctx, "Role:", 12.0, rect.height - 76.0, 11.5, v.text_dim);
                let role_rect = self.role_rect(rect);
                ctx.save();
                ctx.translate(role_rect.x, role_rect.y);
                paint_subtree(&mut self.role_combo, ctx);
                ctx.restore();
            }
            ModalLayer::Save => {
                self.draw_text(
                    ctx,
                    "Save? Are you sure?",
                    12.0,
                    rect.height - 26.0,
                    13.0,
                    v.text_color,
                );
                self.draw_text(
                    ctx,
                    "This opens a progress modal.",
                    12.0,
                    rect.height - 54.0,
                    11.0,
                    v.text_dim,
                );
            }
            ModalLayer::Progress => {
                self.draw_text(
                    ctx,
                    "Saving...",
                    12.0,
                    rect.height - 24.0,
                    13.0,
                    v.text_color,
                );
                let progress = self
                    .state
                    .save_progress
                    .get()
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0);
                let bar = Rect::new(12.0, 20.0, rect.width - 24.0, 14.0);
                ctx.set_fill_color(v.track_bg);
                ctx.begin_path();
                ctx.rounded_rect(bar.x, bar.y, bar.width, bar.height, 7.0);
                ctx.fill();
                ctx.set_fill_color(v.accent);
                ctx.begin_path();
                ctx.rounded_rect(bar.x, bar.y, bar.width * progress, bar.height, 7.0);
                ctx.fill();
            }
        }

        for (label, rect) in self.button_rects(layer) {
            self.draw_button(ctx, rect, label);
        }
        ctx.restore();
    }

    fn point_in_rect(p: Point, r: Rect) -> bool {
        p.x >= r.x && p.x <= r.x + r.width && p.y >= r.y && p.y <= r.y + r.height
    }
}

impl Widget for ModalOverlay {
    fn type_name(&self) -> &'static str {
        "ModalOverlay"
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

    fn layout(&mut self, available: Size) -> Size {
        if self.top_layer().is_none() {
            self.bounds = Rect::new(0.0, 0.0, 0.0, 0.0);
            return Size::new(0.0, 0.0);
        }
        let h = 180.0_f64.min(available.height.max(180.0));
        let w = available.width;
        self.bounds = Rect::new(0.0, 0.0, w, h);
        Size::new(w, h)
    }

    fn paint(&mut self, _: &mut dyn DrawCtx) {}

    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        let Some(_) = self.top_layer() else { return };
        let viewport = agg_gui::current_viewport();
        let w = viewport.width.max(self.bounds.width);
        let h = viewport.height.max(self.bounds.height);

        ctx.save();
        ctx.reset_clip();
        ctx.reset_transform();
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.35));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        if self.state.user_open.get() {
            self.draw_modal(ctx, ModalLayer::User);
        }
        if self.state.save_open.get() {
            self.draw_modal(ctx, ModalLayer::Save);
        }
        if let Some(progress) = self.state.save_progress.get() {
            self.draw_modal(ctx, ModalLayer::Progress);
            if progress >= 1.0 {
                self.state.save_progress.set(None);
                self.state.save_open.set(false);
                self.state.user_open.set(false);
            } else {
                self.state
                    .save_progress
                    .set(Some((progress + 0.025).min(1.0)));
                agg_gui::animation::request_draw();
            }
        }
        ctx.restore();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        let Some(layer) = self.top_layer() else {
            return EventResult::Ignored;
        };
        match event {
            Event::KeyDown {
                key: Key::Escape, ..
            } => {
                // If the role dropdown is open, Escape closes the dropdown
                // first and must NOT close the modal — mirrors egui's
                // `clicking_escape_when_popup_open_should_not_close_modal`.
                if layer == ModalLayer::User
                    && self.role_combo.is_open()
                    && self.role_combo.on_event(event) == EventResult::Consumed
                {
                    agg_gui::animation::request_draw();
                    return EventResult::Consumed;
                }
                self.close_top();
                agg_gui::animation::request_draw();
                EventResult::Consumed
            }
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                let pos = agg_gui::current_mouse_world().unwrap_or(*pos);
                let modal_rect = self.modal_rect(layer);
                if !Self::point_in_rect(pos, modal_rect) {
                    self.close_top();
                    agg_gui::animation::request_draw();
                    return EventResult::Consumed;
                }
                let local = Point::new(pos.x - modal_rect.x, pos.y - modal_rect.y);
                if layer == ModalLayer::User {
                    self.prepare_user_controls(modal_rect);
                    let name_rect = self.name_rect(modal_rect);
                    if Self::point_in_rect(local, name_rect) {
                        self.focus = Some(ModalFocus::Name);
                        self.name_field.on_event(&Event::FocusGained);
                        let field_pos = Point::new(local.x - name_rect.x, local.y - name_rect.y);
                        let result = self.name_field.on_event(&Event::MouseDown {
                            pos: field_pos,
                            button: MouseButton::Left,
                            modifiers: Default::default(),
                        });
                        agg_gui::animation::request_draw();
                        return if result == EventResult::Consumed {
                            EventResult::Consumed
                        } else {
                            EventResult::Consumed
                        };
                    }

                    let role_rect = self.role_rect(modal_rect);
                    if Self::point_in_rect(local, role_rect)
                        || (self
                            .role_combo
                            .hit_test(Point::new(local.x - role_rect.x, local.y - role_rect.y)))
                    {
                        if self.focus == Some(ModalFocus::Name) {
                            self.name_field.on_event(&Event::FocusLost);
                        }
                        self.focus = None;
                        let combo_pos = Point::new(local.x - role_rect.x, local.y - role_rect.y);
                        self.role_combo.on_event(&Event::MouseDown {
                            pos: combo_pos,
                            button: MouseButton::Left,
                            modifiers: Default::default(),
                        });
                        agg_gui::animation::request_draw();
                        return EventResult::Consumed;
                    }
                }
                for (label, rect) in self.button_rects(layer) {
                    if Self::point_in_rect(local, rect) {
                        if self.focus == Some(ModalFocus::Name) {
                            self.name_field.on_event(&Event::FocusLost);
                            self.focus = None;
                        }
                        match (layer, label) {
                            (ModalLayer::User, "Save") => self.state.save_open.set(true),
                            (ModalLayer::User, "Cancel") => self.state.user_open.set(false),
                            (ModalLayer::Save, "Yes Please") => {
                                self.state.save_progress.set(Some(0.0))
                            }
                            (ModalLayer::Save, "No Thanks") => self.state.save_open.set(false),
                            _ => {}
                        }
                        agg_gui::animation::request_draw();
                        return EventResult::Consumed;
                    }
                }
                EventResult::Consumed
            }
            Event::MouseUp {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                if layer == ModalLayer::User {
                    let pos = agg_gui::current_mouse_world().unwrap_or(*pos);
                    let modal_rect = self.modal_rect(layer);
                    let local = Point::new(pos.x - modal_rect.x, pos.y - modal_rect.y);
                    let name_rect = self.name_rect(modal_rect);
                    if Self::point_in_rect(local, name_rect) {
                        let field_pos = Point::new(local.x - name_rect.x, local.y - name_rect.y);
                        self.name_field.on_event(&Event::MouseUp {
                            pos: field_pos,
                            button: MouseButton::Left,
                            modifiers: Default::default(),
                        });
                    }
                    let role_rect = self.role_rect(modal_rect);
                    let combo_pos = Point::new(local.x - role_rect.x, local.y - role_rect.y);
                    self.role_combo.on_event(&Event::MouseUp {
                        pos: combo_pos,
                        button: MouseButton::Left,
                        modifiers: Default::default(),
                    });
                }
                EventResult::Consumed
            }
            Event::MouseMove { pos } => {
                if layer == ModalLayer::User {
                    let pos = agg_gui::current_mouse_world().unwrap_or(*pos);
                    let modal_rect = self.modal_rect(layer);
                    let local = Point::new(pos.x - modal_rect.x, pos.y - modal_rect.y);
                    let name_rect = self.name_rect(modal_rect);
                    self.name_field.on_event(&Event::MouseMove {
                        pos: Point::new(local.x - name_rect.x, local.y - name_rect.y),
                    });
                    let role_rect = self.role_rect(modal_rect);
                    self.role_combo.on_event(&Event::MouseMove {
                        pos: Point::new(local.x - role_rect.x, local.y - role_rect.y),
                    });
                }
                EventResult::Consumed
            }
            Event::MouseWheel {
                pos,
                delta_y,
                delta_x,
                modifiers,
            } => {
                if layer == ModalLayer::User {
                    let pos = agg_gui::current_mouse_world().unwrap_or(*pos);
                    let modal_rect = self.modal_rect(layer);
                    let local = Point::new(pos.x - modal_rect.x, pos.y - modal_rect.y);
                    let role_rect = self.role_rect(modal_rect);
                    self.role_combo.on_event(&Event::MouseWheel {
                        pos: Point::new(local.x - role_rect.x, local.y - role_rect.y),
                        delta_y: *delta_y,
                        delta_x: *delta_x,
                        modifiers: *modifiers,
                    });
                }
                EventResult::Consumed
            }
            Event::KeyDown { .. } | Event::KeyUp { .. } => {
                if self.focus == Some(ModalFocus::Name) {
                    return self.name_field.on_event(event);
                }
                EventResult::Consumed
            }
            _ => EventResult::Consumed,
        }
    }

    fn hit_test(&self, p: Point) -> bool {
        self.top_layer().is_some()
            && p.x >= 0.0
            && p.x <= self.bounds.width
            && p.y >= 0.0
            && p.y <= self.bounds.height
    }

    fn has_active_modal(&self) -> bool {
        self.top_layer().is_some()
    }
}

/// Build the Modals demo — a button that shows an inline modal overlay.
pub fn modals_demo(font: Arc<Font>) -> Box<dyn Widget> {
    let state = Rc::new(ModalState::default());

    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(14.0)
        .with_panel_bg();

    col.push(
        Box::new(Label::new("Modals demo", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );

    let mut row = FlexRow::new().with_gap(8.0);
    {
        let state_for_btn = Rc::clone(&state);
        row.push(
            Box::new(
                Button::new("Open User Modal", Arc::clone(&font))
                    .with_font_size(13.0)
                    .on_click(move || {
                        state_for_btn.user_open.set(true);
                    }),
            ),
            0.0,
        );
    }
    {
        let state_for_btn = Rc::clone(&state);
        row.push(
            Box::new(
                Button::new("Open Save Modal", Arc::clone(&font))
                    .with_font_size(13.0)
                    .on_click(move || {
                        state_for_btn.save_open.set(true);
                    }),
            ),
            0.0,
        );
    }
    col.push(Box::new(row), 0.0);

    col.push(
        Box::new(ModalOverlay::new(Arc::clone(&font), Rc::clone(&state))),
        0.0,
    );

    for line in [
        "Click one of the buttons to open a modal.",
        "Modals have a backdrop and prevent interaction with the rest of the UI.",
        "You can show modals on top of each other and close the topmost modal with escape or by clicking outside the modal.",
    ] {
        col.push(
            Box::new(Label::new(line, Arc::clone(&font)).with_font_size(11.0)),
            0.0,
        );
    }

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    col.push(
        crate::windows::helpers::source_link("text_demos/dialogs.rs", Arc::clone(&font)),
        0.0,
    );
    Box::new(col)
}

#[cfg(test)]
mod tests;
