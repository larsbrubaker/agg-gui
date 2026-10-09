//! A native application menu bar described as data: [`MenuBarModel`] and its
//! [`MenuItemModel`]s, the decisions a native menu makes from them (which
//! standard Command chord a role carries, which items make it into a built
//! menu, which item a chord belongs to), and [`install`], which makes a model
//! the process's menu bar.
//!
//! Port of agg-sharp `Gui/Menu/MenuItemModel.cs` (the model) and the
//! decision half of `PlatformMac/mac/MacMenuBar.cs`. The AppKit half lives in
//! `macos.rs` and is compiled on macOS only; on every other platform
//! [`install`] does nothing and the app's in-window menu is the only one.
//!
//! Everything a model carries is a closure run when a menu opens or an item
//! is picked, never when the model is built: a native menu rebuilds itself
//! from the model each time it is about to be shown, so recent-file lists and
//! gates answer for the moment they are looked at.
//!
//! Picked items do not run inline. AppKit sends an item's action from inside
//! its menu-tracking loop, so the action is queued here and run by the shell
//! loop's next `about_to_wait` ([`run_pending_activations`]), on the UI thread,
//! on a clean stack, with a redraw requested afterwards.

#[cfg(target_os = "macos")]
mod macos;

use std::cell::RefCell;
use std::rc::Rc;

/// What a native menu reads to give an item its conventional shortcut and
/// position (agg-sharp `MenuItemRole`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MenuItemRole {
    #[default]
    None,
    About,
    Settings,
    Quit,
    OpenFile,
    Help,
}

impl MenuItemRole {
    /// Every role, in declaration order (C# `Enum.GetValues<MenuItemRole>()`).
    pub const ALL: [MenuItemRole; 6] = [
        MenuItemRole::None,
        MenuItemRole::About,
        MenuItemRole::Settings,
        MenuItemRole::Quit,
        MenuItemRole::OpenFile,
        MenuItemRole::Help,
    ];
}

/// One entry of a native menu (agg-sharp `MenuItemModel`): a command, a
/// divider, or a submenu whose rows are gathered each time it opens.
#[derive(Clone, Default)]
pub struct MenuItemModel {
    pub text: String,
    /// Hover help. Carried for the in-window menu; a native menu bar does not
    /// show it (agg-sharp `MacMenuBar` sets no tooltips).
    pub tool_tip_text: Option<String>,
    pub role: MenuItemRole,
    pub is_separator: bool,
    /// Asked each time a menu is built; `None` means visible. A hidden item is
    /// left out of the menu, not added and hidden.
    pub is_visible: Option<Rc<dyn Fn() -> bool>>,
    /// `None` means enabled. A disabled item is drawn greyed and its chord
    /// does not fire.
    pub is_enabled: Option<Rc<dyn Fn() -> bool>>,
    /// A check mark beside the item, when present.
    pub is_checked: Option<Rc<dyn Fn() -> bool>>,
    /// The submenu's rows, gathered when the submenu is about to be shown.
    pub sub_menu_items: Option<Rc<dyn Fn() -> Vec<MenuItemModel>>>,
    /// What picking the item does.
    pub action: Option<Rc<dyn Fn()>>,
}

impl MenuItemModel {
    /// A plain item with `text` and nothing else.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    /// A divider.
    pub fn separator() -> Self {
        Self {
            is_separator: true,
            ..Self::default()
        }
    }
}

/// The top level menus of a native menu bar, in display order (agg-sharp
/// `MenuBarModel`). By convention the first is the application menu, which the
/// mac titles with the product name and where About, Settings and Quit live.
#[derive(Clone, Default)]
pub struct MenuBarModel {
    pub menus: Vec<MenuItemModel>,
}

/// AppKit's `NSEventModifierFlags` bits, as [`match_key_equivalent`] reads
/// them (agg-sharp `AppKitConstants`).
pub mod modifier_flags {
    pub const CAPS_LOCK: u64 = 1 << 16;
    pub const SHIFT: u64 = 1 << 17;
    pub const CONTROL: u64 = 1 << 18;
    pub const OPTION: u64 = 1 << 19;
    pub const COMMAND: u64 = 1 << 20;
}

/// The standard Command chord a role carries, or `""` for a role that has
/// none. An empty key equivalent is what `NSMenuItem` wants for "none".
pub fn key_equivalent_for(role: MenuItemRole) -> &'static str {
    match role {
        MenuItemRole::Settings => ",",
        MenuItemRole::Quit => "q",
        MenuItemRole::OpenFile => "o",
        _ => "",
    }
}

/// The items whose visibility gate passes, asked now. Hidden items are left
/// out of a built menu rather than added and hidden, because the menu is
/// rebuilt from the model every time it opens.
pub fn visible_items(items: Option<&[MenuItemModel]>) -> Vec<MenuItemModel> {
    items
        .unwrap_or_default()
        .iter()
        .filter(|item| item.is_visible.as_ref().is_none_or(|gate| gate()))
        .cloned()
        .collect()
}

/// Whether `item`'s enabled gate passes. No gate means enabled.
pub fn is_enabled(item: &MenuItemModel) -> bool {
    item.is_enabled.as_ref().is_none_or(|gate| gate())
}

