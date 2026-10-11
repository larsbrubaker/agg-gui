//! The direction tests of agg-sharp `MenuBarWidgetTests`
//! (Tests/Agg.Tests/Agg.UI/MenuBarWidgetTests.cs):
//! `AMenuWithNoRoomBelowItsTitleOpensUpward` and
//! `AMenuWithRoomBelowItsTitleOpensDownward`.
//!
//! C#'s `MenuBarHarness` docks a `MenuBarWidget` at the top (or, with
//! `VAnchor.Bottom`, the bottom) of a window and reads screen bounds. Here a
//! horizontal [`MenuBar`] sits at the top or bottom of a 400 x 300 window;
//! its overlay is painted once through a ctx positioned at the bar, as the
//! frame does, which is how the bar learns where it sits in the window. Its
//! popup layout is bar-local, so bounds are moved by the bar's origin to
//! compare them in window space.

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, Modifiers, MouseButton};
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::widget::Widget;

use super::super::geometry::BAR_H;
use super::super::model::MenuItem;
use super::{MenuBar, TopMenu};

const WINDOW: Size = Size {
    width: 400.0,
    height: 300.0,
};

fn test_font() -> Arc<Font> {
    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

struct Harness {
    bar: MenuBar,
    /// The bar's (0, 0) in the window.
    origin: Point,
}

impl Harness {
    fn show(at_bottom: bool) -> Self {
        crate::input_profile::set_input_profile(crate::input_profile::InputProfile::Desktop);
        crate::touch_state::clear_last_touch_event_for_testing();
        crate::ux_scale::set_ux_scale(1.0);
        crate::device_scale::set_device_scale(1.0);
        crate::widget::set_current_viewport(WINDOW);
        let mut bar = MenuBar::new(
            test_font(),
            vec![TopMenu::new(
                "File",
                vec![
                    MenuItem::action("Open", "open").into(),
                    MenuItem::action("New Menu Item", "new").into(),
                    MenuItem::action("Save", "save").into(),
                ],
            )],
            |_| {},
        );
        let size = bar.layout(Size::new(WINDOW.width, BAR_H));
        bar.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
        let origin = if at_bottom {
            Point::ORIGIN
        } else {
            Point::new(0.0, WINDOW.height - size.height)
        };
        Self { bar, origin }
    }

    fn title_local(&self) -> Rect {
        self.bar.children()[0].bounds()
    }

    fn title(&self) -> Rect {
        self.to_window(self.title_local())
    }

    fn to_window(&self, r: Rect) -> Rect {
        Rect::new(r.x + self.origin.x, r.y + self.origin.y, r.width, r.height)
    }

    /// Press the File title, then paint a frame of the bar's overlay.
    fn press_title(&mut self) {
        let t = self.title_local();
        let centre = Point::new(t.x + t.width * 0.5, t.y + t.height * 0.5);
        self.bar.on_event(&Event::MouseDown {
            pos: centre,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
        let mut fb = Framebuffer::new(WINDOW.width as u32, WINDOW.height as u32);
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        ctx.translate(self.origin.x, self.origin.y);
        self.bar.paint_global_overlay(&mut ctx);
    }

    fn popup_local(&self) -> Rect {
        self.bar.popup.state.layouts(&self.bar.popup.items, WINDOW)[0].rect
    }

    fn popup(&self) -> Rect {
        self.to_window(self.popup_local())
    }
}

#[test]
fn a_menu_with_no_room_below_its_title_opens_upward() {
    let _guard = crate::input_profile::profile_test_lock();
    let mut harness = Harness::show(true);
    // A press rather than a click, so the drag half of the gesture is still armed
    harness.press_title();

    let popup = harness.popup();
    let title = harness.title();
    assert!(
        popup.bottom() >= title.top() - 0.001,
        "with nothing below the title the panel has to hang above it, not cover the bar"
    );
    assert!(
        popup.top() <= WINDOW.height + 0.001,
        "the flipped panel still has to be fully on screen"
    );

    // A drag up into the flipped panel highlights the row it reaches
    let row = harness
        .bar
        .popup
        .state
        .layouts(&harness.bar.popup.items, WINDOW)[0]
        .rows[1]
        .rect;
    harness.bar.on_event(&Event::MouseMove {
        pos: Point::new(row.x + 10.0, row.y + row.height * 0.5),
    });
    assert_eq!(harness.bar.popup.state.hover_path, Some(vec![1]));
}

#[test]
fn a_menu_with_room_below_its_title_opens_downward() {
    let _guard = crate::input_profile::profile_test_lock();
    let mut harness = Harness::show(false);
    harness.press_title();

    let popup = harness.popup();
    let title = harness.title();
    assert!(
        popup.top() <= title.bottom() + 0.001,
        "a title with room under it opens its menu downward"
    );
    assert!(popup.bottom() >= -0.001);
    assert_eq!(harness.popup_local().top(), 0.0, "it hangs from the bar");
}
