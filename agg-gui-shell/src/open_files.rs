//! Files the OS asks the running app to open (macOS Finder double-click, Open
//! With, a file dropped on the Dock icon), and the queue that carries them to
//! [`crate::ShellHost::on_open_files`].
//!
//! The OS delivers them on its own schedule: a Finder launch hands the paths
//! over while AppKit is still starting, before the host exists. They are
//! therefore pushed into a process-wide queue that the shell drains on each
//! idle iteration of the loop (`about_to_wait`), which only runs once the
//! window, the app and the host are all built: early paths wait, later ones are
//! delivered on the next iteration.
//!
//! Only macOS has a source ([`macos`]); Windows and Linux get their files as
//! process arguments, which the app reads itself, so on those the queue stays
//! empty.

use std::path::PathBuf;
use std::sync::Mutex;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::install;

/// No OS-delivered open requests exist off macOS.
#[cfg(not(target_os = "macos"))]
pub(crate) fn install() {}

/// Paths waiting for the host, oldest first.
pub(crate) struct OpenFileQueue {
    paths: Mutex<Vec<PathBuf>>,
}

impl OpenFileQueue {
    pub(crate) const fn new() -> Self {
        Self {
            paths: Mutex::new(Vec::new()),
        }
    }

    /// Append `paths`, keeping the order the OS gave them in.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn push(&self, paths: impl IntoIterator<Item = PathBuf>) {
        // A poisoned lock only means another thread panicked mid-push; the
        // list itself is still a valid Vec.
        let mut queue = self.paths.lock().unwrap_or_else(|e| e.into_inner());
        queue.extend(paths);
    }

    /// Everything queued so far, leaving the queue empty.
    pub(crate) fn take(&self) -> Vec<PathBuf> {
        let mut queue = self.paths.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *queue)
    }
}

static QUEUE: OpenFileQueue = OpenFileQueue::new();

/// Queue paths the OS wants opened.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn push_open_files(paths: Vec<PathBuf>) {
    QUEUE.push(paths);
}

/// Take the queued paths, leaving the queue empty.
pub(crate) fn take_open_files() -> Vec<PathBuf> {
    QUEUE.take()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    #[test]
    fn paths_that_arrive_before_the_host_are_held_until_taken() {
        let queue = OpenFileQueue::new();
        // Finder's launch-time delivery: two batches before anything drains.
        queue.push(vec![p("/a/one.mcx")]);
        queue.push(vec![p("/a/two.stl"), p("/a/three.3mf")]);
        assert_eq!(
            queue.take(),
            vec![p("/a/one.mcx"), p("/a/two.stl"), p("/a/three.3mf")]
        );
    }

    #[test]
    fn taking_empties_the_queue_so_a_path_is_delivered_once() {
        let queue = OpenFileQueue::new();
        queue.push(vec![p("/a/one.mcx")]);
        assert_eq!(queue.take().len(), 1);
        assert!(queue.take().is_empty());
        // A later request (Open With while running) is a fresh batch.
        queue.push(vec![p("/b/later.mcx")]);
        assert_eq!(queue.take(), vec![p("/b/later.mcx")]);
    }

    #[test]
    fn an_empty_queue_yields_nothing() {
        assert!(OpenFileQueue::new().take().is_empty());
    }

    #[test]
    fn pushes_from_another_thread_are_seen() {
        // AppKit calls the handler on the main thread, but the queue is a
        // static and must be safe from any.
        let queue = std::sync::Arc::new(OpenFileQueue::new());
        let q = std::sync::Arc::clone(&queue);
        std::thread::spawn(move || q.push(vec![p("/t/x.mcx")]))
            .join()
            .unwrap();
        assert_eq!(queue.take(), vec![p("/t/x.mcx")]);
    }
}
