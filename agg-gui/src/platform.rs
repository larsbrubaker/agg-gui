//! Runtime platform conventions shared by widgets.
//!
//! Native builds default from the compiled target. WASM hosts can override this
//! after inspecting the browser's client platform so shortcuts display and match
//! the user's operating system rather than the `wasm32` compile target.
//!
//! A thread can pin its own platform with [`override_platform_for_thread`]
//! (the guard restores the previous answer when dropped). Tests use it to
//! drive both the Mac and the Windows text-edit bindings from any host
//! without touching the process-wide value other threads read — C#'s
//! settable `InternalTextEditWidget.UseMacKeyBindings`, minus the
//! `[NotInParallel]` it needs to stay safe.

use std::cell::Cell;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::event::Modifiers;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    MacOS,
    Windows,
    Linux,
    Other,
}

static CURRENT_PLATFORM: AtomicU8 = AtomicU8::new(default_platform_code());

pub fn set_platform(platform: Platform) {
    CURRENT_PLATFORM.store(platform_code(platform), Ordering::Relaxed);
}

thread_local! {
    static THREAD_PLATFORM: Cell<Option<Platform>> = const { Cell::new(None) };
}

/// The platform whose conventions widgets follow: this thread's override
/// when one is in effect, otherwise the process-wide value.
pub fn current_platform() -> Platform {
    THREAD_PLATFORM
        .with(Cell::get)
        .unwrap_or_else(|| platform_from_code(CURRENT_PLATFORM.load(Ordering::Relaxed)))
}

/// Restores the thread's previous platform override when dropped; returned
/// by [`override_platform_for_thread`].
#[must_use = "the override ends when the guard is dropped"]
pub struct ThreadPlatformOverride {
    previous: Option<Platform>,
}

impl Drop for ThreadPlatformOverride {
    fn drop(&mut self) {
        THREAD_PLATFORM.with(|cell| cell.set(self.previous));
    }
}

/// Makes [`current_platform`] answer `platform` on this thread until the
/// returned guard is dropped. Other threads keep the process-wide value.
pub fn override_platform_for_thread(platform: Platform) -> ThreadPlatformOverride {
    let previous = THREAD_PLATFORM.with(|cell| cell.replace(Some(platform)));
    ThreadPlatformOverride { previous }
}

/// Whether text editing uses the Mac caret and delete bindings (Option for
/// word-wise, Command for line- and document-wise) instead of the Windows
/// ones (Control for word-wise, Control+Home/End for document-wise). C#'s
/// `InternalTextEditWidget.UseMacKeyBindings`; it follows
/// [`current_platform`], so it defaults to the running OS natively and to
/// the browser's OS on the web.
pub fn use_mac_key_bindings() -> bool {
    current_platform() == Platform::MacOS
}

pub fn primary_modifier_label() -> &'static str {
    match current_platform() {
        Platform::MacOS => "Cmd",
        Platform::Windows | Platform::Linux | Platform::Other => "Ctrl",
    }
}

pub fn command_modifier_pressed(modifiers: Modifiers) -> bool {
    match current_platform() {
        Platform::MacOS => modifiers.meta,
        Platform::Windows | Platform::Linux | Platform::Other => modifiers.ctrl,
    }
}

/// The [`Modifiers`] a user holds for the platform's command key: `meta`
/// (Cmd) on macOS, `ctrl` elsewhere.  The inverse of
/// [`command_modifier_pressed`]; tests use it to synthesize portable
/// shortcuts (e.g. a menu item declared as `"Ctrl+N"`) without hard-coding
/// one OS's modifier.
#[cfg(test)]
pub(crate) fn command_modifiers() -> Modifiers {
    match current_platform() {
        Platform::MacOS => Modifiers {
            meta: true,
            ..Modifiers::default()
        },
        Platform::Windows | Platform::Linux | Platform::Other => Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
    }
}

pub fn command_modifier_released(modifiers: Modifiers) -> bool {
    !modifiers.ctrl && !modifiers.meta
}

pub fn platform_from_name(name: &str) -> Platform {
    let name = name.to_ascii_lowercase();
    if name.contains("mac")
        || name.contains("darwin")
        || name.contains("iphone")
        || name.contains("ipad")
    {
        Platform::MacOS
    } else if name.contains("win") {
        Platform::Windows
    } else if name.contains("linux")
        || name.contains("x11")
        || name.contains("ubuntu")
        || name.contains("fedora")
        || name.contains("android")
    {
        Platform::Linux
    } else {
        Platform::Other
    }
}

const fn default_platform_code() -> u8 {
    if cfg!(target_os = "macos") {
        platform_code(Platform::MacOS)
    } else if cfg!(target_os = "windows") {
        platform_code(Platform::Windows)
    } else if cfg!(target_os = "linux") {
        platform_code(Platform::Linux)
    } else {
        platform_code(Platform::Other)
    }
}

const fn platform_code(platform: Platform) -> u8 {
    match platform {
        Platform::MacOS => 1,
        Platform::Windows => 2,
        Platform::Linux => 3,
        Platform::Other => 4,
    }
}

fn platform_from_code(code: u8) -> Platform {
    match code {
        1 => Platform::MacOS,
        2 => Platform::Windows,
        3 => Platform::Linux,
        _ => Platform::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_client_platform_names() {
        assert_eq!(platform_from_name("macOS"), Platform::MacOS);
        assert_eq!(platform_from_name("Win32"), Platform::Windows);
        assert_eq!(platform_from_name("Linux x86_64"), Platform::Linux);
        assert_eq!(platform_from_name("unknown"), Platform::Other);
    }

    #[test]
    fn thread_override_is_scoped_and_nests() {
        let global = current_platform();
        {
            let _mac = override_platform_for_thread(Platform::MacOS);
            assert!(use_mac_key_bindings());
            {
                let _win = override_platform_for_thread(Platform::Windows);
                assert_eq!(current_platform(), Platform::Windows);
                assert!(!use_mac_key_bindings());
                // another thread still reads the process-wide value
                let other = std::thread::spawn(current_platform).join().unwrap();
                assert_eq!(other, global);
            }
            assert_eq!(current_platform(), Platform::MacOS);
        }
        assert_eq!(current_platform(), global);
    }
}
