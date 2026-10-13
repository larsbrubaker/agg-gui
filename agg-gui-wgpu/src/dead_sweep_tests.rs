//! A frame's draws sweep the Arc-keyed texture caches' dead entries once, not once per draw
//! (`sweep.rs`): a page of N `Label`s makes N draws through `draw_image_rgba_arc_impl`, one
//! texture each, and sweeping the whole cache on each was O(N²) per frame (165 ms of a 180 ms
//! paint at ~3,200 labels). The counts are the cache's entries, so they do not depend on timing.
//! Needs a headless GPU adapter and passes trivially without one, as `lcd_arc_cache_tests` does.

use std::sync::Arc;

use agg_gui::color::Color;

use crate::layer_text_readback_tests::{try_device, Target};
use crate::WgpuGfxCtx;

fn image() -> Arc<Vec<u8>> {
    Arc::new(vec![200u8; 4 * 4 * 4])
}

fn draw(ctx: &mut WgpuGfxCtx, data: &Arc<Vec<u8>>) {
    ctx.draw_image_rgba_arc_impl(data, 4, 4, 0.0, 0.0, 4.0, 4.0);
}

#[test]
fn the_arc_cache_sweeps_its_dead_entries_once_a_frame() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), 16, 16);
    let mut ctx = WgpuGfxCtx::new(device, queue, wgpu::TextureFormat::Rgba8Unorm, 16.0, 16.0);
    ctx.reset(16.0, 16.0);

    let (keep, gone_early, gone_late) = (image(), image(), image());
    ctx.begin_frame_cleared(target.view.clone(), Color::rgba(1.0, 1.0, 1.0, 1.0));
    draw(&mut ctx, &keep);
    draw(&mut ctx, &gone_early);
    draw(&mut ctx, &gone_late);
    assert_eq!(ctx.arc_texture_cache.len(), 3);
    ctx.end_frame();

    // The next frame's first draw sweeps what died between the frames.
    drop(gone_early);
    ctx.begin_frame_cleared(target.view.clone(), Color::rgba(1.0, 1.0, 1.0, 1.0));
    draw(&mut ctx, &keep);
    assert_eq!(
        ctx.arc_texture_cache.len(),
        2,
        "the entry whose picture is gone is swept by the frame's first draw"
    );

    // Later draws of the same frame leave the cache alone: that is the saving.
    drop(gone_late);
    for _ in 0..100 {
        draw(&mut ctx, &keep);
    }
    assert_eq!(
        ctx.arc_texture_cache.len(),
        2,
        "no further sweep runs in the frame, so the entry that died mid-frame waits"
    );
    ctx.end_frame();

    ctx.begin_frame_cleared(target.view.clone(), Color::rgba(1.0, 1.0, 1.0, 1.0));
    draw(&mut ctx, &keep);
    assert_eq!(ctx.arc_texture_cache.len(), 1, "the next frame sweeps it");
    ctx.end_frame();
}
