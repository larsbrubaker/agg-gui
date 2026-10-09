//! The AppKit half of the native menu bar: builds a [`MenuBarModel`] into an
//! `NSMenu` tree, makes it `-[NSApp mainMenu]`, and routes picks back to the
//! shell loop. Port of agg-sharp `PlatformMac/mac/MacMenuBar.cs` over objc2.
//!
//! One controller object is the target of every item and the delegate of
//! every menu. Each menu refills itself from its model in `menuNeedsUpdate:`
//! as AppKit is about to show it, so recent files and gates are read when
//! they are looked at. Which item a key chord belongs to is answered from the
//! model in `menuHasKeyEquivalent:forEvent:target:action:` (see
//! [`super::match_key_equivalent`]): without it AppKit would update every menu
//! - re-running every provider - to answer each Command keystroke.
//!
//! Items find their model through a tag that is never reused (a fresh number
//! per built item), so a deallocated item's address handed to a later item
//! can never run the earlier item's action.
//!
//! The application menu also carries the standard Hide, Hide Others and Show
//! All group above Quit, answered by `NSApplication` itself, as every mac
//! application's menu does.

use std::cell::RefCell;
use std::collections::HashMap;

use objc2::rc::Retained;
use std::ffi::c_void;

use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSEvent, NSEventModifierFlags, NSMenu, NSMenuDelegate, NSMenuItem,
};
use objc2_foundation::NSString;

use super::{
    children_of, is_enabled, key_equivalent_for, match_key_equivalent, modifier_flags,
    queue_activation, top_level_menus, MenuBarModel, MenuItemModel, MenuItemRole,
};

/// What the controller needs to answer AppKit: the models behind the built
/// items and menus, and the installed bar.
struct State {
    controller: Retained<MenuController>,
    items: HashMap<isize, MenuItemModel>,
    next_tag: isize,
    /// Every menu built from a container, with that container and whether it
    /// hangs directly off the bar (only those draw key equivalents). Retained
    /// here, so a menu's address cannot be reused while it is listed.
    owners: Vec<(Retained<NSMenu>, MenuItemModel, bool)>,
    /// The application menu (the first top-level menu), which also carries
    /// the standard Hide group.
    application_menu: Option<Retained<NSMenu>>,
    application_title: String,
    installed: Option<MenuBarModel>,
    pending_key_equivalent: Option<MenuItemModel>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and the class adds no
    // Drop impl or ivars.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "AggGuiShellMenuController"]
    struct MenuController;

    unsafe impl NSObjectProtocol for MenuController {}

    unsafe impl NSMenuDelegate for MenuController {
        /// AppKit is about to show `menu`: throw its contents away and build
        /// them again from the model. A menu built from no container (the
        /// main menu, or one from a replaced bar) is left exactly as it is.
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            let owner = STATE.with(|state| {
                state.borrow().as_ref().and_then(|state| {
                    state
                        .owners
                        .iter()
                        .find(|(owned, _, _)| std::ptr::eq(&**owned, menu))
                        .map(|(_, container, top)| (container.clone(), *top))
                })
            });
            let Some((container, top)) = owner else {
                return;
            };
            // Before removeAllItems: that releases the items, after which there
            // is nothing left to read a submenu off.
            forget_contents(menu);
            menu.removeAllItems();
            if let Some(mtm) = MainThreadMarker::new() {
                populate_menu(mtm, menu, &container, top);
            }
        }
    }

    impl MenuController {
        #[unsafe(method(menuItemSelected:))]
        fn menu_item_selected(&self, sender: &NSMenuItem) {
            let tag = sender.tag();
            let model = STATE.with(|state| {
                state
                    .borrow()
                    .as_ref()
                    .and_then(|state| state.items.get(&tag).cloned())
            });
            if let Some(model) = model {
                queue_activation(&model);
            }
        }

        /// AppKit is looking for the item a key equivalent belongs to: answer
        /// from the whole installed model, whichever menu is asking, so the
        /// first menu asked gives the final answer.
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
            let characters = event
                .charactersIgnoringModifiers()
                .map(|s| s.to_string())
                .unwrap_or_default();
            let flags = event.modifierFlags().0 as u64;
            // The standard application chords NSApplication answers itself.
            if let Some(selector) = standard_application_chord(&characters, flags) {
                // SAFETY: both pointers were checked non-null above and AppKit
                // hands them to us to write through.
                unsafe {
                    *target = std::ptr::null_mut();
                    *action.cast::<Option<Sel>>() = Some(selector);
                }
                return Bool::YES;
            }
            let matched = STATE.with(|state| {
                let state = state.borrow();
                let state = state.as_ref()?;
                match_key_equivalent(state.installed.as_ref(), &characters, flags)
            });
            let Some(matched) = matched else {
                return Bool::NO;
            };
            STATE.with(|state| {
                if let Some(state) = state.borrow_mut().as_mut() {
                    state.pending_key_equivalent = Some(matched);
                }
            });
            // SAFETY: as above; the controller outlives the dispatch (it is
            // kept in STATE for the life of the process).
            unsafe {
                *target = self as *const Self as *mut AnyObject;
                *action.cast::<Option<Sel>>() = Some(sel!(menuKeyEquivalentFired:));
            }
            Bool::YES
        }

        /// Runs the item `menuHasKeyEquivalent:` matched.
        #[unsafe(method(menuKeyEquivalentFired:))]
        fn menu_key_equivalent_fired(&self, _sender: Option<&AnyObject>) {
            let model = STATE.with(|state| {
                state
                    .borrow_mut()
                    .as_mut()
                    .and_then(|state| state.pending_key_equivalent.take())
            });
            if let Some(model) = model {
                queue_activation(&model);
            }
        }
    }
);

