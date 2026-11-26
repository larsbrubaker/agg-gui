//! Files AppKit asks the running app to open (feature `open-files`): makes
//! `NSApp`'s delegate answer `application:openFiles:`, which AppKit sends for
//! a Finder double-click, Open With, a drop on the Dock icon, `open file`
//! while the app runs, and a Finder launch (before
//! `applicationDidFinishLaunching:`).
//!
//! agg-gui-shell is the consumer (`open_files/macos.rs`): it decides when to
//! [`install`], logs a failure, and queues the paths the callback receives
//! for its loop. Everything that touches the Objective-C runtime is here.
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

use std::fmt;
use std::path::PathBuf;
use std::sync::Mutex;

use objc2::ffi::class_addMethod;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{msg_send, sel, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSApplicationDelegateReply};
use objc2_foundation::{NSArray, NSString};

/// Why [`install`] left the delegate as it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum InstallError {
    /// `NSApp` has no delegate yet (winit installs one in `EventLoop::new`).
    NoDelegate,
    /// The runtime refused to add `application:openFiles:` to the
    /// delegate's class.
    AddMethodFailed,
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDelegate => f.write_str("NSApp has no delegate"),
            Self::AddMethodFailed => {
                f.write_str("could not add application:openFiles: to the app delegate")
            }
        }
    }
}

impl std::error::Error for InstallError {}

/// What receives the paths: a plain `fn`, because the method body is a free
/// function with nowhere else to find it.
type OnOpen = fn(Vec<PathBuf>);

/// The callback of the latest [`install`], which the added method hands the
/// paths to.
static ON_OPEN: Mutex<Option<OnOpen>> = Mutex::new(None);

fn set_callback(on_open: OnOpen) {
    // A poisoned lock only means a thread panicked while storing a plain
    // `fn` pointer; the value is still valid.
    *ON_OPEN.lock().unwrap_or_else(|e| e.into_inner()) = Some(on_open);
}

/// `-application:openFiles:` on the delegate. Hands the paths to the callback
/// and tells AppKit the open succeeded (the method must reply, or the sender,
/// Finder for one, reports a failure). `app` is the `NSApplication`.
extern "C-unwind" fn application_open_files(
    _this: &AnyObject,
    _cmd: Sel,
    app: &AnyObject,
    files: &NSArray<NSString>,
) {
    let paths: Vec<PathBuf> = files.iter().map(|f| PathBuf::from(f.to_string())).collect();
    // Copied out, so the callback runs with the lock released.
    let on_open = *ON_OPEN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(on_open) = on_open {
        on_open(paths);
    }
    // SAFETY: AppKit sends `application:openFiles:` with the NSApplication as
    // `app`, which implements `-(void)replyToOpenOrPrint:(NSApplicationDelegateReply)`;
    // the reply is an NSUInteger-backed enum, passed by value.
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
    // exactly the type encoding given ("v@:@@"); the transmute only erases
    // the argument list, as `Imp` requires. `class_addMethod` fails (returns
    // NO) rather than replacing a method the class itself already has, and
    // the runtime serializes changes to a class.
    let added = unsafe {
        let imp: Imp = std::mem::transmute::<
            extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, &NSArray<NSString>),
            Imp,
        >(application_open_files);
        class_addMethod(
            class as *const AnyClass as *mut AnyClass,
            selector,
            imp,
            c"v@:@@".as_ptr(),
        )
    };
    added.as_bool()
}

/// Makes the app delegate answer `application:openFiles:`, handing each
/// batch of paths to `on_open` (on the main thread, in the order AppKit gives
/// them). Call after `EventLoop::new()` (which installs winit's delegate) and
/// before the loop runs.
///
/// Calling it again replaces the callback and sets the delegate again; the
/// method already present is kept. A delegate whose class has its own
/// `application:openFiles:` keeps it, and `on_open` is not called.
pub fn install(mtm: MainThreadMarker, on_open: fn(Vec<PathBuf>)) -> Result<(), InstallError> {
    let app = NSApplication::sharedApplication(mtm);
    let delegate = app.delegate().ok_or(InstallError::NoDelegate)?;
    set_callback(on_open);
    let object: &AnyObject = delegate.as_ref();
    if !add_open_files_method(object.class()) {
        return Err(InstallError::AddMethodFailed);
    }
    // AppKit decides at `setDelegate:` time whether the delegate implements
    // the optional methods; set the same delegate again so it sees this one.
    app.setDelegate(Some(&delegate));
    Ok(())
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

    /// What the callback received, batch by batch.
    static OPENED: Mutex<Vec<Vec<PathBuf>>> = Mutex::new(Vec::new());

    fn record_open(paths: Vec<PathBuf>) {
        OPENED.lock().unwrap().push(paths);
    }

    extern "C-unwind" fn record_reply(_this: &NSObject, _cmd: Sel, reply: usize) {
        REPLY.store(reply, Ordering::SeqCst);
    }

    fn make_class(name: &std::ffi::CStr) -> ClassBuilder {
        ClassBuilder::new(name, NSObject::class()).expect("class name is unique")
    }

    #[test]
    fn the_added_method_hands_over_the_paths_and_replies_success() {
        // A delegate-like class that lacks the method, and an application
        // stand-in that records `replyToOpenOrPrint:` (the real NSApplication
        // is main-thread only, which test threads are not).
        let delegate_class = make_class(c"AggOsTestDelegate").register();
        let mut app_builder = make_class(c"AggOsTestApplication");
        // SAFETY: `record_reply` has the signature of
        // `-(void)replyToOpenOrPrint:(NSUInteger)reply`, which is what the
        // method under test sends.
        unsafe {
            app_builder.add_method(
                sel!(replyToOpenOrPrint:),
                record_reply as extern "C-unwind" fn(_, _, _),
            );
        }
        let app_class = app_builder.register();

        set_callback(record_open);
        assert!(!delegate_class.responds_to(sel!(application:openFiles:)));
        assert!(add_open_files_method(delegate_class));
        assert!(delegate_class.responds_to(sel!(application:openFiles:)));
        // Adding again leaves the method that is there.
        assert!(add_open_files_method(delegate_class));

        let files = NSArray::from_retained_slice(&[
            NSString::from_str("/Users/me/a part.mcx"),
            NSString::from_str("/Users/me/b.stl"),
        ]);
        // SAFETY: `new` on an NSObject subclass returns a retained instance.
        let delegate: Retained<AnyObject> = unsafe { msg_send![delegate_class, new] };
        // SAFETY: as above.
        let app: Retained<AnyObject> = unsafe { msg_send![app_class, new] };
        OPENED.lock().unwrap().clear();
        // SAFETY: the delegate class now has `application:openFiles:` taking
        // an object and an NSArray of NSString, which is what is sent; the
        // stand-in app answers the reply it sends back.
        let () = unsafe { msg_send![&*delegate, application: &*app, openFiles: &*files] };

        assert_eq!(
            *OPENED.lock().unwrap(),
            vec![vec![
                PathBuf::from("/Users/me/a part.mcx"),
                PathBuf::from("/Users/me/b.stl")
            ]]
        );
        assert_eq!(
            REPLY.load(Ordering::SeqCst),
            NSApplicationDelegateReply::Success.0
        );
    }
}
