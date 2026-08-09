fn main() {
    // Enable wasm_js for getrandom when building for wasm32
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        println!("cargo:rustc-cfg=wasm_js");
    }

    embed_windows_icon();
}

/// Compile the application icon into the Windows executable.
///
/// Windows reads an executable's icon from an embedded resource rather than
/// from a file beside it, so this is what puts the icon on the .exe in Explorer
/// and on the taskbar. macOS takes the opposite approach — the icon is a file
/// inside the .app bundle, named by packaging/macos/Info.plist — so there is
/// nothing to do here for that platform.
///
/// Gated on the HOST rather than the target, matching the `cfg(windows)`
/// build-dependency in Cargo.toml: build-dependency target tables resolve
/// against the host, so the crate simply does not exist on a macOS or Linux
/// build. The release workflow builds Windows on a Windows runner, where host
/// and target agree.
#[cfg(windows)]
fn embed_windows_icon() {
    println!("cargo:rerun-if-changed=packaging/windows/icon.ico");

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("packaging/windows/icon.ico");
    if let Err(e) = resource.compile() {
        // Not fatal: an iconless build is still a working game, and failing the
        // build over cosmetics would be a poor trade.
        println!("cargo:warning=could not embed the Windows icon: {e}");
    }
}

#[cfg(not(windows))]
fn embed_windows_icon() {}