/// The entries that can become menu-bar menus: visible, and not a separator.
/// The menu bar has no dividers to draw, and an entry that asked to be one
/// would otherwise come out as a blank, unopenable gap. A disabled menu stays:
/// it is drawn greyed, not dropped.
pub fn top_level_menus(menus: &[MenuItemModel]) -> Vec<MenuItemModel> {
    visible_items(Some(menus))
        .into_iter()
        .filter(|menu| !menu.is_separator)
        .collect()
}

/// The children `container` has now: its provider run now (every call, so a
/// list that grew while the application ran opens showing what it grew into),
/// filtered by the visibility gates as they answer now. A leaf has none, and
/// so does a provider that came back empty: any "nothing here" placeholder is
/// the model's to supply.
pub fn children_of(container: &MenuItemModel) -> Vec<MenuItemModel> {
    match &container.sub_menu_items {
        Some(provider) => visible_items(Some(&provider())),
        None => Vec::new(),
    }
}

/// Whether a modifier word is Command and nothing else. Caps lock, Fn and the
/// numeric-pad bit are not part of a chord's identity.
fn is_plain_command_chord(flags: u64) -> bool {
    use modifier_flags::{COMMAND, CONTROL, OPTION, SHIFT};
    (flags & (COMMAND | SHIFT | CONTROL | OPTION)) == COMMAND
}

/// Whether any role carries `chord` (case-insensitively): [`key_equivalent_for`]
/// read backwards, so a role gaining a shortcut cannot leave this behind.
fn some_role_carries(chord: &str) -> bool {
    MenuItemRole::ALL.iter().any(|role| {
        let carried = key_equivalent_for(*role);
        !carried.is_empty() && carried.eq_ignore_ascii_case(chord)
    })
}

/// The item a key event belongs to: the first enabled, visible command directly
/// in a top-level menu (menu order, then item order) whose role carries
/// `characters_ignoring_modifiers` as a plain Command chord.
///
/// A chord no role carries is turned away before any menu is asked for its
/// contents, and the search never opens a submenu (the recent files list
/// reads the disk): one level down is also the only depth a native menu draws
/// a shortcut at, so what is shown and what can fire agree.
pub fn match_key_equivalent(
    model: Option<&MenuBarModel>,
    characters_ignoring_modifiers: &str,
    modifier_flags: u64,
) -> Option<MenuItemModel> {
    let model = model?;
    if characters_ignoring_modifiers.is_empty()
        || !is_plain_command_chord(modifier_flags)
        || !some_role_carries(characters_ignoring_modifiers)
    {
        return None;
    }
    for menu in top_level_menus(&model.menus) {
        for item in children_of(&menu) {
            // A separator has no shortcut and a submenu is opened, never run.
            if item.is_separator || item.sub_menu_items.is_some() {
                continue;
            }
            let chord = key_equivalent_for(item.role);
            if !chord.is_empty()
                && chord.eq_ignore_ascii_case(characters_ignoring_modifiers)
                && is_enabled(&item)
            {
                return Some(item);
            }
        }
    }
    None
}

thread_local! {
    /// Picked items waiting for the shell loop's next `about_to_wait`.
    static PENDING: RefCell<Vec<Rc<dyn Fn()>>> = const { RefCell::new(Vec::new()) };
}

/// Queues `item`'s action to run on the shell loop's next idle pass, and wakes
/// the loop. What a native menu does when an item is picked or its chord fires.
pub fn queue_activation(item: &MenuItemModel) {
    if let Some(action) = item.action.clone() {
        PENDING.with(|pending| pending.borrow_mut().push(action));
        agg_gui::animation::signal_async_state_change();
    }
}

/// Runs every queued activation, in the order they were picked. Returns
/// whether any ran, so the caller can redraw. The queue is taken before the
/// actions run, so an action that opens a menu cannot re-enter it.
pub fn run_pending_activations() -> bool {
    let actions = PENDING.with(|pending| std::mem::take(&mut *pending.borrow_mut()));
    let ran = !actions.is_empty();
    for action in actions {
        action();
    }
    ran
}

/// Makes `model` the application's native menu bar, replacing any bar a
/// previous call installed. Call on the UI thread; calling it from the builder
/// closure handed to [`crate::run`] is the usual place.
///
/// On macOS the bar is applied once AppKit has finished launching: winit sets
/// its own default menu in `applicationDidFinishLaunching:`, so a bar applied
/// before then would be replaced. The shell applies a model installed before
/// launch at `NewEvents(StartCause::Init)`, which winit sends right after
/// setting its default menu and before anything is drawn, so winit's menu is
/// never seen; a model installed after launch is applied at once. An app that
/// installs no model keeps winit's default menu.
///
/// Returns whether a native bar is (or will be, once launch finishes)
/// installed: always `false` off macOS, where the app's own in-window menu is
/// the only one, and `false` when called off the main thread.
pub fn install(model: MenuBarModel) -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::install(model)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = model;
        false
    }
}

/// Told by the shell loop that the platform has finished launching
/// (`NewEvents(StartCause::Init)`): applies a model [`install`]ed before then.
pub(crate) fn finish_launching() {
    #[cfg(target_os = "macos")]
    macos::finish_launching();
}

/// A plain-text read-back of the menu bar the platform is showing now, for
/// diagnostics: on macOS the process and application names AppKit titles the
/// application menu with, then `-[NSApp mainMenu]`'s top-level titles, each
/// followed by its submenu's items (indented, with their key equivalents).
/// Empty off macOS.
pub fn describe_main_menu() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        macos::describe_main_menu()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}
