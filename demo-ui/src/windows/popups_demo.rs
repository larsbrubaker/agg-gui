//! Popups demo — a configurator over the framework's `Popup` API.
//!
//! Reconciled with the Menus demo (`menu_demo.rs`): this window is the tour of
//! the *placement/behavior* half of popups — [`RectAlign`] anchor pairs, the
//! pixel gap, and [`PopupCloseBehavior`] — while the Menus demo tours the
//! *menu system* (nested submenus, checkmarks, radios, shortcuts). The popup
//! CONTENT here is intentionally simple; a label points at the Menus demo for
//! the full menu tour, and a right-click on the trigger opens a small
//! `PopupMenu` (reusing the menu system's nesting) to show the connection.
//!
//! Mirrors egui's `popups.rs`: a button whose popup opens as a menu on
//! left-click / a context menu on right-click, two combo boxes + preset buttons
//! for the parent/child align pair, a gap `DragValue`, a close-behavior selector
//! with an explanatory tooltip, a `popup_open` checkbox, and the "Tooltips are
//! popups, too!" note.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    font_settings, Align2, Button, Color, ComboBox, DragValue, DrawCtx, Event, EventResult,
    FlexColumn, FlexRow, Font, Label, MenuEntry, MenuItem, MenuResponse, MouseButton, Point, Popup,
    PopupCloseBehavior, PopupMenu, Rect, RectAlign, ScrollView, Separator, SizedBox, Size, Tooltip,
    Widget,
};

const POPUP_W: f64 = 234.0;
const POPUP_H: f64 = 112.0;
const TRIGGER_W: f64 = 210.0;
const TRIGGER_H: f64 = 32.0;

/// Build the Popups demo window content.
pub fn popups_demo(font: Arc<Font>) -> Box<dyn Widget> {
    Box::new(PopupsDemo::new(font))
}

/// Shared configuration cells, written by the config widgets and read by the
/// demo each frame to drive the `Popup` controller.
#[derive(Clone)]
struct Config {
    /// Index into [`Align2::ALL`] for the parent (anchor) alignment.
    parent_idx: Rc<Cell<usize>>,
    /// Index into [`Align2::ALL`] for the child (popup) alignment.
    child_idx: Rc<Cell<usize>>,
    /// Pixel gap between anchor and popup.
    gap: Rc<Cell<f64>>,
    /// Index into [`PopupCloseBehavior::ALL`].
    behavior_idx: Rc<Cell<usize>>,
    /// Mirror of the popup's open flag (bound to the checkbox).
    open_flag: Rc<Cell<bool>>,
    /// One-shot open/close request produced by the checkbox's `on_change`.
    pending_open: Rc<Cell<Option<bool>>>,
}

impl Config {
    fn new() -> Self {
        Self {
            parent_idx: Rc::new(Cell::new(RectAlign::BOTTOM_START.parent.all_index())),
            child_idx: Rc::new(Cell::new(RectAlign::BOTTOM_START.child.all_index())),
            gap: Rc::new(Cell::new(4.0)),
            behavior_idx: Rc::new(Cell::new(
                PopupCloseBehavior::CloseOnClickOutside.all_index(),
            )),
            open_flag: Rc::new(Cell::new(false)),
            pending_open: Rc::new(Cell::new(None)),
        }
    }

    fn align(&self) -> RectAlign {
        RectAlign {
            parent: Align2::ALL[self.parent_idx.get().min(8)].0,
            child: Align2::ALL[self.child_idx.get().min(8)].0,
        }
    }

    fn behavior(&self) -> PopupCloseBehavior {
        PopupCloseBehavior::ALL[self.behavior_idx.get().min(2)].0
    }
}

struct PopupsDemo {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    cfg: Config,
    popup: Popup,
    context_menu: PopupMenu,
    /// Trigger button rect in this widget's local (Y-up) coordinates.
    trigger: Rect,
    /// Last fired context-menu action, shown under the trigger as feedback.
    last_action: Rc<RefCell<Option<String>>>,
}

impl PopupsDemo {
    fn new(font: Arc<Font>) -> Self {
        let cfg = Config::new();
        let config_panel = build_config_panel(Arc::clone(&font), &cfg);
        let mut popup = Popup::new();
        popup.set_size(Size::new(POPUP_W, POPUP_H));
        Self {
            bounds: Rect::default(),
            children: vec![config_panel],
            font,
            cfg,
            popup,
            context_menu: PopupMenu::new(context_items()),
            trigger: Rect::default(),
            last_action: Rc::new(RefCell::new(None)),
        }
    }

    fn active_font(&self) -> Arc<Font> {
        font_settings::current_system_font().unwrap_or_else(|| Arc::clone(&self.font))
    }

