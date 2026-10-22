//! The app's adapter check: a closure the shell runs on the adapter the
//! browser handed out, after the adapter request and before the device
//! request, so an app whose own renderer needs more than the adapter offers
//! (blendable float targets, independent blend, a texture format) refuses to
//! start with its own plain message instead of failing later inside a
//! pipeline or texture creation. A refusal is
//! [`crate::WebShellError::AdapterRejected`] and takes the shell's fatal path
//! (console, the app's `on_fatal` hook, the panel).
//!
//! Platform-neutral so the refusal path is unit tested natively with a real
//! adapter; the wasm GPU bring-up (`web/gpu.rs`) calls [`run_adapter_check`]
//! on the first device and on every device-loss rebuild.

use std::rc::Rc;

use crate::error::WebShellError;

/// What the app's check returns: `Ok` to go on, `Err` with the message the
/// user sees (what is missing and what to do).
pub type AdapterCheckFn = dyn Fn(&wgpu::Adapter) -> Result<(), String>;

/// The app's adapter check. Set with
/// [`crate::WebShellConfig::with_adapter_check`].
#[derive(Clone)]
pub struct AdapterCheck(pub(crate) Rc<AdapterCheckFn>);

impl std::fmt::Debug for AdapterCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AdapterCheck(..)")
    }
}

/// Run `check` (when the app set one) on `adapter`: its refusal becomes
/// [`WebShellError::AdapterRejected`] carrying the app's message unchanged.
pub(crate) fn run_adapter_check(
    check: Option<&AdapterCheck>,
    adapter: &wgpu::Adapter,
) -> Result<(), WebShellError> {
    match check {
        Some(check) => (check.0)(adapter).map_err(WebShellError::AdapterRejected),
        None => Ok(()),
    }
}

// Natively only: the tests need a real adapter, from agg-gui-wgpu's headless
// device.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::config::WebShellConfig;
    use crate::fatal::fatal_message;
    use agg_gui_wgpu::HeadlessGpu;

    fn adapter() -> Option<&'static wgpu::Adapter> {
        match HeadlessGpu::shared() {
            Ok(gpu) => Some(gpu.wgpu_adapter()),
            Err(e) => {
                eprintln!("SKIP: {e}");
                None
            }
        }
    }

    #[test]
    fn no_check_lets_every_adapter_through() {
        let Some(adapter) = adapter() else { return };
        let cfg = WebShellConfig::new("c");
        assert!(cfg.adapter_check.is_none());
        assert!(run_adapter_check(cfg.adapter_check.as_ref(), adapter).is_ok());
    }

    /// The check sees the adapter the device will come from, and an adapter
    /// it accepts starts the app.
    #[test]
    fn the_check_reads_the_chosen_adapter() {
        let Some(adapter) = adapter() else { return };
        let seen = Rc::new(RefCell::new(None));
        let record = Rc::clone(&seen);
        let cfg = WebShellConfig::new("c").with_adapter_check(move |a| {
            *record.borrow_mut() = Some((a.get_info(), a.get_downlevel_capabilities()));
            Ok(())
        });
        assert!(run_adapter_check(cfg.adapter_check.as_ref(), adapter).is_ok());
        let (info, downlevel) = seen.borrow_mut().take().expect("the check ran");
        assert_eq!(info, adapter.get_info());
        assert_eq!(downlevel, adapter.get_downlevel_capabilities());
    }

    /// A refused adapter stops start-up with the app's own words: they reach
    /// the fatal panel's text (after the app name) and the `on_fatal` hook.
    #[test]
    fn a_refusal_carries_the_apps_message_to_the_fatal_path() {
        let Some(adapter) = adapter() else { return };
        const WHY: &str = "This browser's graphics can't draw transparent parts. \
                           Try Chrome or Edge.";
        let hooked = Rc::new(RefCell::new(None));
        let record = Rc::clone(&hooked);
        let cfg = WebShellConfig::new("c")
            .with_app_name("MatterCAD")
            .with_adapter_check(|_| Err(WHY.to_string()))
            .with_on_fatal(move |e| *record.borrow_mut() = Some(e.to_string()));
        let err = run_adapter_check(cfg.adapter_check.as_ref(), adapter)
            .expect_err("the check refused the adapter");
        match &err {
            WebShellError::AdapterRejected(msg) => assert_eq!(msg, WHY),
            other => panic!("expected AdapterRejected, got {other:?}"),
        }
        assert_eq!(
            fatal_message(&cfg, &err),
            format!("MatterCAD could not start: {WHY}")
        );
        (cfg.on_fatal.as_ref().expect("hook set").0)(&err);
        assert_eq!(hooked.borrow().as_deref(), Some(WHY));
    }
}
