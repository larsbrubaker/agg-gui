//! The Objective-C side of a native menu bar (feature `app-menu`): one object,
//! a [`MenuTarget`], that is the delegate of every menu it makes and the
//! target of every action item it makes, and that turns what AppKit sends it
//! into calls on a [`MenuHandler`].
//!
//! agg-gui-shell is the consumer (`menu_bar/macos.rs`): it owns the menu
//! model, fills the menus, matches key equivalents and queues activations,
//! all in safe Rust through its `MenuHandler`. What needs `unsafe` is here:
//! the class definition, every pairing of an action selector with a target,
//! and the out-pointer writes of `menuHasKeyEquivalent:forEvent:target:action:`.
//!
//! Soundness: an item made here sends only a selector its target answers.
//! An action item's target is the controller, which implements
//! `menuItemSelected:`; a key-equivalent claim names either the controller
//! with `menuKeyEquivalentFired:`, or no target with one of the standard
//! [`AppAction`] selectors, which AppKit sends up the responder chain to
//! `NSApplication`. Menus hold their delegate and items their target weakly,
//! so the controller would be the only thing keeping either valid; it is
//! never deallocated instead: [`MenuTarget::new`] keeps one reference to it
//! that is never released, so the Objective-C object and its handler live
//! for the rest of the process. Every menu delegate and item target made
//! here therefore stays set and valid, AppKit never resolves an item's
//! action through the responder chain for want of its target, and a
//! controller method (and the handler method it calls) always runs on a
//! live object, even when the handler drops every `MenuTarget` from inside
//! it. The class is main-thread only, so the handler is called on the main
//! thread only.

use std::ffi::c_void;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSEvent, NSMenu, NSMenuDelegate, NSMenuItem};
use objc2_foundation::NSString;

/// What a [`MenuTarget`] calls when AppKit asks it something. Implemented by
/// the menu bar's owner.
///
/// Called on the main thread only. AppKit calls delegate methods from inside
/// other AppKit calls (building a menu can ask another menu to update), so
/// the methods may be re-entered and take `&self`.
pub trait MenuHandler: 'static {
    /// AppKit is about to show `menu`, one made by [`MenuTarget::new_menu`]
    /// (`menuNeedsUpdate:`): the moment to refill it.
    fn menu_needs_update(&self, menu: &NSMenu, mtm: MainThreadMarker);

    /// The item made by [`MenuTarget::new_action_item`] with `tag` was picked
    /// (or its key equivalent pressed).
    fn item_selected(&self, tag: isize);

    /// AppKit is looking for the item a key-down belongs to
    /// (`menuHasKeyEquivalent:forEvent:target:action:`, asked of a menu made
    /// by [`MenuTarget::new_menu`]). [`KeyClaim::Handler`] has AppKit call
    /// [`Self::key_equivalent_fired`] next.
    fn key_equivalent(&self, event: &NSEvent) -> KeyClaim;

    /// The chord [`Self::key_equivalent`] claimed with [`KeyClaim::Handler`]
    /// fired.
    fn key_equivalent_fired(&self);
}

/// The answer to a key equivalent ([`MenuHandler::key_equivalent`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyClaim {
    /// Not a menu chord: AppKit goes on to the next menu, then the window.
    None,
    /// A standard application chord: AppKit sends the action to
    /// `NSApplication`.
    App(AppAction),
    /// The handler's own: AppKit calls [`MenuHandler::key_equivalent_fired`].
    Handler,
}

/// The standard application-menu actions `NSApplication` answers itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppAction {
    /// Hide the application (`hide:`).
    Hide,
    /// Hide every other application (`hideOtherApplications:`).
    HideOthers,
    /// Show every application (`unhideAllApplications:`).
    ShowAll,
}

impl AppAction {
    /// The action's selector; `NSApplication` implements each as an action
    /// method (`-(void)…:(id)sender`).
    fn selector(self) -> Sel {
        match self {
            Self::Hide => sel!(hide:),
            Self::HideOthers => sel!(hideOtherApplications:),
            Self::ShowAll => sel!(unhideAllApplications:),
        }
    }
}

