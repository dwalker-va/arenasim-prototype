fn main() {
    // Declare this script's inputs.
    //
    // A build script that prints no `rerun-if-*` directive at all opts into
    // Cargo's documented fallback of treating EVERY file in the package as an
    // input to it. On a non-Windows host this script printed none, so editing
    // a test, a doc or an asset reran it, which invalidated the crate, which
    // rebuilt all 52 test binaries — a documented zero-rebuild `.ron` retune
    // loop cost a full rebuild.
    //
    // The line matters for being PRESENT more than for the path it names:
    // Cargo already reruns the script whenever build.rs itself changes,
    // because the recompiled script binary is a dependency of the run. What
    // the directive buys is switching the fallback off.
    println!("cargo:rerun-if-changed=build.rs");

    // Enable wasm_js for getrandom when building for wasm32.
    //
    // This needs no `rerun-if-env-changed=CARGO_CFG_TARGET_ARCH` to stay
    // correct across a target switch, and adding one would imply a hazard that
    // does not exist. Cargo runs and fingerprints a build script once PER
    // TARGET, into a per-target output directory, so a
    // `--target wasm32-unknown-unknown` build runs this script again and reads
    // its own output — it cannot inherit the host build's cfg, or fail to see
    // the wasm one.
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
