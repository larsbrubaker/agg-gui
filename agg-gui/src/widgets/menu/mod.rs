//! Reusable menu infrastructure.
//!
//! This module provides the shared model, geometry, state, painter, and widget
//! adapters used by context menus and top menu bars.

pub mod geometry;
pub mod model;
pub mod paint;
pub mod state;
pub mod widget;

pub use geometry::{BAR_H as MENU_BAR_H, MENU_W, ROW_H};
pub use model::{MenuEntry, MenuItem, MenuSelection, MenuShortcut, ShortcutKey};
pub use paint::MenuStyle;
pub use state::{MenuAnchorKind, MenuResponse, PopupMenuState};
pub use widget::{MenuBar, PopupMenu, TopMenu};

#[cfg(test)]
mod tests {
    use crate::event::{Event, Key, Modifiers, MouseButton};
    use crate::geometry::{Point, Size};

    use super::geometry::{hit_test, stack_layout, MenuHit};
    use super::*;

    fn test_items() -> Vec<MenuEntry> {
        vec![
            MenuItem::action("Open", "open")
                .icon('\u{f07c}')
                .shortcut("Ctrl+O")
                .into(),
            MenuItem::action("Disabled", "disabled").disabled().into(),
            MenuEntry::Separator,
            MenuItem::submenu(
                "More",
                vec![
                    MenuItem::action("Leaf", "leaf").into(),
                    MenuItem::action("Checked", "checked").checked(true).into(),
                ],
            )
            .into(),
        ]
    }

    #[test]
    fn popup_clamps_to_viewport() {
        let items = test_items();
        let layouts = stack_layout(
            &items,
            Point::new(500.0, -50.0),
            MenuAnchorKind::Context,
            &[],
            Size::new(240.0, 120.0),
        );
        let rect = layouts[0].rect;
        assert!(rect.x >= 4.0);
        assert!(rect.y >= 4.0);
        assert!(rect.x + rect.width <= 240.0);
        assert!(rect.y + rect.height <= 120.0);
    }

    #[test]
    fn menu_bar_popups_can_open_below_the_bar() {
        let items = test_items();
        let layouts = stack_layout(
            &items,
            Point::new(20.0, 0.0),
            MenuAnchorKind::Bar,
            &[],
            Size::new(400.0, 240.0),
        );
        assert!(
            layouts[0].rect.y < 0.0,
            "bar popups use negative local Y so they paint below a top menu bar"
        );
    }

    #[test]
    fn hover_opens_submenu_and_hit_tests_nested_rows() {
        let items = test_items();
        let mut state = PopupMenuState::default();
        state.open_at(Point::new(20.0, 160.0), MenuAnchorKind::Context);
        let viewport = Size::new(400.0, 240.0);
        let layouts = state.layouts(&items, viewport);
        let more_row = layouts[0].rows[3].rect;

        assert!(state.update_hover(
            &items,
            Point::new(more_row.x + 10.0, more_row.y + 10.0),
            viewport
        ));
        assert_eq!(state.open_path, vec![3]);

        let layouts = state.layouts(&items, viewport);
        let submenu_row = layouts[1].rows[0].rect;
        assert!(matches!(
            hit_test(
                &layouts,
                Point::new(submenu_row.x + 10.0, submenu_row.y + 10.0)
            ),
            Some(MenuHit::Item(path)) if path == vec![3, 0]
        ));
    }

    #[test]
    fn action_click_consumes_and_suppresses_followup_mouse_up() {
        let mut items = test_items();
        let mut state = PopupMenuState::default();
        state.open_at(Point::new(20.0, 160.0), MenuAnchorKind::Context);
        let viewport = Size::new(400.0, 240.0);
        let first_row = state.layouts(&items, viewport)[0].rows[0].rect;

        let (_, response) = state.handle_event(
            &mut items,
            &Event::MouseDown {
                pos: Point::new(first_row.x + 10.0, first_row.y + 10.0),
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            },
            viewport,
        );
        assert_eq!(response, MenuResponse::Action("open".to_string()));
        assert!(state.take_suppress_mouse_up());
    }

    #[test]
    fn keep_open_check_and_radio_actions_do_not_close() {
        let mut items = vec![
            MenuItem::action("Check", "check")
                .checked(false)
                .keep_open()
                .into(),
            MenuItem::action("Radio A", "radio-a")
                .radio(true)
                .keep_open()
                .into(),
            MenuItem::action("Radio B", "radio-b")
                .radio(false)
                .keep_open()
                .into(),
        ];
        let mut state = PopupMenuState::default();
        state.open_at(Point::new(20.0, 120.0), MenuAnchorKind::Context);
        let viewport = Size::new(300.0, 200.0);
        let first_row = state.layouts(&items, viewport)[0].rows[0].rect;

        let (_, response) = state.handle_event(
            &mut items,
            &Event::MouseDown {
                pos: Point::new(first_row.x + 10.0, first_row.y + 10.0),
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            },
            viewport,
        );

        assert_eq!(response, MenuResponse::Action("check".to_string()));
        assert!(state.open);
        assert!(!state.should_suppress_mouse_up());
        let MenuEntry::Item(item) = &items[0] else {
            panic!("first row should be an item");
        };
        assert_eq!(item.selection, MenuSelection::Check { selected: true });

        let third_row = state.layouts(&items, viewport)[0].rows[2].rect;
        let (_, response) = state.handle_event(
            &mut items,
            &Event::MouseDown {
                pos: Point::new(third_row.x + 10.0, third_row.y + 10.0),
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            },
            viewport,
        );
        assert_eq!(response, MenuResponse::Action("radio-b".to_string()));
        assert!(state.open);
        let MenuEntry::Item(item) = &items[1] else {
            panic!("second row should be an item");
        };
        assert_eq!(item.selection, MenuSelection::Radio { selected: false });
        let MenuEntry::Item(item) = &items[2] else {
            panic!("third row should be an item");
        };
        assert_eq!(item.selection, MenuSelection::Radio { selected: true });
    }