impl MenuController {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}

/// Command-H hides the application and Option-Command-H the others: the
/// standard application menu's chords, sent to `NSApp` (target nil walks the
/// responder chain to it).
fn standard_application_chord(characters: &str, flags: u64) -> Option<Sel> {
    use modifier_flags::{COMMAND, CONTROL, OPTION, SHIFT};
    if !characters.eq_ignore_ascii_case("h") {
        return None;
    }
    match flags & (COMMAND | SHIFT | CONTROL | OPTION) {
        f if f == COMMAND => Some(sel!(hide:)),
        f if f == COMMAND | OPTION => Some(sel!(hideOtherApplications:)),
        _ => None,
    }
}

pub(super) fn install(model: MenuBarModel) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("menu_bar::install called off the main thread; no menu bar installed");
        return false;
    };
    let controller = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let state = state.get_or_insert_with(|| State {
            controller: MenuController::new(mtm),
            items: HashMap::new(),
            next_tag: 1,
            owners: Vec::new(),
            application_menu: None,
            application_title: String::new(),
            installed: None,
            pending_key_equivalent: None,
        });
        state.items.clear();
        state.owners.clear();
        state.application_menu = None;
        state.pending_key_equivalent = None;
        state.installed = Some(model.clone());
        state.controller.clone()
    });

    let main_menu = create_menu(mtm, "MainMenu", &controller);
    for (index, top_level) in top_level_menus(&model.menus).into_iter().enumerate() {
        // A top level entry is always a submenu: an item directly in the bar
        // with an action of its own is not a thing AppKit draws.
        let menu_item = create_menu_item(mtm, &top_level.text, None, "");
        let sub_menu = create_menu(mtm, &top_level.text, &controller);
        STATE.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state
                    .owners
                    .push((sub_menu.clone(), top_level.clone(), true));
                if index == 0 {
                    state.application_menu = Some(sub_menu.clone());
                    state.application_title = top_level.text.clone();
                }
            }
        });
        populate_menu(mtm, &sub_menu, &top_level, true);
        menu_item.setSubmenu(Some(&sub_menu));
        menu_item.setEnabled(is_enabled(&top_level));
        main_menu.addItem(&menu_item);
    }
    NSApplication::sharedApplication(mtm).setMainMenu(Some(&main_menu));
    true
}

fn create_menu(
    mtm: MainThreadMarker,
    title: &str,
    controller: &MenuController,
) -> Retained<NSMenu> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    // Every enabled state is written explicitly from the model, so AppKit must
    // not also go looking for a validateMenuItem: or a responder.
    menu.setAutoenablesItems(false);
    menu.setDelegate(Some(ProtocolObject::from_ref(controller)));
    menu
}

fn create_menu_item(
    mtm: MainThreadMarker,
    title: &str,
    action: Option<Sel>,
    key_equivalent: &str,
) -> Retained<NSMenuItem> {
    // SAFETY: `action` is either None or a selector built with `sel!`.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(key_equivalent),
        )
    };
    if !key_equivalent.is_empty() {
        item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    }
    item
}

