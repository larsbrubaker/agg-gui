//! A finger driving the demo's "Demos" menu on a phone-width window: every
//! row with a submenu stays reachable while that submenu is open, so the
//! user can always get back to the top-level list.
//!
//! Reported by Lars on an Android phone (Chrome, ~412 CSS px wide at the
//! Android profile's 1.15 UX scale, so ~358 logical px).  At that width a
//! submenu fits on neither side of its parent, so `geometry.rs`
//! `stack_layout_placed` hangs it from the row that opened it, over the rows
//! below — as a phone's own cascading menus do (iOS shows a submenu over its
//! parent with the opening row as a header that collapses it).  That only
//! works if the opening row stays visible and a tap on it closes the
//! submenu:
//!
//! - a tap on the open row used to re-open it (`state.rs`
//!   `handle_left_down`), so nothing closed it but a tap outside the menus;
//! - a submenu too tall for the room below its row was clamped back up the
//!   window (`overflow.rs` `place_vertical`), over the row itself.
//!
//! Every open panel also stays inside the window, scroll bar strip included:
//! a fixed-width popup grown for touch (`geometry::metrics_for`) was about
//! 357 logical px plus its 4 px margin in a 358 px window, so its right edge,
//! chevrons and shortcuts ran off the screen.  A label too long for the
//! narrowed row ends in an ellipsis before the shortcut / chevron column.
//!
//! The harness is `tests_touch_tap.rs`'s: taps as the touch shells deliver
//! them, through a production [`MenuBar`], whose popup runs bar-local with
//! its root origin known (as `paint_global_overlay` sets it every frame).

use std::sync::Arc;

