//! Tests for `animated_image.rs`: animated GIF playback in `MarkdownView`
//! over the virtual UI clock, the deferred start (no scheduled draw before
//! the image has loaded and painted) and no scheduled draw once the view
//! has left the tree. Includes the port of agg-sharp's
//! `EventSubscriptionHygieneTests.ResponsiveImageSequenceWidgetDefersAnimationStartUntilLoad`.

use std::sync::Arc;
use std::time::Duration;

use super::{ImageState, MarkdownView};
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::image_sequence::ImageSequence;
use crate::text::Font;
use crate::widget::Widget;

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/LiberationSans-Regular.ttf");
const URL: &str = "anim.gif";

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).expect("test font"))
}

/// A two-frame 8×8 GIF: red for 100 ms, then blue for 200 ms.
fn two_frame_gif() -> ImageSequence {
    let bytes = crate::image_sequence::tests::gif_bytes(
        8,
        8,
        &[([255, 0, 0, 255], 10), ([0, 0, 255, 255], 20)],
    );
    ImageSequence::load_gif(&bytes).expect("decodes")
}

fn gif_view() -> MarkdownView {
    MarkdownView::new(format!("![anim]({URL})"), font())
        .with_font_size(12.0)
        .with_padding(8.0)
        .with_image_sequence_provider(|url| (url == URL).then(two_frame_gif))
}

