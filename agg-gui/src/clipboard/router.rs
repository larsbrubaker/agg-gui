//! The native clipboard's routing decision: the in-process clipboard until the
//! app opts in to the system one, then one long-lived system connection.
//!
//! [`Router`] is plain data over a [`SystemBackend`], so the decision is unit
//! tested here with a fake backend that records every call: nothing in this
//! file talks to the OS. `backend.rs` owns the process-wide instance (over
//! `arboard`) and the per-thread one unit tests use; `mod.rs` is the public
//! API that checks [`super::simulate`]'s clipboard before reaching a router.

use super::{ClipboardImage, Contents};

/// The operations the router needs from a system clipboard connection.
pub(super) trait SystemBackend {
    fn get_text(&mut self) -> Option<String>;
    fn set_text(&mut self, text: &str);
    /// Write HTML with a plain-text fallback; a clipboard that can't take
    /// HTML gets the plain text.
    fn set_rich_text(&mut self, plain_text: &str, html_text: &str);
    fn get_image(&mut self) -> Option<ClipboardImage>;
    /// Whether a picture is on the clipboard, ideally without reading it.
    fn has_image(&mut self) -> bool;
    fn set_image(&mut self, image: &ClipboardImage) -> bool;
}

/// Where clipboard reads and writes go when no simulated clipboard is
/// installed.
///
/// Until [`Router::use_system`] the router never touches the OS: it serves
/// the in-process [`Contents`], so a test binary (or any program that is not
/// a windowed app) can copy and paste without overwriting the developer's
/// real clipboard. After it, the router connects lazily through `connect`
/// and keeps that one connection: on X11 the clipboard is served by the
/// process that copied, and dropping arboard's last connection would stop
/// serving it (and can block while it hands the contents over). When a
/// connection can't be made the router keeps using the in-process contents
/// for that call and tries again on the next one.
pub(super) struct Router<S> {
    connect: fn() -> Option<S>,
    wants_system: bool,
    system: Option<S>,
    local: Contents,
}

impl<S: SystemBackend> Router<S> {
    pub(super) const fn new(connect: fn() -> Option<S>) -> Self {
        Self {
            connect,
            wants_system: false,
            system: None,
            local: Contents {
                text: None,
                image: None,
            },
        }
    }

    /// From now on, use the system clipboard (connecting on first use).
    pub(super) fn use_system(&mut self) {
        self.wants_system = true;
    }

    /// Drop the system connection (the app is shutting down); a later
    /// clipboard call connects again.
    pub(super) fn release_system(&mut self) {
        self.system = None;
    }

    /// Whether a system connection is open (tests check nothing reached the OS).
    #[cfg(test)]
    pub(super) fn is_connected(&self) -> bool {
        self.system.is_some()
    }

    /// The system connection, when the app opted in and one can be made.
    fn system(&mut self) -> Option<&mut S> {
        if !self.wants_system {
            return None;
        }
        if self.system.is_none() {
            self.system = (self.connect)();
        }
        self.system.as_mut()
    }

    pub(super) fn get_text(&mut self) -> Option<String> {
        match self.system() {
            Some(system) => system.get_text(),
            None => self.local.text.clone(),
        }
    }

    pub(super) fn set_text(&mut self, text: &str) {
        match self.system() {
            Some(system) => system.set_text(text),
            None => self.local.put_text(text),
        }
    }

    /// The in-process clipboard has no HTML flavour, so it keeps the plain
    /// text, as a system clipboard read through [`Router::get_text`] would.
    pub(super) fn set_rich_text(&mut self, plain_text: &str, html_text: &str) {
        match self.system() {
            Some(system) => system.set_rich_text(plain_text, html_text),
            None => self.local.put_text(plain_text),
        }
    }

    pub(super) fn get_image(&mut self) -> Option<ClipboardImage> {
        match self.system() {
            Some(system) => system.get_image(),
            None => self.local.image.clone(),
        }
    }

    pub(super) fn has_image(&mut self) -> bool {
        match self.system() {
            Some(system) => system.has_image(),
            None => self.local.image.is_some(),
        }
    }

