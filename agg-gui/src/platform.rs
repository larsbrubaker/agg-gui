//! Runtime platform conventions shared by widgets.
//!
//! Native builds default from the compiled target. WASM hosts can override this
//! after inspecting the browser's client platform so shortcuts display and match
//! the user's operating system rather than the `wasm32` compile target.

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

pub fn current_platform() -> Platform {
    platform_from_code(CURRENT_PLATFORM.load(Ordering::Relaxed))
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
}