/// Fills `menu` with one native item per visible child of `container`,
/// recursing into submenus. Only a menu hanging directly off the bar hands its
/// children key equivalents, the only depth `match_key_equivalent` looks at.
fn populate_menu(mtm: MainThreadMarker, menu: &NSMenu, container: &MenuItemModel, top: bool) {
    let Some(controller) =
        STATE.with(|state| state.borrow().as_ref().map(|s| s.controller.clone()))
    else {
        return;
    };
    let is_application_menu = STATE.with(|state| {
        state.borrow().as_ref().is_some_and(|state| {
            state
                .application_menu
                .as_ref()
                .is_some_and(|app| std::ptr::eq(&**app, menu))
        })
    });
    for child in children_of(container) {
        if is_application_menu && child.role == MenuItemRole::Quit {
            add_standard_hide_group(mtm, menu);
        }
        if child.is_separator {
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            continue;
        }
        let menu_item = if child.sub_menu_items.is_some() {
            let menu_item = create_menu_item(mtm, &child.text, None, "");
            let sub_menu = create_menu(mtm, &child.text, &controller);
            STATE.with(|state| {
                if let Some(state) = state.borrow_mut().as_mut() {
                    state.owners.push((sub_menu.clone(), child.clone(), false));
                }
            });
            populate_menu(mtm, &sub_menu, &child, false);
            menu_item.setSubmenu(Some(&sub_menu));
            menu_item
        } else {
            let chord = if top {
                key_equivalent_for(child.role)
            } else {
                ""
            };
            let menu_item =
                create_menu_item(mtm, &child.text, Some(sel!(menuItemSelected:)), chord);
            // SAFETY: the controller is a live object kept in STATE.
            unsafe { menu_item.setTarget(Some(&controller)) };
            let tag = STATE.with(|state| {
                let mut state = state.borrow_mut();
                let state = state.as_mut()?;
                let tag = state.next_tag;
                state.next_tag += 1;
                state.items.insert(tag, child.clone());
                Some(tag)
            });
            if let Some(tag) = tag {
                menu_item.setTag(tag);
            }
            if let Some(is_checked) = &child.is_checked {
                // NSControlStateValueOn is 1.
                menu_item.setState(if is_checked() { 1 } else { 0 });
            }
            menu_item
        };
        if let Some(tool_tip) = &child.tool_tip_text {
            menu_item.setToolTip(Some(&NSString::from_str(tool_tip)));
        }
        menu_item.setEnabled(is_enabled(&child));
        menu.addItem(&menu_item);
    }
}

/// Hide {app} (Cmd-H), Hide Others (Option-Cmd-H), Show All, and a divider:
/// the group every mac application menu carries above Quit.
fn add_standard_hide_group(mtm: MainThreadMarker, menu: &NSMenu) {
    let title = STATE.with(|state| {
        state
            .borrow()
            .as_ref()
            .map(|s| s.application_title.clone())
            .unwrap_or_default()
    });
    let hide = create_menu_item(mtm, &format!("Hide {title}"), Some(sel!(hide:)), "h");
    menu.addItem(&hide);
    let hide_others = create_menu_item(mtm, "Hide Others", Some(sel!(hideOtherApplications:)), "h");
    hide_others
        .setKeyEquivalentModifierMask(NSEventModifierFlags::Command | NSEventModifierFlags::Option);
    menu.addItem(&hide_others);
    menu.addItem(&create_menu_item(
        mtm,
        "Show All",
        Some(sel!(unhideAllApplications:)),
        "",
    ));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
}

/// Drops the models of everything currently inside `menu`, submenus included.
/// The menu itself stays: it is being refilled.
fn forget_contents(menu: &NSMenu) {
    for index in 0..menu.numberOfItems() {
        let Some(item) = menu.itemAtIndex(index) else {
            continue;
        };
        let tag = item.tag();
        STATE.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.items.remove(&tag);
            }
        });
        if let Some(sub_menu) = item.submenu() {
            forget_contents(&sub_menu);
            STATE.with(|state| {
                if let Some(state) = state.borrow_mut().as_mut() {
                    state
                        .owners
                        .retain(|(owned, _, _)| !std::ptr::eq(&**owned, &*sub_menu));
                }
            });
        }
    }
}
