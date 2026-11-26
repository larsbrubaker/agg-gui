//! `agg_os::macos::menu` against real AppKit objects: a `MenuTarget` with a
//! recording `MenuHandler`, a real `NSMenu` and real `NSMenuItem`s, driven
//! the way AppKit drives them (the delegate's `menuNeedsUpdate:`, an item's
//! action through `-[NSMenu performActionForItemAtIndex:]`, and
//! `menuHasKeyEquivalent:forEvent:target:action:` with a synthesized key-down
//! whose answer is dispatched through `-[NSApplication sendAction:to:from:]`
//! and through `-[NSMenu performKeyEquivalent:]`), and that the controller
//! outlives every `MenuTarget` value: menus keep their delegate and items
//! their target, and a handler that drops the last `MenuTarget` from inside
//! its own method finishes the call.
//!
//! AppKit objects are main-thread only, and the test harness runs tests on
//! other threads, so this is a `harness = false` test (`Cargo.toml`): `main`
//! runs every check on the main thread, prints one `ok` or `FAILED` line per
//! check, and exits non-zero when any failed. Off macOS it does nothing.

#[cfg(target_os = "macos")]
mod real {
    use std::cell::{Cell, RefCell};
    use std::ffi::c_void;
    use std::rc::Rc;

    use agg_os::macos::menu::{AppAction, KeyClaim, MenuHandler, MenuTarget};
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::runtime::{AnyObject, Bool, Sel};
    use objc2::{msg_send, sel, MainThreadMarker};
    use objc2_app_kit::{
        NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSMenu, NSMenuDelegate,
    };
    use objc2_foundation::{NSPoint, NSString};

    /// What the handler was told.
    #[derive(Default)]
    struct Record {
        /// The address of each menu `menu_needs_update` was given.
        updated: RefCell<Vec<usize>>,
        selected: RefCell<Vec<isize>>,
        /// The characters and modifier flags of each key event asked about.
        keys: RefCell<Vec<(String, usize)>>,
        fired: Cell<u32>,
        /// What `key_equivalent` answers.
        claim: Cell<Option<KeyClaim>>,
    }

    struct Recorder(Rc<Record>);

    impl MenuHandler for Recorder {
        fn menu_needs_update(&self, menu: &NSMenu, _mtm: MainThreadMarker) {
            self.0.updated.borrow_mut().push(address(menu));
        }

        fn item_selected(&self, tag: isize) {
            self.0.selected.borrow_mut().push(tag);
        }

        fn key_equivalent(&self, event: &NSEvent) -> KeyClaim {
            let characters = event
                .charactersIgnoringModifiers()
                .map(|s| s.to_string())
                .unwrap_or_default();
            self.0
                .keys
                .borrow_mut()
                .push((characters, event.modifierFlags().0));
            self.0.claim.get().unwrap_or(KeyClaim::None)
        }

        fn key_equivalent_fired(&self) {
            self.0.fired.set(self.0.fired.get() + 1);
        }
    }

    fn address<T>(object: &T) -> usize {
        object as *const T as usize
    }

    /// Prints the check's line; returns whether it passed.
    fn check(name: &str, passed: bool, detail: impl FnOnce() -> String) -> bool {
        if passed {
            println!("ok - {name}");
        } else {
            println!("FAILED - {name}: {}", detail());
        }
        passed
    }

