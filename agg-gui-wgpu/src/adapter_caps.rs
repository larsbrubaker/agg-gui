//! The adapter a [`WgpuGfxCtx`]'s device came from, and what it reports that
//! a `wgpu::Device` cannot: downlevel capabilities (independent blend per
//! colour target, for one, which WebGL2 lacks) and per-format texture
//! features. A shell records it with [`WgpuGfxCtx::set_adapter`] when it
//! builds the context; `end_frame` hands both to every custom render pass
//! through [`crate::WgpuCustomRenderCtx`], so a 3-D view chooses its passes
//! from what the adapter says instead of probing the device with test
//! pipelines.
//!
//! Every shell in this workspace records it: agg-gui-shell's painter (native
//! windows), agg-gui-web-shell (WebGPU and WebGL2 canvases) and
//! the headless frame (`headless.rs`).

use crate::WgpuGfxCtx;

/// The adapter and its downlevel capabilities, read once when recorded so a
/// frame's custom renders borrow them without asking the adapter again.
pub(crate) struct AdapterCaps {
    pub(crate) adapter: wgpu::Adapter,
    pub(crate) downlevel: wgpu::DownlevelCapabilities,
}

impl AdapterCaps {
    pub(crate) fn of(adapter: &wgpu::Adapter) -> Self {
        Self {
            adapter: adapter.clone(),
            downlevel: adapter.get_downlevel_capabilities(),
        }
    }
}

impl WgpuGfxCtx {
    /// Record the adapter this context's device was requested from. Custom
    /// render passes then see it, and its downlevel capabilities, in
    /// [`crate::WgpuCustomRenderCtx::adapter`] and
    /// [`crate::WgpuCustomRenderCtx::downlevel`]. Call it again with the new
    /// adapter when a device-loss rebuild keeps the context.
    pub fn set_adapter(&mut self, adapter: &wgpu::Adapter) {
        self.adapter_caps = Some(AdapterCaps::of(adapter));
    }

    /// [`Self::set_adapter`] in builder form, for the line that makes the
    /// context.
    pub fn with_adapter(mut self, adapter: &wgpu::Adapter) -> Self {
        self.set_adapter(adapter);
        self
    }

    /// The adapter recorded with [`Self::set_adapter`]; `None` when the
    /// context's creator recorded none.
    pub fn adapter(&self) -> Option<&wgpu::Adapter> {
        self.adapter_caps.as_ref().map(|caps| &caps.adapter)
    }

    /// The recorded adapter's downlevel capabilities; `None` when no adapter
    /// was recorded.
    pub fn downlevel_capabilities(&self) -> Option<&wgpu::DownlevelCapabilities> {
        self.adapter_caps.as_ref().map(|caps| &caps.downlevel)
    }
}