    /// Pull the live configuration into the `Popup` controller and service any
    /// pending open/close request from the checkbox.
    fn sync_popup(&mut self) {
        self.popup.align = self.cfg.align();
        self.popup.gap = self.cfg.gap.get();
        self.popup.close_behavior = self.cfg.behavior();
        self.popup.set_anchor(self.trigger);
        self.popup.set_size(Size::new(POPUP_W, POPUP_H));
        if let Some(req) = self.cfg.pending_open.take() {
            if req {
                self.context_menu.close();
                self.popup.open();
            } else {
                self.popup.close();
            }
        }
        // Mirror the authoritative open state back to the checkbox.
        self.cfg.open_flag.set(self.popup.is_open());
    }

    fn open_context_menu(&mut self, pos: Point) {
        self.popup.close();
        self.cfg.open_flag.set(false);
        self.context_menu = PopupMenu::new(context_items());
        self.context_menu.open_at(pos);
        agg_gui::animation::request_draw();
    }

    fn contains(rect: Rect, pos: Point) -> bool {
        pos.x >= rect.x
            && pos.x <= rect.x + rect.width
            && pos.y >= rect.y
            && pos.y <= rect.y + rect.height
    }
}

impl Widget for PopupsDemo {
    fn type_name(&self) -> &'static str {
        "PopupsDemo"
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
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        // Reserve a band at the bottom (low Y) for the trigger button and
        // feedback; the config panel fills the top (high Y).
        let band = (available.height * 0.42).clamp(150.0, (available.height - 60.0).max(150.0));
        let config_h = (available.height - band).max(0.0);
        if let Some(panel) = self.children.get_mut(0) {
            panel.layout(Size::new(available.width, config_h));
            panel.set_bounds(Rect::new(0.0, band, available.width, config_h));
        }
        // Center the trigger in the band, biased upward so a downward popup
        // still has room before it clamps.
        let tx = ((available.width - TRIGGER_W) * 0.5).max(0.0);
        let ty = (band * 0.5).clamp(TRIGGER_H + 26.0, band - TRIGGER_H - 8.0);
        self.trigger = Rect::new(tx, ty, TRIGGER_W, TRIGGER_H);
        // Service the checkbox's open/close request every frame — the checkbox
        // consumes its own click, so `on_event` may not run on the toggle.
        self.sync_popup();
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        ctx.set_font(self.active_font());
        ctx.set_font_size(13.0);

        ctx.set_fill_color(v.window_fill);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();

        // Trigger button chrome.
        let t = self.trigger;
        let open = self.popup.is_open() || self.context_menu.is_open();
        ctx.set_fill_color(if open { v.accent } else { v.widget_bg });
        ctx.begin_path();
        ctx.rounded_rect(t.x, t.y, t.width, t.height, 5.0);
        ctx.fill();
        ctx.set_stroke_color(if open { v.accent } else { v.widget_stroke });
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(t.x, t.y, t.width, t.height, 5.0);
        ctx.stroke();

        ctx.set_fill_color(if open { Color::white() } else { v.text_color });
        // FA comment icon + label; drawn roughly centered in the button.
        ctx.fill_text("\u{F075}  Left / right-click me", t.x + 14.0, t.y + t.height * 0.5 - 5.0);

