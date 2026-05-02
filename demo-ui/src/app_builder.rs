#![allow(unused_imports)]
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    AccentColor, App, FlexColumn, FlexRow, Font, InspectorNode, InspectorPanel, Key, Modifiers,
    Rect, Size, Stack, ThemePreference, Widget, Window,
};

use crate::api::{DemoHandles, PlatformHooks};
use crate::backend_panel::{build_backend_panel, FrameHistory, RunMode};
use crate::content::build_demo_content;
use crate::shell::{BackendPane, CanvasBg, SidebarPane, TopMenuBar};
use crate::sidebar::{build_sidebar, SidebarEntry, SidebarGroup};
use crate::specs::{find_cube_idx, tile_rect, DEMOS, TESTS};
use crate::state::{SavedState, StateAccessor};
use crate::top_bar::{self, build_top_bar_inner};
use crate::windows;

pub fn build_demo_ui(
    font: Arc<Font>,
    cube_widget: Box<dyn Widget>,
    renderer_name: &'static str,
    backend_name: &'static str,
    initial_state: Option<SavedState>,
    platform: PlatformHooks,
) -> (App, DemoHandles) {
    let show_inspector = Rc::new(Cell::new(
        initial_state
            .as_ref()
            .and_then(|s| s.inspector.as_ref().map(|i| i.open))
            .unwrap_or(false),
    ));
    let inspector_nodes = Rc::new(RefCell::new(Vec::<InspectorNode>::new()));
    let hovered_bounds = Rc::new(RefCell::new(None::<agg_gui::InspectorOverlay>));
    let base_edits: Rc<RefCell<Vec<agg_gui::WidgetBaseEdit>>> =
        Rc::new(RefCell::new(Vec::new()));
    #[cfg(feature = "reflect")]
    let inspector_edits: Rc<RefCell<Vec<agg_gui::InspectorEdit>>> =
        Rc::new(RefCell::new(Vec::new()));
    let screen_size = Rc::new(Cell::new((0u32, 0u32)));
    let window_fullscreen = Rc::new(Cell::new(
        initial_state
            .as_ref()
            .map(|s| s.window_fullscreen)
            .unwrap_or(false),
    ));
    let window_maximized = Rc::new(Cell::new(
        initial_state
            .as_ref()
            .map(|s| s.window_maximized)
            .unwrap_or(false),
    ));
    let screenshot_request = Rc::new(Cell::new(false));
    let screenshot_image: Rc<RefCell<Option<(Arc<Vec<u8>>, u32, u32)>>> =
        Rc::new(RefCell::new(None));
    let screenshot_capturing = Rc::new(Cell::new(false));
    let initial_theme = initial_state
        .as_ref()
        .map(|s| s.theme_pref)
        .unwrap_or_else(top_bar::detect_system_theme);
    let initial_accent = initial_state
        .as_ref()
        .map(|s| s.accent_color)
        .unwrap_or(AccentColor::Blue);
    top_bar::apply_theme_visuals(initial_theme, initial_accent);
    let theme_pref = Rc::new(Cell::new(initial_theme));
    let accent_color = Rc::new(Cell::new(initial_accent));
    let backend_initially_open = initial_state
        .as_ref()
        .map(|st| st.backend_open)
        .unwrap_or(false);
    let show_backend = Rc::new(Cell::new(backend_initially_open));
    let run_mode = Rc::new(Cell::new(RunMode::Reactive));
    let mobile_menu_open = Rc::new(Cell::new(false));
    let frame_history = Rc::new(RefCell::new(FrameHistory::new()));
    let about_initially_open = initial_state
        .as_ref()
        .map(|st| st.about.open)
        .unwrap_or(true);
    let about_open = Rc::new(Cell::new(about_initially_open));
    let demo_entries: Vec<SidebarEntry> = DEMOS
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let open = initial_state
                .as_ref()
                .and_then(|st| st.demos.get(i))
                .map(|ws| ws.open)
                .unwrap_or(s.open);
            SidebarEntry::new(s.label, open)
        })
        .collect();
    let test_entries: Vec<SidebarEntry> = TESTS
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let open = initial_state
                .as_ref()
                .and_then(|st| st.tests.get(i))
                .map(|ws| ws.open)
                .unwrap_or(s.open);
            SidebarEntry::new(s.label, open)
        })
        .collect();
    let cube_idx = find_cube_idx();
    let cube_visible = Rc::clone(&demo_entries[cube_idx].open);
    let z_order_cell: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(
        initial_state
            .as_ref()
            .map(|s| s.z_order.clone())
            .unwrap_or_default(),
    ));
    let make_on_raised = || {
        let cell = Rc::clone(&z_order_cell);
        move |title: &str| {
            let mut v = cell.borrow_mut();
            v.retain(|t| t != title);
            v.push(title.to_string());
        }
    };
    let font_name_cell: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(
        initial_state.as_ref().and_then(|s| s.font_name.clone()),
    ));
    let font_size_scale_cell: Rc<Cell<f64>> = Rc::new(Cell::new(
        initial_state
            .as_ref()
            .map(|s| s.font_size_scale)
            .unwrap_or(1.0),
    ));
    let standard_dpi = agg_gui::device_scale() <= 1.25;
    let lcd_enabled_cell: Rc<Cell<bool>> = Rc::new(Cell::new(
        initial_state
            .as_ref()
            .map(|s| s.lcd_enabled)
            .unwrap_or(standard_dpi),
    ));
    let hinting_enabled_cell: Rc<Cell<bool>> = Rc::new(Cell::new(
        initial_state
            .as_ref()
            .map(|s| s.hinting_enabled)
            .unwrap_or(standard_dpi),
    ));
    let gamma_cell: Rc<Cell<f64>> = Rc::new(Cell::new(
        initial_state.as_ref().map(|s| s.gamma).unwrap_or(1.0),
    ));
    let width_scale_cell: Rc<Cell<f64>> = Rc::new(Cell::new(
        initial_state.as_ref().map(|s| s.width_scale).unwrap_or(1.0),
    ));
    let interval_cell: Rc<Cell<f64>> = Rc::new(Cell::new(
        initial_state.as_ref().map(|s| s.interval).unwrap_or(0.0),
    ));
    let faux_weight_cell: Rc<Cell<f64>> = Rc::new(Cell::new(
        initial_state.as_ref().map(|s| s.faux_weight).unwrap_or(0.0),
    ));
    let faux_italic_cell: Rc<Cell<f64>> = Rc::new(Cell::new(
        initial_state.as_ref().map(|s| s.faux_italic).unwrap_or(0.0),
    ));
    let primary_weight_cell: Rc<Cell<f64>> = Rc::new(Cell::new(
        initial_state
            .as_ref()
            .map(|s| s.primary_weight)
            .unwrap_or(1.0 / 3.0),
    ));
    let msaa_samples_cell: Rc<Cell<u8>> = Rc::new(Cell::new(
        initial_state.as_ref().map(|s| s.msaa_samples).unwrap_or(0),
    ));
    let system_tab_cell: Rc<Cell<usize>> = Rc::new(Cell::new(
        initial_state.as_ref().map(|s| s.system_tab).unwrap_or(0),
    ));
    let font_index_cell: Rc<Cell<usize>> = Rc::new(Cell::new({
        let name_lock = font_name_cell.borrow();
        name_lock
            .as_deref()
            .and_then(windows::font_option_index)
            .unwrap_or_else(windows::default_font_index)
    }));
    agg_gui::font_settings::set_font_size_scale(font_size_scale_cell.get());
    agg_gui::font_settings::set_lcd_enabled(lcd_enabled_cell.get());
    agg_gui::font_settings::set_hinting_enabled(hinting_enabled_cell.get());
    agg_gui::font_settings::set_gamma(gamma_cell.get());
    agg_gui::font_settings::set_width(width_scale_cell.get());
    agg_gui::font_settings::set_interval(interval_cell.get());
    agg_gui::font_settings::set_faux_weight(faux_weight_cell.get());
    agg_gui::font_settings::set_faux_italic(faux_italic_cell.get());
    agg_gui::font_settings::set_primary_weight(primary_weight_cell.get());
    let resolved_font_idx = font_name_cell
        .borrow()
        .as_deref()
        .and_then(windows::font_option_index)
        .unwrap_or_else(|| font_index_cell.get());
    windows::init_system_cells(windows::SystemCells {
        font_name: Rc::clone(&font_name_cell),
        font_index: Rc::clone(&font_index_cell),
        font_size_scale: Rc::clone(&font_size_scale_cell),
        lcd_enabled: Rc::clone(&lcd_enabled_cell),
        hinting_enabled: Rc::clone(&hinting_enabled_cell),
        gamma: Rc::clone(&gamma_cell),
        width_scale: Rc::clone(&width_scale_cell),
        interval: Rc::clone(&interval_cell),
        faux_weight: Rc::clone(&faux_weight_cell),
        faux_italic: Rc::clone(&faux_italic_cell),
        primary_weight: Rc::clone(&primary_weight_cell),
        msaa_samples: Rc::clone(&msaa_samples_cell),
        system_tab: Rc::clone(&system_tab_cell),
        platform: platform.clone(),
    });
    {
        let cells = windows::system_cells();
        windows::request_font_by_index(&cells, resolved_font_idx);
    }
    let all_specs_count = DEMOS.len() + TESTS.len();
    let reset_cells: Vec<Rc<Cell<Option<Rect>>>> = (0..all_specs_count)
        .map(|_| Rc::new(Cell::new(None)))
        .collect();
    let demo_pos_cells: Vec<Rc<Cell<Rect>>> = (0..DEMOS.len())
        .map(|_| Rc::new(Cell::new(Rect::default())))
        .collect();
    let demo_max_cells: Vec<Rc<Cell<bool>>> = DEMOS
        .iter()
        .enumerate()
        .map(|(i, _)| {
            Rc::new(Cell::new(
                initial_state
                    .as_ref()
                    .and_then(|st| st.demos.get(i))
                    .map(|ws| ws.maximized)
                    .unwrap_or(false),
            ))
        })
        .collect();
    let test_pos_cells: Vec<Rc<Cell<Rect>>> = (0..TESTS.len())
        .map(|_| Rc::new(Cell::new(Rect::default())))
        .collect();
    let test_max_cells: Vec<Rc<Cell<bool>>> = TESTS
        .iter()
        .enumerate()
        .map(|(i, _)| {
            Rc::new(Cell::new(
                initial_state
                    .as_ref()
                    .and_then(|st| st.tests.get(i))
                    .map(|ws| ws.maximized)
                    .unwrap_or(false),
            ))
        })
        .collect();
    let about_pos_cell: Rc<Cell<Rect>> = Rc::new(Cell::new(Rect::default()));
    let about_max_cell: Rc<Cell<bool>> = Rc::new(Cell::new(
        initial_state
            .as_ref()
            .map(|st| st.about.maximized)
            .unwrap_or(false),
    ));
    let default_canvas_h = 720.0_f64;
    let rc_for_cb: Vec<_> = reset_cells.iter().map(Rc::clone).collect();
    let rc_for_key: Vec<_> = reset_cells.iter().map(Rc::clone).collect();
    let specs_w: Vec<f64> = DEMOS
        .iter()
        .map(|s| s.win_w)
        .chain(TESTS.iter().map(|s| s.win_w))
        .collect();
    let specs_h: Vec<f64> = DEMOS
        .iter()
        .map(|s| s.win_h)
        .chain(TESTS.iter().map(|s| s.win_h))
        .collect();
    let on_organize = {
        let sw = specs_w.clone();
        let sh = specs_h.clone();
        move || {
            for (i, cell) in rc_for_cb.iter().enumerate() {
                let r = tile_rect(i, default_canvas_h, sw[i], sh[i]);
                cell.set(Some(r));
            }
        }
    };
    let tool_entries: Vec<SidebarEntry> = vec![SidebarEntry::from_cell(
        "\u{F188} Inspector",
        Rc::clone(&show_inspector),
    )];
    let group_names: &[&'static str] = &[
        "Widgets",
        "Layout",
        "Graphics",
        "Interaction",
        "Tests",
        "Window Resize Test",
        "Tools",
    ];
    fn sidebar_sort_key(s: &str) -> String {
        s.trim_start_matches(|c: char| {
            let cp = c as u32;
            (0xE000..=0xF8FF).contains(&cp)
        })
        .trim_start()
        .to_lowercase()
    }
    let sidebar_groups: Vec<SidebarGroup> = group_names
        .iter()
        .map(|&name| {
            let mut entries: Vec<&SidebarEntry> = match name {
                "Tests" => test_entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| !e.label.starts_with('↔'))
                    .map(|(_, e)| e)
                    .collect(),
                "Window Resize Test" => test_entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.label.starts_with('↔'))
                    .map(|(_, e)| e)
                    .collect(),
                "Tools" => tool_entries
                    .iter()
                    .chain(
                        demo_entries
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| DEMOS[*i].group == "Tools")
                            .map(|(_, e)| e),
                    )
                    .collect(),
                _ => demo_entries
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| DEMOS[*i].group == name)
                    .map(|(_, e)| e)
                    .collect(),
            };
            entries.sort_by(|a, b| sidebar_sort_key(a.label).cmp(&sidebar_sort_key(b.label)));
            SidebarGroup { name, entries }
        })
        .collect();
    let sidebar_widget = build_sidebar(
        Arc::clone(&font),
        Rc::clone(&about_open),
        &sidebar_groups,
        on_organize,
    );
    let sidebar_panel = SidebarPane::new(sidebar_widget, Rc::clone(&mobile_menu_open));
    let mut canvas = Stack::new().add(Box::new(CanvasBg::new()));
    for (i, spec) in DEMOS.iter().enumerate() {
        let open_cell = Rc::clone(&demo_entries[i].open);
        let reset_cell = Rc::clone(&reset_cells[i]);
        let initial = initial_state
            .as_ref()
            .and_then(|st| st.demos.get(i))
            .filter(|ws| ws.has_valid_bounds())
            .map(|ws| ws.to_rect())
            .unwrap_or_else(|| tile_rect(i, default_canvas_h, spec.win_w, spec.win_h));
        let content: Box<dyn Widget> = if i == cube_idx {
            windows::coming_soon()
        } else {
            build_demo_content(
                spec.title,
                Arc::clone(&font),
                Rc::clone(&screenshot_request),
                Rc::clone(&screenshot_image),
                Rc::clone(&screenshot_capturing),
            )
        };
        let auto_size = spec.title == "\u{F096} Frame";
        let win = Window::new(spec.title, Arc::clone(&font), content)
            .with_bounds(Rect::new(
                initial.x,
                initial.y,
                initial.width,
                initial.height,
            ))
            .with_visible_cell(open_cell)
            .with_reset_cell(reset_cell)
            .with_position_cell(Rc::clone(&demo_pos_cells[i]))
            .with_maximized_cell(Rc::clone(&demo_max_cells[i]))
            .with_auto_size(auto_size)
            .on_raised(make_on_raised());
        canvas = canvas.add(Box::new(win));
    }
    {
        let open_cell = Rc::clone(&demo_entries[cube_idx].open);
        let reset_cell = Rc::clone(&reset_cells[cube_idx]);
        let spec = &DEMOS[cube_idx];
        let initial = initial_state
            .as_ref()
            .and_then(|st| st.demos.get(cube_idx))
            .filter(|ws| ws.has_valid_bounds())
            .map(|ws| ws.to_rect())
            .unwrap_or_else(|| tile_rect(cube_idx, default_canvas_h, spec.win_w, spec.win_h));
        let content = windows::cube_content(Arc::clone(&font), cube_widget);
        let win = Window::new(spec.title, Arc::clone(&font), content)
            .with_bounds(Rect::new(
                initial.x,
                initial.y,
                initial.width,
                initial.height,
            ))
            .with_visible_cell(open_cell)
            .with_reset_cell(reset_cell)
            .with_position_cell(Rc::clone(&demo_pos_cells[cube_idx]))
            .with_maximized_cell(Rc::clone(&demo_max_cells[cube_idx]))
            .on_raised(make_on_raised());
        canvas.children_mut()[1 + cube_idx] = Box::new(win);
    }
    let mut resize_sub: std::collections::HashMap<String, windows::ResizeTestWindow> =
        windows::window_resize_sub_windows(Arc::clone(&font))
            .into_iter()
            .map(|e| (e.title.clone(), e))
            .collect();
    for (i, spec) in TESTS.iter().enumerate() {
        let total_i = DEMOS.len() + i;
        let open_cell = Rc::clone(&test_entries[i].open);
        let reset_cell = Rc::clone(&reset_cells[total_i]);
        let initial = initial_state
            .as_ref()
            .and_then(|st| st.tests.get(i))
            .filter(|ws| ws.has_valid_bounds())
            .map(|ws| ws.to_rect())
            .unwrap_or_else(|| tile_rect(total_i, default_canvas_h, spec.win_w, spec.win_h));
        if let Some(sub) = resize_sub.remove(spec.title) {
            let mut win = Window::new(spec.title, Arc::clone(&font), sub.content)
                .with_bounds(Rect::new(
                    initial.x,
                    initial.y,
                    initial.width,
                    initial.height,
                ))
                .with_visible_cell(open_cell)
                .with_reset_cell(reset_cell)
                .with_position_cell(Rc::clone(&test_pos_cells[i]))
                .with_maximized_cell(Rc::clone(&test_max_cells[i]))
                .on_raised(make_on_raised());
            if sub.vscroll {
                win = win.with_vscroll(true);
            }
            if sub.auto_size {
                win = win.with_auto_size(true);
            } else {
                win = win.with_resizable_axes(sub.resizable_h, sub.resizable_v);
                if !sub.resizable {
                    win = win.with_resizable(false);
                }
            }
            if sub.tight_fit {
                win = win.with_tight_content_fit(true);
            }
            if sub.floor_fit {
                win = win.with_height_floor_to_content(true);
            }
            canvas = canvas.add(Box::new(win));
            continue;
        }
        let content: Box<dyn Widget> = match spec.title {
            "\u{F0EA} Clipboard Test" => windows::clipboard_test(Arc::clone(&font)),
            "\u{F05B} Cursor Test" => windows::cursor_test(Arc::clone(&font)),
            "\u{F00A} Grid Test" => windows::grid_test(Arc::clone(&font)),
            "\u{F007} Id Test" => windows::id_test(Arc::clone(&font)),
            "\u{F1DA} Input Event History" => windows::input_event_history(Arc::clone(&font)),
            "\u{F11C} Input Test" => windows::input_test(Arc::clone(&font)),
            "\u{F0E4} Layout Test" => windows::layout_test(Arc::clone(&font)),
            "\u{F0AD} Manual Layout Test" => windows::manual_layout_test(Arc::clone(&font)),
            "\u{F03E} SVG Test" => windows::svg_test(Arc::clone(&font)),
            _ => windows::coming_soon(),
        };
        let win = Window::new(spec.title, Arc::clone(&font), content)
            .with_bounds(Rect::new(
                initial.x,
                initial.y,
                initial.width,
                initial.height,
            ))
            .with_visible_cell(open_cell)
            .with_reset_cell(reset_cell)
            .with_position_cell(Rc::clone(&test_pos_cells[i]))
            .with_maximized_cell(Rc::clone(&test_max_cells[i]))
            .on_raised(make_on_raised());
        canvas = canvas.add(Box::new(win));
    }
    {
        let about_initial = initial_state
            .as_ref()
            .map(|st| &st.about)
            .filter(|ws| ws.has_valid_bounds())
            .map(|ws| ws.to_rect())
            .unwrap_or_else(|| Rect::new(80.0, 80.0, 440.0, 500.0));
        let about_win = Window::new(
            "About agg-gui",
            Arc::clone(&font),
            windows::about(Arc::clone(&font)),
        )
        .with_bounds(about_initial)
        .with_visible_cell(Rc::clone(&about_open))
        .with_position_cell(Rc::clone(&about_pos_cell))
        .with_maximized_cell(Rc::clone(&about_max_cell))
        .on_raised(make_on_raised());
        canvas = canvas.add(Box::new(about_win));
    }
    let inspector_snapshot_cell: Rc<RefCell<Option<agg_gui::InspectorSavedState>>> =
        Rc::new(RefCell::new(None));
    // Inspector window geometry — persisted just like the demo windows.
    // The position cell is mirrored back into the saved state via
    // `inspector_snapshot` so size + position + maximize survive restarts.
    const INSPECTOR_DEFAULT_BOUNDS: Rect = Rect {
        x: 960.0,
        y: 60.0,
        width: 320.0,
        height: 520.0,
    };
    let inspector_saved_window = initial_state
        .as_ref()
        .and_then(|s| s.inspector.as_ref())
        .and_then(|i| i.window.clone())
        .filter(|w| w.has_valid_bounds());
    let inspector_initial_bounds = inspector_saved_window
        .as_ref()
        .map(|w| w.to_rect())
        .unwrap_or(INSPECTOR_DEFAULT_BOUNDS);
    let inspector_pos_cell = Rc::new(Cell::new(inspector_initial_bounds));
    let inspector_max_cell = Rc::new(Cell::new(
        inspector_saved_window
            .as_ref()
            .map(|w| w.maximized)
            .unwrap_or(false),
    ));
    {
        let mut inspector = InspectorPanel::new(
            Arc::clone(&font),
            Rc::clone(&inspector_nodes),
            Rc::clone(&hovered_bounds),
        )
        .with_snapshot_cell(Rc::clone(&inspector_snapshot_cell))
        .with_base_edit_queue(Rc::clone(&base_edits));
        #[cfg(feature = "reflect")]
        {
            inspector = inspector.with_edit_queue(Rc::clone(&inspector_edits));
        }
        if let Some(saved) = initial_state.as_ref().and_then(|s| s.inspector.clone()) {
            inspector.apply_saved_state(agg_gui::InspectorSavedState {
                expanded: saved.expanded,
                selected: saved.selected,
                props_h: saved.props_h,
            });
        }
        let inspector_win =
            Window::new("\u{F188} Inspector", Arc::clone(&font), Box::new(inspector))
                .with_bounds(inspector_initial_bounds)
                .with_visible_cell(Rc::clone(&show_inspector))
                .with_position_cell(Rc::clone(&inspector_pos_cell))
                .with_maximized_cell(Rc::clone(&inspector_max_cell))
                .on_raised(make_on_raised());
        canvas = canvas.add(Box::new(inspector_win));
    }
    {
        let saved_order = z_order_cell.borrow().clone();
        if !saved_order.is_empty() {
            let kids = canvas.children_mut();
            for title in &saved_order {
                if let Some(idx) = kids.iter().position(|w| w.id() == Some(title.as_str())) {
                    let win = kids.remove(idx);
                    kids.push(win);
                }
            }
        }
    }
    let main_area = canvas;
    let on_reset_all = {
        let demo_open = demo_entries
            .iter()
            .map(|e| Rc::clone(&e.open))
            .collect::<Vec<_>>();
        let test_open = test_entries
            .iter()
            .map(|e| Rc::clone(&e.open))
            .collect::<Vec<_>>();
        let about_open = Rc::clone(&about_open);
        let reset_cells = reset_cells.iter().map(Rc::clone).collect::<Vec<_>>();
        let specs_w = specs_w.clone();
        let specs_h = specs_h.clone();
        let font_scale = Rc::clone(&font_size_scale_cell);
        let lcd_cell = Rc::clone(&lcd_enabled_cell);
        let hint_cell = Rc::clone(&hinting_enabled_cell);
        let gamma = Rc::clone(&gamma_cell);
        let width_scl = Rc::clone(&width_scale_cell);
        let interval = Rc::clone(&interval_cell);
        let fweight = Rc::clone(&faux_weight_cell);
        let fitalic = Rc::clone(&faux_italic_cell);
        let pweight = Rc::clone(&primary_weight_cell);
        move || {
            for c in &demo_open {
                c.set(false);
            }
            for c in &test_open {
                c.set(false);
            }
            about_open.set(false);
            for (i, cell) in reset_cells.iter().enumerate() {
                let r = tile_rect(i, default_canvas_h, specs_w[i], specs_h[i]);
                cell.set(Some(r));
            }
            let default_idx = windows::default_font_index();
            let standard_dpi = agg_gui::device_scale() <= 1.25;
            let cells = windows::system_cells();
            windows::apply_font_by_index(&cells, default_idx);
            agg_gui::font_settings::set_font_size_scale(1.0);
            agg_gui::font_settings::set_lcd_enabled(standard_dpi);
            agg_gui::font_settings::set_hinting_enabled(standard_dpi);
            agg_gui::font_settings::set_gamma(1.0);
            agg_gui::font_settings::set_width(1.0);
            agg_gui::font_settings::set_interval(0.0);
            agg_gui::font_settings::set_faux_weight(0.0);
            agg_gui::font_settings::set_faux_italic(0.0);
            agg_gui::font_settings::set_primary_weight(1.0 / 3.0);
            font_scale.set(1.0);
            lcd_cell.set(standard_dpi);
            hint_cell.set(standard_dpi);
            gamma.set(1.0);
            width_scl.set(1.0);
            interval.set(0.0);
            fweight.set(0.0);
            fitalic.set(0.0);
            pweight.set(1.0 / 3.0);
        }
    };
    let system_open = {
        let idx = DEMOS
            .iter()
            .position(|d| d.title == "\u{F013} System")
            .expect("DEMOS must contain the System window entry");
        Rc::clone(&demo_entries[idx].open)
    };
    let backend_panel_widget = build_backend_panel(
        Arc::clone(&font),
        Rc::clone(&run_mode),
        Rc::clone(&frame_history),
        Rc::clone(&screen_size),
        Rc::clone(&show_inspector),
        system_open,
        renderer_name,
        backend_name,
        on_reset_all,
    );
    let backend_pane = BackendPane {
        bounds: Rect::default(),
        children: vec![backend_panel_widget],
        show: Rc::clone(&show_backend),
    };
    let demos_body = FlexRow::new()
        .with_gap(0.0)
        .add(Box::new(backend_pane))
        .add_flex(Box::new(main_area), 1.0)
        .add(Box::new(sidebar_panel));
    let top_bar_inner = build_top_bar_inner(
        Arc::clone(&font),
        Rc::clone(&show_backend),
        Rc::clone(&mobile_menu_open),
        Rc::clone(&theme_pref),
        Rc::clone(&accent_color),
    );
    let root = FlexColumn::new()
        .with_gap(0.0)
        .add(Box::new(TopMenuBar::new(top_bar_inner)))
        .add_flex(Box::new(demos_body), 1.0);
    let mut app = App::new(Box::new(root));
    let on_organize_key = {
        move || {
            for (i, cell) in rc_for_key.iter().enumerate() {
                let r = tile_rect(i, default_canvas_h, specs_w[i], specs_h[i]);
                cell.set(Some(r));
            }
        }
    };
    let demo_open_cells: Vec<Rc<Cell<bool>>> =
        demo_entries.iter().map(|e| Rc::clone(&e.open)).collect();
    let test_open_cells: Vec<Rc<Cell<bool>>> =
        test_entries.iter().map(|e| Rc::clone(&e.open)).collect();
    app.set_global_key_handler({
        let on_org = on_organize_key;
        move |key: Key, mods: Modifiers| {
            if mods.ctrl && mods.shift {
                match key {
                    Key::Char('O') | Key::Char('o') => {
                        on_org();
                        return true;
                    }
                    Key::Char('R') | Key::Char('r') => {
                        for c in &demo_open_cells {
                            c.set(false);
                        }
                        for c in &test_open_cells {
                            c.set(false);
                        }
                        return true;
                    }
                    _ => {}
                }
            }
            false
        }
    });
    let state_accessor = StateAccessor {
        demo_open: demo_entries.iter().map(|e| Rc::clone(&e.open)).collect(),
        demo_pos: demo_pos_cells,
        demo_maximized: demo_max_cells,
        test_open: test_entries.iter().map(|e| Rc::clone(&e.open)).collect(),
        test_pos: test_pos_cells,
        test_maximized: test_max_cells,
        about_open: Rc::clone(&about_open),
        about_pos: about_pos_cell,
        about_maximized: about_max_cell,
        backend_open: Rc::clone(&show_backend),
        theme_pref: Rc::clone(&theme_pref),
        accent_color: Rc::clone(&accent_color),
        window_size: Rc::clone(&screen_size),
        window_fullscreen: Rc::clone(&window_fullscreen),
        window_maximized: Rc::clone(&window_maximized),
        inspector_snapshot: {
            let cell = Rc::clone(&inspector_snapshot_cell);
            let open_cell = Rc::clone(&show_inspector);
            let pos_cell = Rc::clone(&inspector_pos_cell);
            let max_cell = Rc::clone(&inspector_max_cell);
            Rc::new(move || {
                cell.borrow()
                    .as_ref()
                    .map(|s| {
                        let r = pos_cell.get();
                        crate::state::InspectorPersist {
                            expanded: s.expanded.clone(),
                            selected: s.selected,
                            props_h: s.props_h,
                            open: open_cell.get(),
                            window: Some(crate::state::WindowState {
                                open: open_cell.get(),
                                x: r.x,
                                y: r.y,
                                w: r.width,
                                h: r.height,
                                maximized: max_cell.get(),
                            }),
                        }
                    })
            })
        },
        font_name: Rc::clone(&font_name_cell),
        font_size_scale: Rc::clone(&font_size_scale_cell),
        lcd_enabled: Rc::clone(&lcd_enabled_cell),
        hinting_enabled: Rc::clone(&hinting_enabled_cell),
        gamma: Rc::clone(&gamma_cell),
        width_scale: Rc::clone(&width_scale_cell),
        interval: Rc::clone(&interval_cell),
        faux_weight: Rc::clone(&faux_weight_cell),
        faux_italic: Rc::clone(&faux_italic_cell),
        primary_weight: Rc::clone(&primary_weight_cell),
        msaa_samples: Rc::clone(&msaa_samples_cell),
        system_tab: Rc::clone(&system_tab_cell),
        z_order: Rc::clone(&z_order_cell),
    };
    let handles = DemoHandles {
        show_inspector,
        inspector_nodes,
        hovered_bounds,
        base_edits,
        #[cfg(feature = "reflect")]
        inspector_edits,
        cube_visible,
        run_mode,
        screen_size,
        frame_history,
        window_fullscreen,
        window_maximized,
        screenshot_request: Rc::clone(&screenshot_request),
        screenshot_image: Rc::clone(&screenshot_image),
        screenshot_capturing: Rc::clone(&screenshot_capturing),
        state: state_accessor,
    };
    (app, handles)
}
