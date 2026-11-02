//! The browser's opt-in trace: one line per press decision, menu request or list change, for an
//! app chasing a lost or misread click. The app installs where lines go and whether its trace is
//! on ([`set_tracer`], once per process); with no tracer, or with it off, [`trace`] builds no line
//! at all, so a call costs one check.

use std::sync::OnceLock;

/// The app's tracer: whether its trace is on, and where a line goes.
struct Tracer {
    enabled: fn() -> bool,
    emit: fn(&str),
}

static TRACER: OnceLock<Tracer> = OnceLock::new();

/// Sends the browser's trace lines to `emit` while `enabled` answers true. The first install
/// wins; later ones change nothing.
pub fn set_tracer(enabled: fn() -> bool, emit: fn(&str)) {
    let _ = TRACER.set(Tracer { enabled, emit });
}

/// Whether a trace line would go anywhere now.
pub fn tracing() -> bool {
    TRACER.get().is_some_and(|tracer| (tracer.enabled)())
}

/// Traces the line `message` builds, building it only when the trace is on.
pub fn trace(message: impl FnOnce() -> String) {
    if let Some(tracer) = TRACER.get().filter(|tracer| (tracer.enabled)()) {
        (tracer.emit)(&message());
    }
}