    pub(super) fn set_image(&mut self, image: ClipboardImage) -> bool {
        match self.system() {
            Some(system) => system.set_image(&image),
            None => {
                self.local.put_image(image);
                true
            }
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::{Router, SystemBackend};
    use crate::clipboard::ClipboardImage;
    use std::cell::Cell;

    thread_local! {
        /// How many connections this thread's fake backends were asked for.
        static CONNECTS: Cell<usize> = const { Cell::new(0) };
        /// Whether a connection attempt succeeds.
        static CONNECT_FAILS: Cell<bool> = const { Cell::new(false) };
    }

    /// A stand-in for the OS clipboard that records what reached it.
    #[derive(Default)]
    pub(in crate::clipboard) struct FakeSystem {
        text: Option<String>,
        html: Option<String>,
        image: Option<ClipboardImage>,
        calls: usize,
    }

    impl FakeSystem {
        pub(in crate::clipboard) fn connect() -> Option<Self> {
            CONNECTS.with(|c| c.set(c.get() + 1));
            if CONNECT_FAILS.with(Cell::get) {
                None
            } else {
                Some(Self::default())
            }
        }
    }

    impl SystemBackend for FakeSystem {
        fn get_text(&mut self) -> Option<String> {
            self.calls += 1;
            self.text.clone()
        }
        fn set_text(&mut self, text: &str) {
            self.calls += 1;
            self.text = Some(text.to_string());
            self.html = None;
            self.image = None;
        }
        fn set_rich_text(&mut self, plain_text: &str, html_text: &str) {
            self.set_text(plain_text);
            self.html = Some(html_text.to_string());
        }
        fn get_image(&mut self) -> Option<ClipboardImage> {
            self.calls += 1;
            self.image.clone()
        }
        fn has_image(&mut self) -> bool {
            self.calls += 1;
            self.image.is_some()
        }
        fn set_image(&mut self, image: &ClipboardImage) -> bool {
            self.calls += 1;
            self.text = None;
            self.image = Some(image.clone());
            true
        }
    }

    fn connects() -> usize {
        CONNECTS.with(Cell::get)
    }

    fn picture() -> ClipboardImage {
        ClipboardImage::new(1, 1, vec![1, 2, 3, 4]).expect("1x1 RGBA")
    }

    #[test]
    fn before_opting_in_the_clipboard_stays_in_process() {
        let before = connects();
        let mut router = Router::new(FakeSystem::connect);
        assert_eq!(router.get_text(), None);
        router.set_text("hello");
        assert_eq!(router.get_text().as_deref(), Some("hello"));
        router.set_rich_text("plain", "<b>plain</b>");
        assert_eq!(router.get_text().as_deref(), Some("plain"));
        assert!(!router.has_image());
        assert!(router.set_image(picture()));
        assert!(router.has_image());
        assert_eq!(router.get_image(), Some(picture()));
        assert_eq!(router.get_text(), None, "a picture replaces the text");
        router.set_text("again");
        assert_eq!(router.get_image(), None, "text replaces the picture");
        assert!(!router.has_image());
        assert_eq!(connects(), before, "the OS was never asked for");
        assert!(router.system.is_none());
    }

    #[test]
    fn after_opting_in_one_system_connection_serves_every_call() {
        let before = connects();
        let mut router = Router::new(FakeSystem::connect);
        router.set_text("local");
        router.use_system();
        assert_eq!(router.get_text(), None, "the system clipboard is empty");
        router.set_text("system");
        router.set_rich_text("rich", "<i>rich</i>");
        assert!(router.set_image(picture()));
        assert!(router.has_image());
        assert_eq!(router.get_image(), Some(picture()));
        assert_eq!(connects() - before, 1, "the connection is kept, not remade");
        let system = router.system.as_ref().expect("connected");
        assert_eq!(
            system.calls, 6,
            "get, set, rich set, picture set, picture check, picture get"
        );
        assert_eq!(system.html.as_deref(), Some("<i>rich</i>"));
        assert_eq!(router.local.text.as_deref(), Some("local"));
    }

    #[test]
    fn a_failed_connection_falls_back_in_process_and_retries() {
        let before = connects();
        let mut router = Router::new(FakeSystem::connect);
        router.use_system();
        CONNECT_FAILS.with(|f| f.set(true));
        router.set_text("kept here");
        assert_eq!(router.get_text().as_deref(), Some("kept here"));
        CONNECT_FAILS.with(|f| f.set(false));
        assert_eq!(router.get_text(), None, "now served by the system");
        assert_eq!(connects() - before, 3);
    }

    #[test]
    fn releasing_drops_the_connection_and_the_next_call_reconnects() {
        let before = connects();
        let mut router = Router::new(FakeSystem::connect);
        router.use_system();
        router.set_text("one");
        router.release_system();
        assert!(router.system.is_none());
        assert_eq!(router.get_text(), None, "a fresh fake connection");
        assert_eq!(connects() - before, 2);
    }
}