use crate::event::{Event, Modifiers, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::input_profile::InputProfile;
use crate::text::Font;
use crate::widget::Widget;

use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;

use super::super::fit_width::SHORTCUT_GAP;
use super::super::geometry::{menu_bar_height, PopupLayout, MARGIN};
use super::super::model::{MenuEntry, MenuItem};
use super::super::style::MenuStyle;
use super::{MenuBar, TopMenu};

/// The demo's groups and how many demos each holds (`demo-ui/src/specs.rs`).
const GROUPS: [(&str, usize); 7] = [
    ("Widgets", 13),
    ("Layout", 8),
    ("Graphics", 9),
    ("Interaction", 4),
    ("Tests", 7),
    ("Window Resize Test", 6),
    ("Tools", 1),
];

fn test_font() -> Arc<Font> {
    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn demos_menu() -> Vec<MenuEntry> {
    GROUPS
        .iter()
        .map(|(group, count)| {
            let demos = (0..*count)
                .map(|i| {
                    MenuItem::action(format!("{group} demo {i}"), format!("{group}.{i}")).into()
                })
                .collect();
            MenuItem::submenu(*group, demos).icon('\u{F009}').into()
        })
        .collect()
}

/// Whether `a` and `b` share any area; panels that only touch (to within
/// rounding of the row heights summed to place them) do not.
fn overlaps(a: Rect, b: Rect) -> bool {
    const EPS: f64 = 1e-6;
    a.x < b.x + b.width - EPS
        && a.x + a.width > b.x + EPS
        && a.y < b.y + b.height - EPS
        && a.y + a.height > b.y + EPS
}

struct Phone {
    bar: MenuBar,
    viewport: Size,
}

impl Phone {
    /// The demo's menu bar across the top of a phone window `css_h` CSS px
    /// tall, under the Android profile and its recommended UX scale.
    fn show(css_h: f64) -> Self {
        Self::show_with(css_h, demos_menu())
    }

    /// [`Self::show`] with `items` as the Demos menu.
    fn show_with(css_h: f64, items: Vec<MenuEntry>) -> Self {
        let profile = InputProfile::MobileAndroid;
        crate::input_profile::set_input_profile(profile);
        crate::touch_state::clear_last_touch_event_for_testing();
        let ux = profile.recommended_ux_scale();
        crate::ux_scale::set_ux_scale(ux);
        let viewport = Size::new((412.0 / ux).floor(), (css_h / ux).floor());
        crate::widget::set_current_viewport(viewport);
        let mut bar = MenuBar::new(test_font(), vec![TopMenu::new("Demos", items)], |_| {})
            .with_font_size(13.0);
        let bar_h = menu_bar_height();
        bar.layout(Size::new(viewport.width, bar_h));
        bar.set_bounds(Rect::new(
            0.0,
            viewport.height - bar_h,
            viewport.width,
            bar_h,
        ));
        bar.popup
            .state
            .set_space_origin(Some(Point::new(0.0, viewport.height - bar_h)));
        Self { bar, viewport }
    }

    fn layouts(&self) -> Vec<PopupLayout> {
        if !self.bar.popup.is_open() {
            return Vec::new();
        }
        self.bar
            .popup
            .state
            .layouts(&self.bar.popup.items, self.viewport)
    }

    /// Row `row` of the top-level Demos panel, bar-local.
    fn group_row(&self, row: usize) -> Rect {
        self.layouts()[0].rows[row].rect
    }

    /// A finger down and up on one spot, as the touch shells deliver it.
    fn tap(&mut self, pos: Point) {
        crate::touch_state::note_touch_event();
        self.bar.on_event(&Event::MouseMove { pos });
        for down in [true, false] {
            crate::touch_state::note_touch_event();
            let (button, modifiers) = (MouseButton::Left, Modifiers::default());
            self.bar.on_event(&if down {
                Event::MouseDown {
                    pos,
                    button,
                    modifiers,
                }
            } else {
                Event::MouseUp {
                    pos,
                    button,
                    modifiers,
                }
            });
        }
    }

    fn tap_rect(&mut self, r: Rect) {
        self.tap(Point::new(r.x + r.width * 0.5, r.y + r.height * 0.5));
    }

    fn open_demos(&mut self) {
        let title = self.bar.children()[0].bounds();
        self.tap(Point::new(title.x + title.width * 0.5, title.height * 0.5));
        assert_eq!(self.bar.open_index, Some(0), "the Demos menu is open");
    }

    /// Assert every open panel lies inside the window less its margin.
    fn assert_panels_fit(&self, what: &str) {
        let margin = MARGIN;
        for (level, layout) in self.layouts().iter().enumerate() {
            let r = layout.rect;
            assert!(
                r.x >= margin - 1e-6 && r.x + r.width <= self.viewport.width - margin + 1e-6,
                "{what}: panel {level} {r:?} (scroll {:?}) must fit the {} px window \
                 less its {margin} px margins",
                layout.scroll.map(|s| s.bar),
                self.viewport.width
            );
        }
    }

    /// Paint the open popup chain as the bar's overlay pass does, so its
    /// row labels lay out against the rows.
    fn paint(&mut self) {
        let mut fb = Framebuffer::new(self.viewport.width as u32, self.viewport.height as u32);
        let mut ctx = GfxCtx::new(&mut fb);
        self.bar
            .popup
            .paint(&mut ctx, test_font(), 13.0, self.viewport);
    }
}

fn finish() {
    crate::touch_state::clear_last_touch_event_for_testing();
    crate::ux_scale::set_ux_scale(1.0);
    crate::input_profile::set_input_profile(InputProfile::Desktop);
}

/// Open each group in turn: its submenu leaves the group's row uncovered, a
/// tap on that row closes it again, and the next group can then be opened.
fn every_group_opens_and_closes_from_its_own_row(css_h: f64) {
    let mut phone = Phone::show(css_h);
    phone.open_demos();
    for (row_idx, (group, _)) in GROUPS.iter().enumerate() {
        let row = phone.group_row(row_idx);
        phone.tap_rect(row);
        assert_eq!(
            phone.bar.popup.state.open_path,
            vec![row_idx],
            "a tap on {group} opens its submenu"
        );
        let layouts = phone.layouts();
        assert_eq!(layouts.len(), 2);
        let (parent, sub) = (layouts[0].rect, layouts[1].rect);
        assert!(
            parent.x + parent.width + sub.width > phone.viewport.width && parent.x < sub.width,
            "the fixture is narrow: {group}'s submenu fits on neither side"
        );
        // Re-read the row: the parent panel does not move.
        let row = phone.group_row(row_idx);
        assert!(
            !overlaps(sub, row),
            "{group}'s submenu {sub:?} must not cover its row {row:?} \
             (window {:?})",
            phone.viewport
        );

        phone.tap_rect(row);
        assert!(
            phone.bar.popup.state.open_path.is_empty(),
            "a second tap on {group} closes its submenu"
        );
        assert_eq!(phone.layouts().len(), 1, "the top-level list is back");
        assert_eq!(phone.bar.open_index, Some(0));
    }
    finish();
}

#[test]
fn on_a_phone_each_group_row_closes_the_submenu_it_opened() {
    let _guard = crate::input_profile::profile_test_lock();
    // Portrait: every submenu fits below its row.
    every_group_opens_and_closes_from_its_own_row(760.0);
}

#[test]
fn on_a_short_phone_window_a_tall_submenu_still_leaves_its_row_uncovered() {
    let _guard = crate::input_profile::profile_test_lock();
    // A short window (an on-screen keyboard up, or a short phone): the
    // Widgets submenu is taller than the room on either side of its row, so
    // it scrolls in the larger room instead of sliding over the row.
    every_group_opens_and_closes_from_its_own_row(480.0);
}

#[test]
fn on_a_phone_a_sibling_row_above_the_open_submenu_switches_to_it() {
    let _guard = crate::input_profile::profile_test_lock();
    let mut phone = Phone::show(760.0);
    phone.open_demos();
    const TESTS: usize = 4;
    const LAYOUT: usize = 1;
    let tests_row = phone.group_row(TESTS);
    phone.tap_rect(tests_row);
    assert_eq!(phone.bar.popup.state.open_path, vec![TESTS]);

    let layout_row = phone.group_row(LAYOUT);
    assert!(!overlaps(phone.layouts()[1].rect, layout_row));
    phone.tap_rect(layout_row);
    assert_eq!(
        phone.bar.popup.state.open_path,
        vec![LAYOUT],
        "a tap on a sibling row switches to its submenu"
    );
    finish();
}

#[test]
fn on_a_phone_every_open_panel_fits_inside_the_window() {
    let _guard = crate::input_profile::profile_test_lock();
    // Portrait, and a short window where Widgets scrolls and gains a scroll
    // bar strip that must count inside the width too.
    for css_h in [760.0, 480.0] {
        let mut phone = Phone::show(css_h);
        phone.open_demos();
        phone.assert_panels_fit("the Demos menu");
        for (row_idx, (group, _)) in GROUPS.iter().enumerate() {
            let row = phone.group_row(row_idx);
            phone.tap_rect(row);
            phone.assert_panels_fit(&format!("{group} open in a {css_h} CSS px window"));
            phone.tap_rect(row);
        }
    }
    finish();
}

#[test]
fn on_a_phone_a_long_label_elides_before_its_shortcut_and_chevron() {
    let _guard = crate::input_profile::profile_test_lock();
    let items = vec![MenuItem::submenu(
        "A group whose name is far too long to fit across a phone",
        vec![
            MenuItem::action("An action with a long name that will not fit either", "a")
                .shortcut("Ctrl+Shift+K")
                .into(),
        ],
    )
    .into()];
    let mut phone = Phone::show_with(760.0, items);
    phone.open_demos();
    let group_row = phone.group_row(0);
    phone.tap_rect(group_row);
    phone.assert_panels_fit("the long-label menus");
    phone.paint();

    let style = MenuStyle::default();
    let layouts = phone.layouts();
    let labels = &phone.bar.popup.labels;

    // The submenu row: its label ends before the chevron column.
    let row = layouts[0].rows[0].rect;
    let label = labels.row_label(0, 0).expect("group label");
    assert!(
        label.ellipsis_active(),
        "the long group name is elided ({})",
        label.shown_text()
    );
    assert!(
        row.x + style.label_x + label.bounds().width
            <= row.x + row.width - style.shortcut_right + 1e-6,
        "the group label {:?} must end before the chevron column of {row:?}",
        label.bounds()
    );

    // The action row: its label ends a gap before the whole shortcut.
    let row = layouts[1].rows[0].rect;
    let label = labels.row_label(1, 0).expect("action label");
    let shortcut = labels.row_shortcut(1, 0).expect("shortcut label");
    let shortcut_left = row.x + row.width - style.shortcut_right - shortcut.bounds().width;
    assert!(
        shortcut.shown_text() == shortcut.text_str() && !shortcut.ellipsis_active(),
        "the shortcut is shown whole"
    );
    assert!(label.ellipsis_active(), "the long action name is elided");
    assert!(
        row.x + style.label_x + label.bounds().width + SHORTCUT_GAP <= shortcut_left + 1e-6,
        "the action label {:?} must end {SHORTCUT_GAP} px before its shortcut at {shortcut_left}",
        label.bounds()
    );
    finish();
}
