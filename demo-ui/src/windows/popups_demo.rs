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
//! Mirrors egui's popups.rs (egui_demo_lib): a "Click, right-click and hover
//! me!" trigger whose popup opens as a menu (left-click) / context menu
//! (right-click) and shows "Tooltips are popups, too!" on hover; a code-styled
//! configurator with two `Align2` combo boxes, a 12-entry preset combo, a gap
//! `DragValue`, a close-behavior combo with per-item hover tooltips, a
//! `popup_open` checkbox, and a reset button.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    font_settings, Align2, Button, Checkbox, Color, ComboBox, DragValue, DrawCtx, Event,
    EventResult, FlexColumn, FlexRow, Font, Key, Label, MenuEntry, MenuItem, MenuResponse,
    MouseButton, Point, Popup, PopupCloseBehavior, PopupMenu, Rebuilder, Rect, RectAlign,
    ScrollView, Separator, Size, SizedBox, Spacer, Tooltip, Widget,
};

const POPUP_W: f64 = 234.0;
const POPUP_H: f64 = 112.0;
const TRIGGER_W: f64 = 232.0;
const TRIGGER_H: f64 = 32.0;
/// Width of the code-style caption column in the configurator rows.
const CAPTION_W: f64 = 150.0;

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
    /// Preset combo selection: 0 = "<Select Preset>", i+1 = `PRESETS[i]`.
    preset_idx: Rc<Cell<usize>>,
    /// The preset index this demo last wrote/observed — lets `sync_popup`
    /// distinguish a user pick in the preset combo from its own write-back.
    last_preset: Rc<Cell<usize>>,
    /// Pixel gap between anchor and popup.
    gap: Rc<Cell<f64>>,
    /// Index into [`PopupCloseBehavior::ALL`].
    behavior_idx: Rc<Cell<usize>>,
    /// Mirror of the popup's open flag (bound to the checkbox).
    open_flag: Rc<Cell<bool>>,
    /// One-shot open/close request produced by the checkbox's `on_change`.
    pending_open: Rc<Cell<Option<bool>>>,
    /// Bumped by the reset button; the config panel's `Rebuilder` keys on it
    /// so widgets without cell bindings (the gap `DragValue`) re-read state.
    reset_epoch: Rc<Cell<u64>>,
}

