//! agg-sharp `Tests/Agg.Tests/Agg Automation Tests/PresentFailureContainmentTests.cs`:
//! a frame whose GPU submission wgpu rejects may cost that frame, never the
//! frames after it.
//!
//! C# shows a window whose widget draws cleanly but leaves a recording wgpu
//! rejects at submit (an 8x8 copy out of a 4x4 texture) and a model-view push
//! it never pops, every frame; a failed present used to skip the frame's
//! cleanup, so later frames continued the failed one and the push stack
//! overflowed. Here the same saboteur runs inside a [`HeadlessFrame`] (the
//! frame a shell runs, on a device of this binary's own): a custom render pass
//! ([`WgpuCustomRender`], the hook a 3-D viewport uses) records the same
//! invalid copy into the frame's own encoder, so the frame's own submission is
//! the one wgpu rejects, and the widget leaves a `save` with a translation
//! outstanding. The device carries [`UncapturedErrors::install`], the handler
//! the window's [`agg_gui_wgpu::Gpu`] installs — without it wgpu's default
//! handler panics on the first rejected submission and takes the app down.
//!
//! What is asserted is C#'s: the rejection was reported (so the recording
//! really was invalid), the saboteur kept drawing whole frames well past any
//! stack depth, and a clean frame afterwards draws where it should — no
//! leftover translation from the unbalanced saves — with no further errors.
//!
//! Skipped (passes trivially) when no GPU adapter is available.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::Rect;
use agg_gui_wgpu::{
    HeadlessFrame, HeadlessGpu, UncapturedErrors, WgpuCustomRender, WgpuCustomRenderCtx, WgpuGfxCtx,
};

const SIZE: u32 = 64;

/// C#'s `InvalidRecordingWidget` recording: an 8x8 copy out of a 4x4 texture,
/// which wgpu rejects when the encoder holding it is finished and submitted.
#[derive(Default)]
struct InvalidRecording {
    textures: Option<(wgpu::Texture, wgpu::Texture)>,
    recorded: Rc<Cell<u32>>,
}

impl WgpuCustomRender for InvalidRecording {
    fn render(&mut self, ctx: WgpuCustomRenderCtx<'_>) {
        let (source, destination) = self.textures.get_or_insert_with(|| {
            let texture = |w: u32, usage: wgpu::TextureUsages, label: &str| {
                ctx.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: w,
                        height: w,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: usage | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
            };
            (
                texture(4, wgpu::TextureUsages::COPY_SRC, "presentFailureSource"),
                texture(
                    8,
                    wgpu::TextureUsages::COPY_DST,
                    "presentFailureDestination",
                ),
            )
        });
        ctx.encoder.copy_texture_to_texture(
            source.as_image_copy(),
            destination.as_image_copy(),
            wgpu::Extent3d {
                width: 8,
                height: 8,
                depth_or_array_layers: 1,
            },
        );
        self.recorded.set(self.recorded.get() + 1);
    }
}

/// C#'s `InvalidRecordingWidget.OnDraw` while sabotaging: a recording wgpu
/// rejects (in the frame's own encoder), and a push that is never popped.
fn sabotage(ctx: &mut WgpuGfxCtx, recording: &Rc<RefCell<InvalidRecording>>) {
    ctx.push_custom_render(
        recording.clone(),
        Rect::new(0.0, 0.0, SIZE as f64, SIZE as f64),
    );
    ctx.save();
    ctx.translate(3.0, 3.0);
}

/// A clean frame: a red square at (16, 16)-(48, 48) on blue.
fn clean_frame(frame: &mut HeadlessFrame) -> Vec<u8> {
    frame.render(Color::rgb(0.0, 0.0, 1.0), |ctx| {
        ctx.set_fill_color(Color::rgb(1.0, 0.0, 0.0));
        ctx.begin_path();
        ctx.rect(16.0, 16.0, 32.0, 32.0);
        ctx.fill();
    });
    frame.read_rgba().expect("read back the clean frame")
}

fn px(data: &[u8], x: usize, y: usize) -> [u8; 4] {
    let i = (y * SIZE as usize + x) * 4;
    [data[i], data[i + 1], data[i + 2], data[i + 3]]
}

#[test]
fn a_present_that_fails_every_frame_still_starts_each_next_frame_whole() {
    let gpu = match HeadlessGpu::create() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: no GPU adapter ({e})");
            return;
        }
    };
    let errors = UncapturedErrors::install(gpu.device());
    let mut frame = HeadlessFrame::new(&gpu, SIZE, SIZE);

    // C#'s `MaxMatrixDepth + 8`; agg-gui's save stack has no fixed depth, so
    // the count is chosen so the leftover translations (3 px a frame) would
    // push the clean frame's square entirely off the target.
    let enough_to_trip_the_guard = 40;
    let recording = Rc::new(RefCell::new(InvalidRecording::default()));
    let recorded = recording.borrow().recorded.clone();
    let mut sabotaged_count = 0;
    for _ in 0..enough_to_trip_the_guard {
        frame.render(Color::rgb(1.0, 1.0, 1.0), |ctx| {
            sabotage(ctx, &recording);
            sabotaged_count += 1;
        });
    }
    let _ = gpu.device().poll(wgpu::PollType::wait_indefinitely());

    assert!(
        sabotaged_count >= enough_to_trip_the_guard,
        "the saboteur has to keep drawing whole frames past any stack depth"
    );
    assert_eq!(
        recorded.get(),
        enough_to_trip_the_guard,
        "every frame's own encoder carried the invalid copy"
    );
    let seen = errors.count();
    assert!(
        seen > 0,
        "the rejected submit has to be reported, or the recording was never invalid and nothing has been proved"
    );

    let clean = clean_frame(&mut frame);
    let _ = gpu.device().poll(wgpu::PollType::wait_indefinitely());
    // Read-back rows run top first; the square is centred either way.
    assert_eq!(
        px(&clean, 32, 32),
        [255, 0, 0, 255],
        "a frame after failed submissions starts whole: the square is where it was drawn"
    );
    assert_eq!(
        px(&clean, 4, 4),
        [0, 0, 255, 255],
        "and the clear covers the rest"
    );
    assert_eq!(
        errors.count(),
        seen,
        "the clean frame raised no error of its own: {:?}",
        errors.last()
    );
}
