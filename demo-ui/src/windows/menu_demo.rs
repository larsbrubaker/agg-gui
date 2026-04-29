//! Menu infrastructure demo.
//!
//! Exercises the shared menu model through both a top menu bar and a
//! right-click context area so native and WASM builds cover the same behavior.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    DrawCtx, Event, EventResult, Font, MenuBar, MenuEntry, MenuItem, MenuResponse, MouseButton,
    Point, PopupMenu, Rect, Size, TopMenu, Widget,
};

pub fn menu_demo(font: Arc<Font>) -> Box<dyn Widget> {
    Box::new(MenuDemo::new(font))
}

struct MenuDemo {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    log: Rc<RefCell<Vec<String>>>,
    context_menu: PopupMenu,
    context_area: Rect,
}

impl MenuDemo {
    fn new(font: Arc<Font>) -> Self {
        let log = Rc::new(RefCell::new(vec![
            "Right-click the test area or open a top menu.".to_string(),
        ]));
        let top_log = Rc::clone(&log);
        let menu_bar = MenuBar::new(Arc::clone(&font), top_menus(), move |action| {
            push_log(&top_log, action);
        });
        Self {
            bounds: Rect::default(),
            children: vec![Box::new(menu_bar)],
            font,
            log,
            context_menu: PopupMenu::new(shared_items("context")),
            context_area: Rect::default(),
        }
    }
}

impl Widget for MenuDemo {
    fn type_name(&self) -> &'static str {
        "MenuDemo"
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }

    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        if let Some(menu_bar) = self.children.get_mut(0) {
            let bar_size = menu_bar.layout(Size::new(available.width, 26.0));
            menu_bar.set_bounds(Rect::new(
                0.0,
                (available.height - bar_size.height).max(0.0),
                available.width,
                bar_size.height,
            ));
        }
        self.context_area = Rect::new(
            14.0,
            (available.height - 26.0 - 140.0).max(58.0),
            (available.width - 28.0).max(0.0),
            120.0,
        );
        Size::new(available.width, f64::min(252.0, available.height))
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(14.0);

        ctx.set_fill_color(v.window_fill);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();

        ctx.set_fill_color(v.panel_fill);
        ctx.begin_path();
        ctx.rounded_rect(
            self.context_area.x,
            self.context_area.y,
            self.context_area.width,
            self.context_area.height,
            6.0,
        );
        ctx.fill();
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(
            self.context_area.x,
            self.context_area.y,
            self.context_area.width,
            self.context_area.height,
            6.0,
        );
        ctx.stroke();

        ctx.set_fill_color(v.text_color);
        ctx.fill_text(
            "Right-click here: icons, disabled rows, checks, shortcuts, submenus, shadow.",
            self.context_area.x + 12.0,
            self.context_area.y + self.context_area.height - 28.0,
        );

        ctx.fill_text("Action log:", 16.0, 54.0);
        for (idx, line) in self.log.borrow().iter().rev().take(3).enumerate() {
            ctx.fill_text(line, 16.0, 32.0 - idx as f64 * 18.0);
        }
    }

    fn hit_test_global_overlay(&self, _local_pos: Point) -> bool {
        self.context_menu.is_open()
    }

    fn has_active_modal(&self) -> bool {
        self.context_menu.is_open()
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if self.context_menu.is_open() {
            let (result, response) = self
                .context_menu
                .handle_event(event, agg_gui::widget::current_viewport());
            match response {
                MenuResponse::Action(action) => push_log(&self.log, &action),
                MenuResponse::Closed | MenuResponse::None => {}
            }
            if result == EventResult::Consumed {
                return result;
            }
        }

        match event {
            Event::MouseDown {
                pos,
                button: MouseButton::Right,
                ..
            } if contains(self.context_area, *pos) => {
                self.context_menu = PopupMenu::new(shared_items("context"));
                self.context_menu.open_at(*pos);
                agg_gui::animation::request_draw();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        self.context_menu.paint(
            ctx,
            Arc::clone(&self.font),
            14.0,
            agg_gui::widget::current_viewport(),
        );
    }
}

fn top_menus() -> Vec<TopMenu> {
    vec![
        TopMenu::new("File", shared_items("file")),
        TopMenu::new(
            "Edit",
            vec![
                MenuItem::action("Undo", "edit.undo")
                    .icon('\u{f0e2}')
                    .shortcut("Ctrl+Z")
                    .into(),
                MenuItem::action("Redo", "edit.redo")
                    .icon('\u{f01e}')
                    .shortcut("Ctrl+Y")
                    .disabled()
                    .into(),
                MenuEntry::Separator,
                MenuItem::action("Copy", "edit.copy")
                    .icon('\u{f0c5}')
                    .shortcut("Ctrl+C")
                    .into(),
            ],
        ),
        TopMenu::new("View", shared_items("view")),
    ]
}

fn shared_items(prefix: &str) -> Vec<MenuEntry> {
    vec![
        MenuItem::action("New", format!("{prefix}.new"))
            .icon('\u{f067}')
            .shortcut("Ctrl+N")
            .into(),
        MenuItem::action("Open", format!("{prefix}.open"))
            .icon('\u{f07c}')
            .shortcut("Ctrl+O")
            .into(),
        MenuEntry::Separator,
        MenuItem::action("Show Slider", format!("{prefix}.show-slider"))
            .checked(true)
            .keep_open()
            .into(),
        MenuItem::action("Option A", format!("{prefix}.option-a"))
            .radio(true)
            .keep_open()
            .into(),
        MenuItem::action("Option B", format!("{prefix}.option-b"))
            .radio(false)
            .keep_open()
            .into(),
        MenuItem::action("Disabled Item", format!("{prefix}.disabled"))
            .disabled()
            .into(),
        MenuItem::submenu(
            "More",
            vec![
                MenuItem::action("Nested Action", format!("{prefix}.nested")).into(),
                MenuItem::submenu(
                    "Deep Submenu",
                    vec![
                        MenuItem::action("Leaf One", format!("{prefix}.leaf-one")).into(),
                        MenuItem::action("Leaf Two", format!("{prefix}.leaf-two"))
                            .checked(true)
                            .into(),
                    ],
                )
                .into(),
            ],
        )
        .icon('\u{f0da}')
        .into(),
    ]
}

fn push_log(log: &Rc<RefCell<Vec<String>>>, action: &str) {
    let mut log = log.borrow_mut();
    log.push(format!("Action fired: {action}"));
    if log.len() > 8 {
        log.remove(0);
    }
    agg_gui::animation::request_draw();
}

fn contains(rect: Rect, pos: Point) -> bool {
    pos.x >= rect.x
        && pos.x <= rect.x + rect.width
        && pos.y >= rect.y
        && pos.y <= rect.y + rect.height
}