/// The controller's instance variables: the handler it calls.
struct Ivars {
    handler: Box<dyn MenuHandler>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the class adds no
    // Drop impl, and its ivars are set before `init` (`MenuTarget::new`).
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "AggOsMenuTarget"]
    #[ivars = Ivars]
    struct Controller;

    // SAFETY: NSObjectProtocol has no requirements beyond being an NSObject
    // subclass, which the superclass above guarantees.
    unsafe impl NSObjectProtocol for Controller {}

    // SAFETY: the one protocol method implemented below has the signature
    // NSMenuDelegate declares for it (objc2-app-kit's `menuNeedsUpdate:`).
    unsafe impl NSMenuDelegate for Controller {
        // SAFETY: `-(void)menuNeedsUpdate:(NSMenu *)menu`; AppKit passes a
        // non-null menu.
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            self.ivars()
                .handler
                .menu_needs_update(menu, MainThreadMarker::from(self));
        }
    }

    impl Controller {
        /// Every action item's action: reports the item's tag.
        // SAFETY: `-(void)menuItemSelected:(id)sender`, an action method; the
        // sender is nullable, and is an NSMenuItem whenever an item made by
        // `MenuTarget::new_action_item` sends it (the only items given this
        // selector with this target).
        #[unsafe(method(menuItemSelected:))]
        fn menu_item_selected(&self, sender: Option<&NSMenuItem>) {
            if let Some(sender) = sender {
                self.ivars().handler.item_selected(sender.tag());
            }
        }

        /// AppKit is looking for the item a key equivalent belongs to.
        // SAFETY: `-(BOOL)menuHasKeyEquivalent:(NSMenu *)menu
        // forEvent:(NSEvent *)event target:(id *)target action:(SEL *)action`
        // (NSMenuDelegate, not in objc2-app-kit's generated trait): the menu
        // and event are non-null objects, `target` an `id *` and `action` a
        // `SEL *` (taken as an untyped pointer, since `Sel` has no reference
        // encoding), both nullable and checked before use; BOOL as `Bool`.
        #[unsafe(method(menuHasKeyEquivalent:forEvent:target:action:))]
        fn menu_has_key_equivalent(
            &self,
            _menu: &NSMenu,
            event: &NSEvent,
            target: *mut *mut AnyObject,
            // A `SEL *`, written as an `Option<Sel>` (pointer-sized, null for none).
            action: *mut c_void,
        ) -> Bool {
            // Both are documented as nullable; with nowhere to report the
            // dispatch there is no honest way to claim the chord.
            if target.is_null() || action.is_null() {
                return Bool::NO;
            }
            let (to, selector) = match self.ivars().handler.key_equivalent(event) {
                KeyClaim::None => return Bool::NO,
                // A nil target walks the responder chain to NSApp, which
                // implements every `AppAction`.
                KeyClaim::App(app_action) => (std::ptr::null_mut(), app_action.selector()),
                // This controller implements `menuKeyEquivalentFired:` (below)
                // with an action method's signature, and is never deallocated
                // (`MenuTarget::new`), so the target stays valid for as long
                // as AppKit keeps it.
                KeyClaim::Handler => (
                    self as *const Self as *mut AnyObject,
                    sel!(menuKeyEquivalentFired:),
                ),
            };
            // SAFETY: both pointers were checked non-null above, and AppKit
            // hands them to us to write an `id` and a `SEL` through. The `id`
            // is nil or this controller, written as a +0 reference: an `id *`
            // out-parameter returns an object the caller does not own, and
            // the controller is never deallocated (`MenuTarget::new`), so the
            // pointer is valid for any use the caller makes of it, at any
            // time, without a retain of its own.
            unsafe {
                *target = to;
                *action.cast::<Option<Sel>>() = Some(selector);
            }
            Bool::YES
        }

        /// Runs the chord `menuHasKeyEquivalent:` claimed for the handler.
        // SAFETY: `-(void)menuKeyEquivalentFired:(id)sender`, an action
        // method; the sender is unused and nullable.
        #[unsafe(method(menuKeyEquivalentFired:))]
        fn menu_key_equivalent_fired(&self, _sender: Option<&AnyObject>) {
            self.ivars().handler.key_equivalent_fired();
        }
    }
);

/// The delegate of the menus and the target of the action items it makes,
/// calling its [`MenuHandler`]. Cloning shares the one Objective-C object.
///
/// An application makes one. Its Objective-C object and the handler live for
/// the rest of the process, however many `MenuTarget` values are dropped: a
/// menu's delegate and an item's target, which AppKit holds weakly, never go
/// away, and a handler method always runs on a live object, even one that
/// drops every `MenuTarget` from inside it. The handler's `Drop` therefore
/// never runs. Main-thread only: it is neither `Send` nor `Sync`.
#[derive(Clone)]
pub struct MenuTarget(Retained<Controller>);

impl MenuTarget {
    /// A target that calls `handler`. The Objective-C object it makes, and
    /// `handler` with it, live for the rest of the process: dropping every
    /// `MenuTarget` deallocates nothing, and `handler` is never dropped.
    pub fn new(mtm: MainThreadMarker, handler: Box<dyn MenuHandler>) -> Self {
        let this = Controller::alloc(mtm).set_ivars(Ivars { handler });
        // SAFETY: NSObject's designated initializer, on an allocation whose
        // ivars are set.
        let controller: Retained<Controller> = unsafe { msg_send![super(this), init] };
        // Deliberately never released: this one reference keeps the
        // controller alive for the rest of the process, which the soundness
        // of every menu and item made from it rests on (module header).
        std::mem::forget(controller.clone());
        Self(controller)
    }

    /// An empty menu titled `title` whose delegate is this target.
    pub fn new_menu(&self, title: &str) -> Retained<NSMenu> {
        let mtm = MainThreadMarker::from(&*self.0);
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
        menu.setDelegate(Some(ProtocolObject::from_ref(&*self.0)));
        menu
    }

    /// An item whose action reaches [`MenuHandler::item_selected`] with
    /// `tag`. `key_equivalent` is `""` for none.
    pub fn new_action_item(
        &self,
        title: &str,
        tag: isize,
        key_equivalent: &str,
    ) -> Retained<NSMenuItem> {
        let mtm = MainThreadMarker::from(&*self.0);
        // SAFETY: `sel!` builds a valid selector.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                Some(sel!(menuItemSelected:)),
                &NSString::from_str(key_equivalent),
            )
        };
        // SAFETY: the target answers the item's action: the controller
        // implements `menuItemSelected:` as `-(void)menuItemSelected:(id)sender`.
        // The property is weak, and the controller is never deallocated
        // (`MenuTarget::new`), so the item keeps this target for its life.
        unsafe { item.setTarget(Some(&self.0)) };
        item.setTag(tag);
        item
    }

    /// An item that sends `action` to `NSApplication` (no target: AppKit walks
    /// the responder chain to it). `key_equivalent` is `""` for none.
    pub fn new_app_item(
        mtm: MainThreadMarker,
        title: &str,
        action: AppAction,
        key_equivalent: &str,
    ) -> Retained<NSMenuItem> {
        // SAFETY: `AppAction::selector` returns selectors built with `sel!`,
        // and NSApplication implements each as an action method.
        unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                Some(action.selector()),
                &NSString::from_str(key_equivalent),
            )
        }
    }
}