    #[test]
    fn disabled_rows_do_not_fire_actions() {
        let mut items = test_items();
        let mut state = PopupMenuState::default();
        state.open_at(Point::new(20.0, 160.0), MenuAnchorKind::Context);
        let viewport = Size::new(400.0, 240.0);
        let disabled_row = state.layouts(&items, viewport)[0].rows[1].rect;

        let (_, response) = state.handle_event(
            &mut items,
            &Event::MouseDown {
                pos: Point::new(disabled_row.x + 10.0, disabled_row.y + 10.0),
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            },
            viewport,
        );
        assert_eq!(response, MenuResponse::None);
        assert!(state.open);
    }

    #[test]
    fn disabled_rows_do_not_become_hovered() {
        let items = test_items();
        let mut state = PopupMenuState::default();
        state.open_at(Point::new(20.0, 160.0), MenuAnchorKind::Context);
        let viewport = Size::new(400.0, 240.0);
        let disabled_row = state.layouts(&items, viewport)[0].rows[1].rect;

        assert!(!state.update_hover(
            &items,
            Point::new(disabled_row.x + 10.0, disabled_row.y + 10.0),
            viewport,
        ));
        assert_eq!(state.hover_path, None);
    }

    #[test]
    fn touch_synthesized_move_does_not_set_popup_hover() {
        // Regression: a touch tap synthesises a MouseMove at the tap point
        // before the MouseDown.  Without suppression, that move would set
        // `hover_path` and the post-tap state would still paint a hover
        // panel on the just-tapped row even though the menu has closed.
        // After the fix, an enabled-row MouseMove inside the touch-synth
        // window must leave `hover_path` as `None`.
        let items = test_items();
        let mut state = PopupMenuState::default();
        state.open_at(Point::new(20.0, 160.0), MenuAnchorKind::Context);
        let viewport = Size::new(400.0, 240.0);
        let first_row = state.layouts(&items, viewport)[0].rows[0].rect;

        // Force the touch-synth window to be active by recording a touch
        // event right before the MouseMove — same call the touch shells
        // make on every touchstart / touchmove / touchend.
        crate::touch_state::clear_last_touch_event_for_testing();
        crate::touch_state::note_touch_event();

        state.update_hover(
            &items,
            Point::new(first_row.x + 10.0, first_row.y + 10.0),
            viewport,
        );
        assert_eq!(
            state.hover_path, None,
            "a touch-synth MouseMove must not paint a popup-row hover"
        );

        // Reset for sibling tests.
        crate::touch_state::clear_last_touch_event_for_testing();
    }

    #[test]
    fn desktop_move_still_sets_popup_hover() {
        // Mirror test: outside the touch-synth window the same MouseMove
        // SHOULD set hover so desktop users see the subtle hover panel.
        let items = test_items();
        let mut state = PopupMenuState::default();
        state.open_at(Point::new(20.0, 160.0), MenuAnchorKind::Context);
        let viewport = Size::new(400.0, 240.0);
        let first_row = state.layouts(&items, viewport)[0].rows[0].rect;

        crate::touch_state::clear_last_touch_event_for_testing();

        assert!(state.update_hover(
            &items,
            Point::new(first_row.x + 10.0, first_row.y + 10.0),
            viewport,
        ));
        assert_eq!(state.hover_path, Some(vec![0]));
    }

    #[test]
    fn outside_click_dismisses_menu() {
        let mut items = test_items();
        let mut state = PopupMenuState::default();
        state.open_at(Point::new(20.0, 160.0), MenuAnchorKind::Context);
        let (_, response) = state.handle_event(
            &mut items,
            &Event::MouseDown {
                pos: Point::new(390.0, 10.0),
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            },
            Size::new(400.0, 240.0),
        );
        assert_eq!(response, MenuResponse::Closed);
        assert!(!state.open);
    }

    #[test]
    fn keyboard_navigation_activates_hovered_row() {
        let mut items = test_items();
        let mut state = PopupMenuState::default();
        state.open_at(Point::new(20.0, 160.0), MenuAnchorKind::Context);
        let viewport = Size::new(400.0, 240.0);

        state.handle_event(
            &mut items,
            &Event::KeyDown {
                key: Key::ArrowDown,
                modifiers: Modifiers::default(),
            },
            viewport,
        );
        let (_, response) = state.handle_event(
            &mut items,
            &Event::KeyDown {
                key: Key::Enter,
                modifiers: Modifiers::default(),
            },
            viewport,
        );
        assert_eq!(response, MenuResponse::Action("open".to_string()));
    }

    #[test]
    fn model_and_style_include_icons_and_shadow() {
        let items = test_items();
        let MenuEntry::Item(item) = &items[0] else {
            panic!("first row should be an item");
        };
        assert_eq!(item.icon, Some('\u{f07c}'));
        assert!(item.shortcut.is_some());
        assert!(MenuStyle::default().shadow_alpha > 0.0);
    }
}