    fn command_key(chars: &str) -> Retained<NSEvent> {
        let chars = NSString::from_str(chars);
        NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            NSEventType::KeyDown,
            NSPoint::new(0.0, 0.0),
            NSEventModifierFlags::Command,
            0.0,
            0,
            None,
            &chars,
            &chars,
            false,
            40,
        )
        .expect("AppKit makes a key event")
    }

    /// What `menuHasKeyEquivalent:forEvent:target:action:` answers: the
    /// return value, the target written and the action written.
    fn ask_key_equivalent(
        delegate: &AnyObject,
        menu: &NSMenu,
        event: &NSEvent,
    ) -> (bool, *mut AnyObject, Option<Sel>) {
        let mut target: *mut AnyObject = std::ptr::null_mut();
        let mut action: Option<Sel> = None;
        let target_out: *mut *mut AnyObject = &mut target;
        let action_out: *mut c_void = (&mut action as *mut Option<Sel>).cast();
        // SAFETY: the delegate is the controller, which implements
        // `-(BOOL)menuHasKeyEquivalent:(NSMenu *)menu forEvent:(NSEvent *)event
        // target:(id *)target action:(SEL *)action` with these argument types;
        // both out-pointers point at locals that outlive the call, of the
        // types the method writes (`id` and `SEL`, as `Option<Sel>`).
        let claimed: Bool = unsafe {
            msg_send![
                delegate,
                menuHasKeyEquivalent: menu,
                forEvent: event,
                target: target_out,
                action: action_out
            ]
        };
        (claimed.as_bool(), target, action)
    }

    /// Runs every check; returns whether all passed.
    pub fn run() -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            println!("FAILED - main thread: main() is not on the main thread");
            return false;
        };
        // `performActionForItemAtIndex:` dispatches through NSApp.
        let app = NSApplication::sharedApplication(mtm);
        let record = Rc::new(Record::default());
        let target = MenuTarget::new(mtm, Box::new(Recorder(Rc::clone(&record))));
        let menu = target.new_menu("File");
        let item = target.new_action_item("Open", 42, "o");
        menu.addItem(&item);
        let hide = MenuTarget::new_app_item(mtm, "Hide Test", AppAction::Hide, "h");
        menu.addItem(&hide);
        let mut all = true;

        let delegate = menu.delegate();
        all &= check(
            "the menu's delegate is the target",
            delegate.is_some(),
            || "no delegate".into(),
        );
        let Some(delegate) = delegate else {
            return false;
        };
        let delegate_object: &AnyObject = delegate.as_ref();
        let controller = delegate_object as *const AnyObject as *mut AnyObject;

        let item_target = item
            .target()
            .map(|t| Retained::as_ptr(&t) as *mut AnyObject);
        all &= check(
            "an action item targets the target with menuItemSelected:",
            item_target == Some(controller) && item.action() == Some(sel!(menuItemSelected:)),
            || format!("target {item_target:?}, action {:?}", item.action()),
        );
        all &= check(
            "an action item keeps its title, tag and key equivalent",
            item.title().to_string() == "Open"
                && item.tag() == 42
                && item.keyEquivalent().to_string() == "o",
            || {
                format!(
                    "{} / {} / {}",
                    item.title(),
                    item.tag(),
                    item.keyEquivalent()
                )
            },
        );
        all &= check(
            "an app item sends hide: to no target",
            hide.target().is_none() && hide.action() == Some(sel!(hide:)),
            || format!("action {:?}", hide.action()),
        );

        delegate.menuNeedsUpdate(&menu);
        all &= check(
            "menuNeedsUpdate: reaches the handler with the menu",
            *record.updated.borrow() == vec![address(&*menu)],
            || format!("{:?}", record.updated.borrow()),
        );

        menu.performActionForItemAtIndex(0);
        all &= check(
            "performing an action item reaches item_selected with its tag",
            *record.selected.borrow() == vec![42],
            || format!("{:?}", record.selected.borrow()),
        );

        let event = command_key("k");
        record.claim.set(Some(KeyClaim::Handler));
        let (claimed, to, action) = ask_key_equivalent(delegate_object, &menu, &event);
        all &= check(
            "a handler claim names the target and menuKeyEquivalentFired:",
            claimed && to == controller && action == Some(sel!(menuKeyEquivalentFired:)),
            || format!("claimed {claimed}, target {to:?}, action {action:?}"),
        );
        all &= check(
            "the handler is asked with the event's characters and modifiers",
            *record.keys.borrow() == vec![("k".to_string(), NSEventModifierFlags::Command.0)],
            || format!("{:?}", record.keys.borrow()),
        );
        if let (true, Some(action)) = (claimed, action) {
            // SAFETY: the action and target are what the controller just
            // claimed: it implements `menuKeyEquivalentFired:` as an action
            // method taking a nullable sender.
            let sent = unsafe { app.sendAction_to_from(action, Some(delegate_object), None) };
            all &= check(
                "sending the claimed action reaches key_equivalent_fired",
                sent && record.fired.get() == 1,
                || format!("sent {sent}, fired {}", record.fired.get()),
            );
        }

        let handled = menu.performKeyEquivalent(&event);
        all &= check(
            "-[NSMenu performKeyEquivalent:] asks the delegate and fires the claim",
            handled && record.fired.get() == 2 && record.keys.borrow().len() == 2,
            || {
                format!(
                    "handled {handled}, fired {}, asked {}",
                    record.fired.get(),
                    record.keys.borrow().len()
                )
            },
        );

        record.claim.set(Some(KeyClaim::App(AppAction::HideOthers)));
        let (claimed, to, action) = ask_key_equivalent(delegate_object, &menu, &event);
        all &= check(
            "an app claim names no target and the app's selector",
            claimed && to.is_null() && action == Some(sel!(hideOtherApplications:)),
            || format!("claimed {claimed}, target {to:?}, action {action:?}"),
        );

        record.claim.set(Some(KeyClaim::None));
        let (claimed, to, action) = ask_key_equivalent(delegate_object, &menu, &event);
        all &= check(
            "no claim answers NO and writes nothing",
            !claimed && to.is_null() && action.is_none(),
            || format!("claimed {claimed}, target {to:?}, action {action:?}"),
        );

        let asked = record.keys.borrow().len();
        // SAFETY: as in `ask_key_equivalent`, with both out-pointers null,
        // which the method's contract allows.
        let claimed: Bool = unsafe {
            msg_send![
                delegate_object,
                menuHasKeyEquivalent: &*menu,
                forEvent: &*event,
                target: std::ptr::null_mut::<*mut AnyObject>(),
                action: std::ptr::null_mut::<c_void>()
            ]
        };
        all &= check(
            "null out-pointers answer NO without asking the handler",
            !claimed.as_bool() && record.keys.borrow().len() == asked,
            || format!("claimed {}", claimed.as_bool()),
        );

        all &= dropped_target_keeps_handler_alive(mtm);
        all &= handler_dropping_its_target_completes(mtm);
        all
    }

    /// Records the tags it is given and sets its flag when dropped, so the
    /// test sees whether the handler is ever deallocated.
    struct Watched {
        dropped: Rc<Cell<bool>>,
        selected: Rc<RefCell<Vec<isize>>>,
    }

    impl Drop for Watched {
        fn drop(&mut self) {
            self.dropped.set(true);
        }
    }

    impl MenuHandler for Watched {
        fn menu_needs_update(&self, _menu: &NSMenu, _mtm: MainThreadMarker) {}
        fn item_selected(&self, tag: isize) {
            self.selected.borrow_mut().push(tag);
        }
        fn key_equivalent(&self, _event: &NSEvent) -> KeyClaim {
            KeyClaim::None
        }
        fn key_equivalent_fired(&self) {}
    }

    /// The controller lives for the rest of the process: once every
    /// `MenuTarget` value is dropped the handler is still alive, the menu
    /// keeps its delegate and the item its target, and the item's action
    /// still reaches the handler.
    fn dropped_target_keeps_handler_alive(mtm: MainThreadMarker) -> bool {
        let dropped = Rc::new(Cell::new(false));
        let selected = Rc::new(RefCell::new(Vec::new()));
        let (menu, item) = autoreleasepool(|_| {
            let handler = Watched {
                dropped: Rc::clone(&dropped),
                selected: Rc::clone(&selected),
            };
            let target = MenuTarget::new(mtm, Box::new(handler));
            let menu = target.new_menu("Kept");
            let item = target.new_action_item("Kept", 7, "");
            menu.addItem(&item);
            (menu, item)
        });
        let mut all = check(
            "dropping every MenuTarget does not drop the handler",
            !dropped.get(),
            || "the handler was dropped".into(),
        );
        let (has_delegate, has_target) =
            autoreleasepool(|_| (menu.delegate().is_some(), item.target().is_some()));
        all &= check(
            "after every MenuTarget is dropped the menu keeps its delegate and the item its target",
            has_delegate && has_target,
            || format!("delegate {has_delegate}, target {has_target}"),
        );
        autoreleasepool(|_| menu.performActionForItemAtIndex(0));
        all &= check(
            "after every MenuTarget is dropped the action item still reaches item_selected",
            *selected.borrow() == vec![7],
            || format!("{:?}", selected.borrow()),
        );
        all
    }

    /// Drops the `MenuTarget` it shares with the test from inside
    /// `item_selected`, then records that the call went on to its end.
    struct DropsItsTarget {
        slot: Rc<RefCell<Option<MenuTarget>>>,
        completed: Rc<RefCell<Vec<isize>>>,
    }

    impl MenuHandler for DropsItsTarget {
        fn menu_needs_update(&self, _menu: &NSMenu, _mtm: MainThreadMarker) {}
        fn item_selected(&self, tag: isize) {
            let last = self.slot.borrow_mut().take();
            drop(last);
            self.completed.borrow_mut().push(tag);
        }
        fn key_equivalent(&self, _event: &NSEvent) -> KeyClaim {
            KeyClaim::None
        }
        fn key_equivalent_fired(&self) {}
    }

    /// A handler that drops the last `MenuTarget` while its own method runs
    /// finishes the call, and is called again by the next pick.
    fn handler_dropping_its_target_completes(mtm: MainThreadMarker) -> bool {
        let slot: Rc<RefCell<Option<MenuTarget>>> = Rc::new(RefCell::new(None));
        let completed = Rc::new(RefCell::new(Vec::new()));
        let menu = autoreleasepool(|_| {
            let handler = DropsItsTarget {
                slot: Rc::clone(&slot),
                completed: Rc::clone(&completed),
            };
            *slot.borrow_mut() = Some(MenuTarget::new(mtm, Box::new(handler)));
            let borrowed = slot.borrow();
            let target = borrowed.as_ref().expect("the slot was just filled");
            let menu = target.new_menu("Reentrant");
            menu.addItem(&target.new_action_item("Reentrant", 9, ""));
            menu
        });
        autoreleasepool(|_| menu.performActionForItemAtIndex(0));
        let mut all = check(
            "a handler that drops its last MenuTarget inside item_selected completes the call",
            *completed.borrow() == vec![9] && slot.borrow().is_none(),
            || format!("{:?}", completed.borrow()),
        );
        autoreleasepool(|_| menu.performActionForItemAtIndex(0));
        all &= check(
            "that handler is called again by the next pick",
            *completed.borrow() == vec![9, 9],
            || format!("{:?}", completed.borrow()),
        );
        all
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    if !real::run() {
        std::process::exit(1);
    }
}