        // Hints under the trigger.
        ctx.set_font_size(11.0);
        ctx.set_fill_color(v.text_dim);
        ctx.fill_text(
            "Left-click: popup (menu)   Right-click: context menu",
            t.x,
            t.y - 16.0,
        );
        if let Some(action) = self.last_action.borrow().as_ref() {
            ctx.set_fill_color(v.accent);
            ctx.fill_text(&format!("Context action: {action}"), t.x, t.y - 32.0);
        }
    }

    fn hit_test_global_overlay(&self, _local_pos: Point) -> bool {
        self.popup.is_open() || self.context_menu.is_open()
    }

    fn has_active_modal(&self) -> bool {
        self.popup.is_open() || self.context_menu.is_open()
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        self.sync_popup();
        let viewport = agg_gui::widget::current_viewport();

        // Context menu (right-click) takes priority while open — reuse the
        // menu system's full event handling.
        if self.context_menu.is_open() {
            let (result, response) = self.context_menu.handle_event(event, viewport);
            match response {
                MenuResponse::Action(action) => {
                    *self.last_action.borrow_mut() = Some(action);
                    agg_gui::animation::request_draw();
                }
                MenuResponse::Closed | MenuResponse::None => {}
            }
            if result == EventResult::Consumed {
                return result;
            }
        }

        match event {
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                // Apply the configured close behavior first.
                let outcome = self.popup.on_mouse_down(*pos, viewport);
                if outcome.closed {
                    self.cfg.open_flag.set(false);
                }
                if outcome.consumed {
                    agg_gui::animation::request_draw();
                    return EventResult::Consumed;
                }
                // Not consumed by the popup — a click on the trigger toggles it.
                if Self::contains(self.trigger, *pos) {
                    let now_open = self.popup.toggle();
                    self.cfg.open_flag.set(now_open);
                    agg_gui::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                pos,
                button: MouseButton::Right,
                ..
            } if Self::contains(self.trigger, *pos) => {
                self.open_context_menu(*pos);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        self.sync_popup();
        let viewport = agg_gui::widget::current_viewport();
        if self.popup.is_open() {
            let rect = self.popup.rect(viewport);
            self.paint_popup(ctx, rect);
        }
        self.context_menu
            .paint(ctx, self.active_font(), 14.0, viewport);
    }
}

impl PopupsDemo {
    /// Paint the simple popup surface at `rect` (local Y-up coordinates).
    fn paint_popup(&mut self, ctx: &mut dyn DrawCtx, rect: Rect) {
        ctx.save();
        ctx.reset_clip();
        let v = ctx.visuals();
        ctx.set_font(self.active_font());

        // Drop shadow, panel, border.
        ctx.set_fill_color(Color::black().with_alpha(0.22));
        ctx.begin_path();
        ctx.rounded_rect(rect.x + 4.0, rect.y - 4.0, rect.width, rect.height, 6.0);
        ctx.fill();
        ctx.set_fill_color(v.panel_fill);
        ctx.begin_path();
        ctx.rounded_rect(rect.x, rect.y, rect.width, rect.height, 6.0);
        ctx.fill();
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(rect.x, rect.y, rect.width, rect.height, 6.0);
        ctx.stroke();

        // Text lines, stacked from the top of the panel downward (Y-up).
        let align = self.cfg.align();
        let align_desc = align
            .preset_label()
            .map(|l| l.to_string())
            .unwrap_or_else(|| {
                format!(
                    "{} / {}",
                    Align2::ALL[align.parent.all_index()].1,
                    Align2::ALL[align.child.all_index()].1
                )
            });
        let behavior = PopupCloseBehavior::ALL[self.cfg.behavior_idx.get().min(2)].1;
        let lines: [(String, f64, Color); 4] = [
            ("\u{F075}  Popup".to_string(), 13.0, v.text_color),
            (format!("Align: {align_desc}"), 12.0, v.text_dim),
            (format!("Gap: {:.0} px", self.cfg.gap.get()), 12.0, v.text_dim),
            (format!("Close: {behavior}"), 12.0, v.text_dim),
        ];
        let mut y = rect.y + rect.height - 22.0;
        for (text, size, color) in &lines {
            ctx.set_font_size(*size);
            ctx.set_fill_color(*color);
            ctx.fill_text(text, rect.x + 12.0, y);
            y -= 22.0;
        }
        ctx.restore();
    }
}

/// The compact right-click context menu — reuses the menu system's nesting to
/// show the connection to the Menus demo.
fn context_items() -> Vec<MenuEntry> {
    vec![
        MenuItem::action("Cut", "cut")
            .icon('\u{F0C4}')
            .shortcut("Ctrl+X")
            .into(),
        MenuItem::action("Copy", "copy")
            .icon('\u{F0C5}')
            .shortcut("Ctrl+C")
            .into(),
        MenuEntry::Separator,
        MenuItem::submenu(
            "More",
            vec![
                MenuItem::action("Nested action", "nested").into(),
                MenuItem::action("Another one", "nested-2").into(),
            ],
        )
        .icon('\u{F0DA}')
        .into(),
    ]
}

/// Build the scrollable configuration panel (all the config widgets live here,
/// so the modal-extension routing keeps them usable while a popup is open).
fn build_config_panel(font: Arc<Font>, cfg: &Config) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(12.0)
        .with_panel_bg();

    col.push(
        Box::new(Label::new("Popup placement & behavior", Arc::clone(&font)).with_font_size(14.0)),
        0.0,
    );

    let anchor_labels: Vec<&str> = Align2::ALL.iter().map(|(_, l)| *l).collect();
    col.push(
        combo_row(
            "Parent anchor",
            &anchor_labels,
            Rc::clone(&cfg.parent_idx),
            Arc::clone(&font),
        ),
        0.0,
    );
    col.push(
        combo_row(
            "Popup anchor",
            &anchor_labels,
            Rc::clone(&cfg.child_idx),
            Arc::clone(&font),
        ),
        0.0,
    );

    col.push(preset_row(&font, cfg, &["Bottom", "Top", "Left", "Right"]), 0.0);
    col.push(
        preset_row(&font, cfg, &["Bottom Start", "Bottom End", "Top End"]),
        0.0,
    );

    // Gap DragValue.
    {
        let gap = Rc::clone(&cfg.gap);
        col.push(
            Box::new(
                FlexRow::new()
                    .with_gap(8.0)
                    .add(Box::new(
                        SizedBox::new().with_width(120.0).with_child(Box::new(
                            Label::new("Gap (px)", Arc::clone(&font)).with_font_size(13.0),
                        )),
                    ))
                    .add(Box::new(
                        SizedBox::new().with_width(110.0).with_height(26.0).with_child(Box::new(
                            DragValue::new(gap.get(), 0.0, 40.0, Arc::clone(&font))
                                .with_step(1.0)
                                .with_decimals(0)
                                .on_change(move |x| gap.set(x)),
                        )),
                    )),
            ),
            0.0,
        );
    }

    // Close-behavior selector, wrapped in a tooltip listing every option.
    {
        let behavior_names: Vec<&str> =
            PopupCloseBehavior::ALL.iter().map(|(_, n, _)| *n).collect();
        let tip: String = PopupCloseBehavior::ALL
            .iter()
            .map(|(_, name, desc)| format!("{name}: {desc}"))
            .collect::<Vec<_>>()
            .join("\n");
        let combo = ComboBox::new(
            behavior_names,
            cfg.behavior_idx.get(),
            Arc::clone(&font),
        )
        .with_font_size(13.0)
        .with_selected_cell(Rc::clone(&cfg.behavior_idx));
        col.push(
            Box::new(
                FlexRow::new()
                    .with_gap(8.0)
                    .add(Box::new(
                        SizedBox::new().with_width(120.0).with_child(Box::new(
                            Label::new("Close behavior", Arc::clone(&font)).with_font_size(13.0),
                        )),
                    ))
                    .add_flex(
                        Box::new(Tooltip::new(Box::new(combo), tip, Arc::clone(&font)).at_widget()),
                        1.0,
                    ),
            ),
            0.0,
        );
    }

    // popup_open checkbox — bidirectional with the Popup controller.
    {
        use agg_gui::Checkbox;
        let open_flag = Rc::clone(&cfg.open_flag);
        let pending = Rc::clone(&cfg.pending_open);
        col.push(
            Box::new(
                Checkbox::new("popup_open", Arc::clone(&font), open_flag.get())
                    .with_font_size(13.0)
                    .with_state_cell(Rc::clone(&open_flag))
                    .on_change(move |checked| pending.set(Some(checked))),
            ),
            0.0,
        );
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // "Tooltips are popups, too!" — with its own tooltip.
    col.push(
        Box::new(
            Tooltip::new(
                Box::new(
                    Label::new("Tooltips are popups, too!", Arc::clone(&font)).with_font_size(12.0),
                ),
                "Like this one!",
                Arc::clone(&font),
            )
            .at_widget(),
        ),
        0.0,
    );

    // Pointer to the Menus demo for the full menu tour.
    col.push(
        Box::new(
            Label::new(
                "For nested submenus, checkmarks, radios and shortcuts, see the Menus demo.",
                Arc::clone(&font),
            )
            .with_font_size(11.0)
            .with_wrap(true),
        ),
        0.0,
    );

    Box::new(ScrollView::new(Box::new(col)))
}

/// A `Label + ComboBox` row bound to `idx`.
fn combo_row(
    label: &str,
    options: &[&str],
    idx: Rc<Cell<usize>>,
    font: Arc<Font>,
) -> Box<dyn Widget> {
    let combo = ComboBox::new(options.to_vec(), idx.get(), Arc::clone(&font))
        .with_font_size(13.0)
        .with_selected_cell(idx);
    Box::new(
        FlexRow::new()
            .with_gap(8.0)
            .add(Box::new(
                SizedBox::new().with_width(120.0).with_child(Box::new(
                    Label::new(label, Arc::clone(&font)).with_font_size(13.0),
                )),
            ))
            .add_flex(Box::new(combo), 1.0),
    )
}

/// A row of preset buttons; clicking one writes the preset's parent/child
/// alignment indices into the shared cells (the combos pick them up on layout).
fn preset_row(font: &Arc<Font>, cfg: &Config, presets: &[&str]) -> Box<dyn Widget> {
    let mut row = FlexRow::new().with_gap(6.0);
    for name in presets {
        let Some((align, label)) = RectAlign::PRESETS.iter().find(|(_, l)| l == name) else {
            continue;
        };
        let parent_idx = Rc::clone(&cfg.parent_idx);
        let child_idx = Rc::clone(&cfg.child_idx);
        let align = *align;
        row = row.add(Box::new(
            Button::new(*label, Arc::clone(font))
                .with_font_size(12.0)
                .on_click(move || {
                    parent_idx.set(align.parent.all_index());
                    child_idx.set(align.child.all_index());
                    agg_gui::animation::request_draw();
                }),
        ));
    }
    Box::new(row)
}
