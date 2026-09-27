//! The fatal-error path: the message shown to the user, whether the panel
//! replaces the canvas, the panel's stylesheet, and the app's `on_fatal` hook.
//!
//! Platform-neutral so the choices are unit tested natively. The DOM side
//! (console.error, building and inserting the panel) lives in the crate's
//! `web` module, which calls [`fatal_message`] / [`shows_panel`] with the
//! live [`crate::WebShellConfig`].

use std::rc::Rc;

use crate::config::WebShellConfig;
use crate::error::WebShellError;

/// CSS class the built-in panel carries; its stylesheet is [`PANEL_CSS`].
pub(crate) const PANEL_CLASS: &str = "agg-gui-web-shell-fatal";

/// Stylesheet injected once for the built-in panel: readable on light pages,
/// and on dark ones through `prefers-color-scheme: dark`. Not injected when
/// the app supplies its own class
/// ([`crate::WebShellConfig::with_fatal_panel_class`]).
pub(crate) const PANEL_CSS: &str = "\
.agg-gui-web-shell-fatal{max-width:40em;margin:4em auto;padding:1.5em 2em;\
font:16px/1.5 system-ui,sans-serif;color:#333;background:#fff3f0;\
border:1px solid #e0b4a8;border-radius:8px;white-space:pre-wrap}\
@media (prefers-color-scheme: dark){.agg-gui-web-shell-fatal{color:#f3e6e3;\
background:#3a2320;border-color:#7a4a40}}";

/// Composes the user-facing text for a fatal error. Set with
/// [`crate::WebShellConfig::with_fatal_message`].
pub type FatalMessageFn = fn(&WebShellError) -> String;

/// App callback run for every fatal error — synchronous start failures, async
/// GPU/builder failures, and a device loss the shell gave up on. Set with
/// [`crate::WebShellConfig::with_on_fatal`].
#[derive(Clone)]
pub struct FatalHook(pub(crate) Rc<dyn Fn(&WebShellError)>);

impl std::fmt::Debug for FatalHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FatalHook(..)")
    }
}

/// The text the panel shows (and the console logs) for `err`: the app's
/// [`FatalMessageFn`] when set, else the error prefixed with the app name
/// when one is configured, else the bare error.
pub(crate) fn fatal_message(config: &WebShellConfig, err: &WebShellError) -> String {
    if let Some(compose) = config.fatal_message {
        return compose(err);
    }
    match &config.app_name {
        Some(name) => format!("{name} could not start: {err}"),
        None => err.to_string(),
    }
}

/// Whether `err` replaces the canvas with the panel. A second `start` call is
/// a programming error on a page whose first instance is running fine — it is
/// logged, never painted over the working app.
pub(crate) fn shows_panel(config: &WebShellConfig, err: &WebShellError) -> bool {
    config.fatal_panel && !matches!(err, WebShellError::AlreadyStarted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_error_by_default() {
        let cfg = WebShellConfig::new("c");
        let err = WebShellError::WebGpuUnavailable;
        assert_eq!(fatal_message(&cfg, &err), err.to_string());
    }

    #[test]
    fn app_name_prefixes_the_message() {
        let cfg = WebShellConfig::new("c").with_app_name("COLMAP");
        let msg = fatal_message(&cfg, &WebShellError::CanvasNotFound("x".into()));
        assert!(msg.starts_with("COLMAP could not start: "), "{msg}");
        assert!(msg.contains("#x"));
    }

    #[test]
    fn custom_message_wins() {
        fn custom(e: &WebShellError) -> String {
            format!("custom: {}", matches!(e, WebShellError::WebGpuUnavailable))
        }
        let cfg = WebShellConfig::new("c")
            .with_app_name("ignored")
            .with_fatal_message(custom);
        assert_eq!(
            fatal_message(&cfg, &WebShellError::WebGpuUnavailable),
            "custom: true"
        );
    }

    #[test]
    fn double_start_never_paints_the_panel() {
        let cfg = WebShellConfig::new("c");
        assert!(!shows_panel(&cfg, &WebShellError::AlreadyStarted));
        assert!(shows_panel(&cfg, &WebShellError::WebGpuUnavailable));
        let off = WebShellConfig::new("c").with_fatal_panel(false);
        assert!(!shows_panel(&off, &WebShellError::WebGpuUnavailable));
    }

    #[test]
    fn panel_css_has_a_dark_variant_for_its_class() {
        assert!(PANEL_CSS.contains("prefers-color-scheme: dark"));
        assert!(PANEL_CSS.contains(PANEL_CLASS));
    }

    #[test]
    fn on_fatal_hook_is_stored_and_callable() {
        use std::cell::Cell;
        thread_local!(static HIT: Cell<bool> = const { Cell::new(false) });
        let cfg = WebShellConfig::new("c").with_on_fatal(|_| HIT.with(|h| h.set(true)));
        (cfg.on_fatal.as_ref().expect("hook set").0)(&WebShellError::NoWindow);
        assert!(HIT.with(|h| h.get()));
    }
}
