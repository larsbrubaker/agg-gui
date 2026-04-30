//! Build-time native shell resources for `demo-native`.
//!
//! The Rust app sets a `winit` window icon at runtime, but Windows also needs
//! an executable resource so Explorer, Alt-Tab, and pinned shortcuts do not
//! fall back to a generic application icon.

fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/app-icon.ico")
            .compile()
            .expect("embed Windows application icon");
    }
}