/// Minimal parent standing in for the tree a view is added to and removed from.
struct Host {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Host {
    fn type_name(&self) -> &'static str {
        "Host"
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
        for child in &mut self.children {
            let s = child.layout(available);
            child.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn crate::draw_ctx::DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Paint `widget` through the real traversal with fresh draw signals and
/// report whether a scheduled draw was armed.
fn paint_and_check_armed(widget: &mut dyn Widget) -> bool {
    let mut fb = crate::framebuffer::Framebuffer::new(200, 200);
    let mut ctx = crate::gfx_ctx::GfxCtx::new(&mut fb);
    crate::animation::clear_draw_request();
    crate::widget::paint_subtree(widget, &mut ctx);
    let armed = crate::animation::peek_next_draw_deadline().is_some();
    crate::animation::clear_draw_request();
    armed
}

fn host_with(view: MarkdownView) -> Host {
    let mut host = Host {
        bounds: Rect::new(0.0, 0.0, 200.0, 200.0),
        children: vec![Box::new(view)],
    };
    host.layout(Size::new(200.0, 200.0));
    host
}

fn view_of(host: &Host) -> &MarkdownView {
    host.children[0]
        .as_any()
        .expect("as_any")
        .downcast_ref::<MarkdownView>()
        .expect("markdown view")
}

#[test]
fn rust_only_two_frame_gif_advances_frames_over_virtual_time() {
    let _clock = crate::clock::scoped_virtual(None);
    crate::widget::set_current_viewport(Size::new(200.0, 200.0));
    let mut host = host_with(gif_view());

    assert!(paint_and_check_armed(&mut host));
    assert_eq!(view_of(&host).image_frame_index(URL), Some(0));
    assert!(!view_of(&host).needs_draw(), "frame 0 is still current");

    crate::clock::advance(Duration::from_millis(60));
    assert!(!view_of(&host).needs_draw(), "60 ms in: still frame 0");

    crate::clock::advance(Duration::from_millis(40));
    assert!(view_of(&host).needs_draw(), "100 ms in: frame 1 is due");
    paint_and_check_armed(&mut host);
    assert_eq!(view_of(&host).image_frame_index(URL), Some(1));

    // C# wraps only once the time is *past* the length (`currentTime >
    // Time`), so exactly 300 ms still shows the last frame.
    crate::clock::advance(Duration::from_millis(200));
    assert!(!view_of(&host).needs_draw(), "300 ms in: still frame 1");
    crate::clock::advance(Duration::from_millis(10));
    assert!(view_of(&host).needs_draw(), "310 ms in: wrapped to frame 0");
    paint_and_check_armed(&mut host);
    assert_eq!(view_of(&host).image_frame_index(URL), Some(0));
}

#[test]
fn rust_only_scheduled_draw_targets_the_next_frame() {
    let _clock = crate::clock::scoped_virtual(None);
    crate::widget::set_current_viewport(Size::new(200.0, 200.0));
    let mut host = host_with(gif_view());
    let start = crate::clock::now();

    let mut fb = crate::framebuffer::Framebuffer::new(200, 200);
    let mut ctx = crate::gfx_ctx::GfxCtx::new(&mut fb);
    crate::animation::clear_draw_request();
    crate::widget::paint_subtree(&mut host, &mut ctx);
    let deadline = crate::animation::peek_next_draw_deadline().expect("armed");
    crate::animation::clear_draw_request();
    assert_eq!(deadline - start, Duration::from_millis(100));
    assert_eq!(view_of(&host).next_draw_deadline(), Some(deadline));
}

#[test]
fn rust_only_no_redraw_scheduled_before_load_or_after_removal() {
    let _clock = crate::clock::scoped_virtual(None);
    crate::widget::set_current_viewport(Size::new(200.0, 200.0));
    // A remote GIF still loading: nothing to animate yet.
    let mut view = MarkdownView::new("![anim](https://example.com/anim.gif)", font())
        .with_font_size(12.0)
        .with_padding(8.0);
    view.layout_markdown(Size::new(200.0, 200.0));
    let state = Arc::clone(&view.image_cache[0].state);
    *state.lock().expect("image state") = ImageState::Loading;
    let mut host = host_with(view);

    assert!(
        !paint_and_check_armed(&mut host),
        "no draw scheduled before load"
    );
    assert_eq!(view_of(&host).next_draw_deadline(), None);
    assert!(!view_of(&host).is_image_animating("https://example.com/anim.gif"));

    // The fetch completes (what `image_loader` stores for an animated GIF).
    let pixels = super::animated_image::pixels_of_sequence(two_frame_gif()).expect("frames");
    assert!(pixels.sequence.is_some());
    *state.lock().expect("image state") = ImageState::Ready {
        image: pixels,
        seen: false,
    };
    host.layout(Size::new(200.0, 200.0));
    assert!(
        paint_and_check_armed(&mut host),
        "loaded and painted: animating"
    );
    assert!(view_of(&host).is_image_animating("https://example.com/anim.gif"));

    // Removed from the tree: the host's paints schedule nothing more.
    let removed = host.children.remove(0);
    crate::clock::advance(Duration::from_millis(100));
    assert!(
        !paint_and_check_armed(&mut host),
        "no draw scheduled after removal"
    );
    assert!(!host.needs_draw());
    assert_eq!(host.next_draw_deadline(), None);
    drop(removed);
}

/// agg-sharp `EventSubscriptionHygieneTests.ResponsiveImageSequenceWidgetDefersAnimationStartUntilLoad`.
/// C# counts `UiThread` intervals: none on construction, one after
/// `OnLoad`, none after `Close`. agg-gui has no interval registry; the
/// equivalent is the scheduled draw an image arms while painted. "Load" is
/// the first paint (C#'s `ImageLinkSimpleX` also starts its load from
/// `OnDraw`) and "close" is removal from the tree. C# builds the widget
/// from a one-frame `ImageSequence(new ImageBuffer(8, 8))`; a one-frame
/// image here draws as a still and never ticks (nothing would change), so
/// the test uses a two-frame 8×8 sequence.
#[test]
fn responsive_image_sequence_widget_defers_animation_start_until_load() {
    let _clock = crate::clock::scoped_virtual(None);
    crate::widget::set_current_viewport(Size::new(200.0, 200.0));
    crate::animation::clear_draw_request();

    let mut host = host_with(gif_view());
    // Constructed and laid out, not loaded: no interval registered.
    assert_eq!(crate::animation::peek_next_draw_deadline(), None);
    assert_eq!(host.next_draw_deadline(), None);

    // OnLoad (first paint): one scheduled draw.
    assert!(paint_and_check_armed(&mut host));
    assert!(view_of(&host).is_image_animating(URL));
    assert!(host.next_draw_deadline().is_some());

    // Close (removed): back to none.
    host.children.clear();
    assert!(!paint_and_check_armed(&mut host));
    assert_eq!(host.next_draw_deadline(), None);
}

#[test]
fn rust_only_static_image_never_schedules_draws() {
    let _clock = crate::clock::scoped_virtual(None);
    crate::widget::set_current_viewport(Size::new(200.0, 200.0));
    let view = MarkdownView::new("![still](still.png)", font())
        .with_font_size(12.0)
        .with_padding(8.0)
        .with_image_provider(|_| Some((vec![255; 8 * 8 * 4], 8, 8)));
    let mut host = host_with(view);
    assert!(!paint_and_check_armed(&mut host));
    assert_eq!(host.next_draw_deadline(), None);
    assert_eq!(view_of(&host).image_frame_index("still.png"), None);
}