impl Config {
    fn new() -> Self {
        let default_preset = preset_index_of(RectAlign::BOTTOM_START)
            .map(|i| i + 1)
            .unwrap_or(0);
        Self {
            parent_idx: Rc::new(Cell::new(RectAlign::BOTTOM_START.parent.all_index())),
            child_idx: Rc::new(Cell::new(RectAlign::BOTTOM_START.child.all_index())),
            preset_idx: Rc::new(Cell::new(default_preset)),
            last_preset: Rc::new(Cell::new(default_preset)),
            gap: Rc::new(Cell::new(4.0)),
            // CloseOnClick — egui's default and its demo's starting selection.
            behavior_idx: Rc::new(Cell::new(PopupCloseBehavior::CloseOnClick.all_index())),
            open_flag: Rc::new(Cell::new(false)),
            pending_open: Rc::new(Cell::new(None)),
            reset_epoch: Rc::new(Cell::new(0)),
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

    /// Restore every cell to the demo defaults (egui's reset "⟲" button).
    fn reset(&self) {
        self.parent_idx
            .set(RectAlign::BOTTOM_START.parent.all_index());
        self.child_idx
            .set(RectAlign::BOTTOM_START.child.all_index());
        self.gap.set(4.0);
        self.behavior_idx
            .set(PopupCloseBehavior::CloseOnClick.all_index());
        self.pending_open.set(Some(false));
        self.reset_epoch.set(self.reset_epoch.get() + 1);
        agg_gui::animation::request_draw();
    }
}

fn preset_index_of(align: RectAlign) -> Option<usize> {
    RectAlign::PRESETS.iter().position(|(a, _)| *a == align)
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
        // Hover target overlaid on the custom-painted trigger button so the
        // framework's Tooltip machinery serves the "hover me!" part —
        // matching egui, where the tooltip is ON the trigger itself.
        let trigger_tooltip: Box<dyn Widget> = Box::new(
            Tooltip::new(
                Box::new(
                    SizedBox::new()
                        .with_width(TRIGGER_W)
                        .with_height(TRIGGER_H),
                ),
                "Tooltips are popups, too!",
                Arc::clone(&font),
            )
            .at_widget(),
        );
        let mut popup = Popup::new();
        popup.set_size(Size::new(POPUP_W, POPUP_H));
        Self {
            bounds: Rect::default(),
            children: vec![config_panel, trigger_tooltip],
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

    /// Pull the live configuration into the `Popup` controller, reconcile the
    /// preset combo with the parent/child combos, and service any pending
    /// open/close request from the checkbox.
    fn sync_popup(&mut self) {
        // Preset combo → align combos: a change we did not write back
        // ourselves means the user picked an entry (0 = "<Select Preset>").
        let sel = self.cfg.preset_idx.get();
        if sel != self.cfg.last_preset.get() {
            if let Some((align, _)) = sel.checked_sub(1).and_then(|i| RectAlign::PRESETS.get(i)) {
                self.cfg.parent_idx.set(align.parent.all_index());
                self.cfg.child_idx.set(align.child.all_index());
            }
            self.cfg.last_preset.set(sel);
        }
        // Align combos → preset combo: show the matching preset name, or the
        // "<Select Preset>" placeholder for a hand-composed pair.
        let expected = preset_index_of(self.cfg.align())
            .map(|i| i + 1)
            .unwrap_or(0);
        if expected != self.cfg.preset_idx.get() {
            self.cfg.preset_idx.set(expected);
            self.cfg.last_preset.set(expected);
        }

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
        // still has room before it flips or clamps.
        let tx = ((available.width - TRIGGER_W) * 0.5).max(0.0);
        let ty = (band * 0.5).clamp(TRIGGER_H + 26.0, band - TRIGGER_H - 8.0);
        self.trigger = Rect::new(tx, ty, TRIGGER_W, TRIGGER_H);
        // The tooltip hover target tracks the trigger rect exactly.
        if let Some(tip) = self.children.get_mut(1) {
            tip.layout(Size::new(TRIGGER_W, TRIGGER_H));
            tip.set_bounds(self.trigger);
        }
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
        // Same wording as egui's trigger button.
        ctx.fill_text(
            "Click, right-click and hover me!",
            t.x + 17.0,
            t.y + t.height * 0.5 - 5.0,
        );

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
        // menu system's full event handling (including its own Escape).
        if self.context_menu.is_open() {
            let (result, response) = self.context_menu.handle_event(event, viewport);
            match response {
                MenuResponse::Action(action) => {
                    *self.last_action.borrow_mut() = Some(action);
                    agg_gui::animation::request_draw();
                }
                MenuResponse::Closed | MenuResponse::None => {}
            }
            if result.is_consumed() {
                return result;
            }
        }

        match event {
            // Escape closes the popup regardless of close behavior — an
            // IgnoreClicks popup stays keyboard-dismissable (egui parity).
            Event::KeyDown {
                key: Key::Escape, ..
            } => {
                if self.popup.on_escape() {
                    self.cfg.open_flag.set(false);
                    agg_gui::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
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
            } => {
                // Right-clicks inside the open popup are swallowed so they
                // don't fall through to whatever sits underneath.
                if self.popup.is_open() && self.popup.contains(*pos, viewport) {
                    return EventResult::Consumed;
                }
                if Self::contains(self.trigger, *pos) {
                    self.open_context_menu(*pos);
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    // Overlay coordinates: `paint_global_overlays` (widget/paint.rs) walks the
    // tree preserving each widget's local transform, and event positions
    // arrive in the same local space — so placing AND hit-testing the popup
    // in local coordinates is self-consistent. Clamping/flipping against
    // `current_viewport()` in local space follows the exact convention the
    // menu system uses (`PopupMenuState::layouts` clamps its panels the same
    // way), trading pixel-exact edge clamping inside deeply nested widgets
    // for a cheap, transform-free protocol.
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
            ("\u{F075}  Popup contents".to_string(), 13.0, v.text_color),
            (format!("Align: {align_desc}"), 12.0, v.text_dim),
            (
                format!("Gap: {:.0} px", self.cfg.gap.get()),
                12.0,
                v.text_dim,
            ),
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

/// Build the scrollable configuration panel. The column is wrapped in a
/// `Rebuilder` keyed on `reset_epoch` so the reset button can refresh widgets
/// that lack a cell binding (the gap `DragValue`).
fn build_config_panel(font: Arc<Font>, cfg: &Config) -> Box<dyn Widget> {
    let epoch = Rc::clone(&cfg.reset_epoch);
    let cfg = cfg.clone();
    Box::new(ScrollView::new(Box::new(Rebuilder::new(
        move || epoch.get(),
        move || build_config_column(Arc::clone(&font), &cfg),
    ))))
}

/// The configurator rows, presented code-style like egui's demo
/// (`let align = RectAlign { parent: …, child: … };` and friends).
fn build_config_column(font: Arc<Font>, cfg: &Config) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(6.0)
        .with_padding(12.0)
        .with_panel_bg();

    // "let align = RectAlign {"  +  reset button on the right.
    {
        let reset_cfg = cfg.clone();
        col.push(
            Box::new(
                FlexRow::new()
                    .with_gap(8.0)
                    .add(Box::new(code_label("let align = RectAlign {", &font)))
                    .add_flex(Box::new(Spacer::new()), 1.0)
                    .add(Box::new(Tooltip::new(
                        // FA arrow-rotate-left stands in for egui's "⟲".
                        Box::new(
                            SizedBox::new()
                                .with_width(30.0)
                                .with_height(24.0)
                                .with_child(Box::new(
                                    Button::new("\u{F0E2}", Arc::clone(&font))
                                        .with_font_size(12.0)
                                        .on_click(move || reset_cfg.reset()),
                                )),
                        ),
                        "Reset to defaults",
                        Arc::clone(&font),
                    ))),
            ),
            0.0,
        );
    }

    let anchor_labels: Vec<&str> = Align2::ALL.iter().map(|(_, l)| *l).collect();
    col.push(
        combo_row(
            "    parent: Align2::",
            &anchor_labels,
            Rc::clone(&cfg.parent_idx),
            None,
            Arc::clone(&font),
        ),
        0.0,
    );
    col.push(
        combo_row(
            "    child: Align2::",
            &anchor_labels,
            Rc::clone(&cfg.child_idx),
            None,
            Arc::clone(&font),
        ),
        0.0,
    );
    col.push(Box::new(code_label("};", &font)), 0.0);

    // Preset combo: "<Select Preset>" placeholder + the 12 named presets,
    // like egui's single preset ComboBox.
    {
        let mut preset_labels: Vec<&str> = vec!["<Select Preset>"];
        preset_labels.extend(RectAlign::PRESETS.iter().map(|(_, l)| *l));
        col.push(
            combo_row(
                "let align = RectAlign::",
                &preset_labels,
                Rc::clone(&cfg.preset_idx),
                None,
                Arc::clone(&font),
            ),
            0.0,
        );
    }

    // Gap DragValue.
    {
        let gap = Rc::clone(&cfg.gap);
        col.push(
            Box::new(
                FlexRow::new()
                    .with_gap(8.0)
                    .add(Box::new(
                        SizedBox::new()
                            .with_width(CAPTION_W)
                            .with_child(Box::new(code_label("let gap = ", &font))),
                    ))
                    .add(Box::new(
                        SizedBox::new()
                            .with_width(110.0)
                            .with_height(26.0)
                            .with_child(Box::new(
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

    // Close-behavior combo with per-item hover tooltips (egui parity).
    {
        let behavior_names: Vec<&str> =
            PopupCloseBehavior::ALL.iter().map(|(_, n, _)| *n).collect();
        let behavior_tips: Vec<&str> =
            PopupCloseBehavior::ALL.iter().map(|(_, _, d)| *d).collect();
        col.push(
            combo_row(
                "let close_behavior = PopupCloseBehavior::",
                &behavior_names,
                Rc::clone(&cfg.behavior_idx),
                Some(behavior_tips),
                Arc::clone(&font),
            ),
            0.0,
        );
    }

    // popup_open checkbox — bidirectional with the Popup controller.
    {
        let open_flag = Rc::clone(&cfg.open_flag);
        let pending = Rc::clone(&cfg.pending_open);
        col.push(
            Box::new(
                FlexRow::new()
                    .with_gap(8.0)
                    .add(Box::new(
                        SizedBox::new()
                            .with_width(CAPTION_W)
                            .with_child(Box::new(code_label("let popup_open = ", &font))),
                    ))
                    .add(Box::new(
                        Checkbox::new("", Arc::clone(&font), open_flag.get())
                            .with_font_size(13.0)
                            .with_state_cell(Rc::clone(&open_flag))
                            .on_change(move |checked| pending.set(Some(checked))),
                    )),
            ),
            0.0,
        );
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

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

    Box::new(col)
}

/// A caption label for the code-style configurator rows.
fn code_label(text: &str, font: &Arc<Font>) -> Label {
    Label::new(text, Arc::clone(font)).with_font_size(13.0)
}

/// A caption + `ComboBox` row bound to `idx`, with optional per-item hover
/// tooltips. Long captions get a widened caption column; the combo takes the
/// remaining width.
fn combo_row(
    caption: &str,
    options: &[&str],
    idx: Rc<Cell<usize>>,
    tooltips: Option<Vec<&str>>,
    font: Arc<Font>,
) -> Box<dyn Widget> {
    let mut combo = ComboBox::new(options.to_vec(), idx.get(), Arc::clone(&font))
        .with_font_size(13.0)
        .with_selected_cell(idx);
    if let Some(tips) = tooltips {
        combo = combo.with_item_tooltips(tips);
    }
    let caption_w = (caption.len() as f64 * 6.7).max(CAPTION_W).min(280.0);
    Box::new(
        FlexRow::new()
            .with_gap(8.0)
            .add(Box::new(
                SizedBox::new()
                    .with_width(caption_w)
                    .with_child(Box::new(code_label(caption, &font))),
            ))
            .add_flex(Box::new(combo), 1.0),
    )
}
