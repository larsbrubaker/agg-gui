//! Tests for `adapter_caps.rs`: a custom render pass sees the adapter its
//! device came from and that adapter's downlevel capabilities, so a 3-D view
//! can choose its passes (one pass with independent blend, or one per
//! target where the adapter lacks it) without test pipelines.
//!
//! Skipped (pass trivially) when no GPU adapter is available.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::Rect;

use crate::custom_render::{SharedCustomRenderer, WgpuCustomRender, WgpuCustomRenderCtx};
use crate::headless::{HeadlessFrame, HeadlessGpu, HEADLESS_FORMAT};
use crate::WgpuGfxCtx;

/// What a custom render pass was handed.
#[derive(Debug, PartialEq)]
struct Seen {
    adapter_info: Option<wgpu::AdapterInfo>,
    downlevel: Option<wgpu::DownlevelCapabilities>,
}

/// Records what it was handed and draws nothing.
struct Recorder {
    seen: Rc<RefCell<Option<Seen>>>,
}

impl WgpuCustomRender for Recorder {
    fn render(&mut self, ctx: WgpuCustomRenderCtx<'_>) {
        *self.seen.borrow_mut() = Some(Seen {
            adapter_info: ctx.adapter.map(|a| a.get_info()),
            downlevel: ctx.downlevel.cloned(),
        });
    }
}

fn gpu() -> Option<&'static HeadlessGpu> {
    match HeadlessGpu::shared() {
        Ok(gpu) => Some(gpu),
        Err(e) => {
            eprintln!("SKIP: {e}");
            None
        }
    }
}

/// Run one recorder pass in a frame driven by `frame` and return what it saw.
fn seen_in(frame: &mut HeadlessFrame) -> Seen {
    let seen = Rc::new(RefCell::new(None));
    let pass: SharedCustomRenderer = Rc::new(RefCell::new(Recorder {
        seen: Rc::clone(&seen),
    }));
    frame.render(Color::rgb(0.0, 0.0, 0.0), |ctx| {
        ctx.push_custom_render(pass, Rect::new(0.0, 0.0, 8.0, 8.0));
    });
    let got = seen.borrow_mut().take();
    got.expect("the custom render ran")
}

/// The headless frame (like the native and web shells) records its adapter:
/// a custom render pass gets it and its downlevel capabilities.
#[test]
fn a_custom_render_sees_the_frames_adapter_and_downlevel_flags() {
    let Some(gpu) = gpu() else { return };
    let mut frame = HeadlessFrame::new(gpu, 16, 16);
    let adapter = gpu.wgpu_adapter();
    assert_eq!(
        seen_in(&mut frame),
        Seen {
            adapter_info: Some(adapter.get_info()),
            downlevel: Some(adapter.get_downlevel_capabilities()),
        }
    );
    assert_eq!(
        frame.ctx().downlevel_capabilities(),
        Some(&adapter.get_downlevel_capabilities())
    );
}

/// A context made without an adapter says so (`None`), rather than
/// claiming capabilities nobody reported; recording one afterwards fills it.
#[test]
fn a_context_without_an_adapter_reports_none_until_one_is_set() {
    let Some(gpu) = gpu() else { return };
    let ctx = WgpuGfxCtx::new(
        Arc::clone(gpu.device()),
        Arc::clone(gpu.queue()),
        HEADLESS_FORMAT,
        16.0,
        16.0,
    );
    assert!(ctx.adapter().is_none());
    assert!(ctx.downlevel_capabilities().is_none());
    let mut ctx = ctx;
    ctx.set_adapter(gpu.wgpu_adapter());
    assert_eq!(
        ctx.adapter().map(|a| a.get_info()),
        Some(gpu.wgpu_adapter().get_info())
    );
    assert_eq!(
        ctx.downlevel_capabilities(),
        Some(&gpu.wgpu_adapter().get_downlevel_capabilities())
    );
}
