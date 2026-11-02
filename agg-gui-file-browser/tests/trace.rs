//! The browser's trace builds and sends a line only once an app has installed a tracer that is
//! on, and the first tracer installed is the one that stays.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use agg_gui_file_browser::{set_tracer, trace, tracing};

static ON: AtomicBool = AtomicBool::new(false);
static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static BUILT: AtomicUsize = AtomicUsize::new(0);

fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

fn emit(line: &str) {
    LINES.lock().unwrap().push(line.to_string());
}

fn other(_: &str) {
    panic!("a second tracer never replaces the first");
}

fn line(text: &str) -> String {
    BUILT.fetch_add(1, Ordering::Relaxed);
    text.to_string()
}

#[test]
fn lines_are_built_and_sent_only_while_the_installed_tracer_is_on() {
    // One test owns the process-wide tracer, so the steps run in order.
    trace(|| line("before any tracer"));
    assert!(!tracing());
    set_tracer(on, emit);
    trace(|| line("installed, off"));
    assert_eq!(
        BUILT.load(Ordering::Relaxed),
        0,
        "no line is built while off"
    );
    ON.store(true, Ordering::Relaxed);
    assert!(tracing());
    set_tracer(on, other);
    trace(|| line("row press #3"));
    assert_eq!(*LINES.lock().unwrap(), vec!["row press #3".to_string()]);
    assert_eq!(BUILT.load(Ordering::Relaxed), 1);
}
