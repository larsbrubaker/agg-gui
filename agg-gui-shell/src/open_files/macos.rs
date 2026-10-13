//! The AppKit half of OS-delivered open requests: makes `NSApp`'s delegate
//! answer `application:openFiles:`, which AppKit sends for a Finder
//! double-click, Open With, a drop on the Dock icon, `open file` while the app
//! runs, and a Finder launch (before `applicationDidFinishLaunching:`).
//!
//! winit 0.30's delegate (`WinitApplicationDelegate`) implements only the
//! launch and terminate notifications, and its class is private, so the method
//! is added to that class at run time (`class_addMethod`) once `EventLoop::new`
//! has installed the delegate, then the delegate is set again because AppKit
//! caches which optional delegate methods exist when `setDelegate:` runs.
//!
//! Why not an `NSAppleEventManager` handler for `kAEOpenDocuments` instead:
//! AppKit installs its own handler for that event while it finishes launching,
//! and that handler is what turns the event into `application:openFiles:`,
//! replies to the sender and applies the launch-time ordering. A handler of
//! ours registered earlier would be replaced, and one registered later would
//! have to repeat all of that. Answering the delegate method rides the same
//! path every Cocoa app uses and needs no ordering against AppKit's startup.

use std::path::PathBuf;

use objc2::ffi::class_addMethod;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{msg_send, sel, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSApplicationDelegateReply};
use objc2_foundation::{NSArray, NSString};

/// `-application:openFiles:` on the delegate. Queues the paths for the shell
/// loop and tells AppKit the open succeeded (the method must reply, or the
/// sender, Finder for one, reports a failure). `app` is the `NSApplication`.
extern "C-unwind" fn application_open_files(
    _this: &AnyObject,
    _cmd: Sel,
    app: &AnyObject,
    files: &NSArray<NSString>,
) {
    let paths: Vec<PathBuf> = files.iter().map(|f| PathBuf::from(f.to_string())).collect();
    super::push_open_files(paths);
    let () = unsafe { msg_send![app, replyToOpenOrPrint: NSApplicationDelegateReply::Success] };
}

/// Give `class` an `application:openFiles:` unless it already has one.
/// Returns whether the class answers the selector afterwards.
fn add_open_files_method(class: &AnyClass) -> bool {
    let selector = sel!(application:openFiles:);
    if class.responds_to(selector) {
        return true;
    }
    // SAFETY: `application_open_files` is an `extern "C-unwind"` function
    // taking (self, _cmd, id, NSArray*) and returning nothing, which is
    // exactly the type encoding given ("v@:@@"); the cast only erases the
    // argument list, as `Imp` requires. `class_addMethod` fails (returns NO)
    // rather than replacing a method the class itself already has.
    let added = unsafe {
        let imp: Imp = std::mem::transmute(
            application_open_files
                as extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, &NSArray<NSString>),
        );
        class_addMethod(
            class as *const _ as *mut _,
            selector,
            imp,
            c"v@:@@".as_ptr(),
        )
    };
    added.as_bool()
}

/// Make the app delegate answer `application:openFiles:`. Call after
/// `EventLoop::new()` (which installs winit's delegate) and before the loop
/// runs, on the main thread. A no-op, logged, when no delegate is installed.
pub(crate) fn install() {
    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("agg-gui-shell: open-file events need the main thread; not installed");
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let Some(delegate) = app.delegate() else {
        log::warn!("agg-gui-shell: NSApp has no delegate; open-file events not installed");
        return;
    };
    let object: &AnyObject = delegate.as_ref();
    if !add_open_files_method(object.class()) {
        log::warn!("agg-gui-shell: could not add application:openFiles: to the app delegate");
        return;
    }
    // AppKit decides at `setDelegate:` time whether the delegate implements
    // the optional methods; set the same delegate again so it sees this one.
    app.setDelegate(Some(&delegate));
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use objc2::rc::Retained;
    use objc2::runtime::{ClassBuilder, NSObject};
    use objc2::{msg_send, ClassType};

    use super::*;

    /// What the stand-in `NSApplication` was told; `usize::MAX` = nothing.
    static REPLY: AtomicUsize = AtomicUsize::new(usize::MAX);

    extern "C-unwind" fn record_reply(_this: &NSObject, _cmd: Sel, reply: usize) {
        REPLY.store(reply, Ordering::SeqCst);
    }

    fn make_class(name: &std::ffi::CStr) -> ClassBuilder {
        ClassBuilder::new(name, NSObject::class()).expect("class name is unique")
    }

    #[test]
    fn the_added_method_queues_the_paths_and_replies_success() {
        // A delegate-like class that lacks the method, and an application
        // stand-in that records `replyToOpenOrPrint:` (the real NSApplication
        // is main-thread only, which test threads are not).
        let delegate_class = make_class(c"AggShellTestDelegate").register();
        let mut app_builder = make_class(c"AggShellTestApplication");
        unsafe {
            app_builder.add_method(
                sel!(replyToOpenOrPrint:),
                record_reply as extern "C-unwind" fn(_, _, _),
            );
        }
        let app_class = app_builder.register();

        assert!(!delegate_class.responds_to(sel!(application:openFiles:)));
        assert!(add_open_files_method(delegate_class));
        assert!(delegate_class.responds_to(sel!(application:openFiles:)));
        // Adding again leaves the method that is there.
        assert!(add_open_files_method(delegate_class));

        let files = NSArray::from_retained_slice(&[
            NSString::from_str("/Users/me/a part.mcx"),
            NSString::from_str("/Users/me/b.stl"),
        ]);
        let delegate: Retained<AnyObject> = unsafe { msg_send![delegate_class, new] };
        let app: Retained<AnyObject> = unsafe { msg_send![app_class, new] };
        super::super::take_open_files();
        let () = unsafe { msg_send![&*delegate, application: &*app, openFiles: &*files] };

        assert_eq!(
            super::super::take_open_files(),
            vec![
                PathBuf::from("/Users/me/a part.mcx"),
                PathBuf::from("/Users/me/b.stl")
            ]
        );
        assert_eq!(
            REPLY.load(Ordering::SeqCst),
            NSApplicationDelegateReply::Success.0
        );
    }
}
