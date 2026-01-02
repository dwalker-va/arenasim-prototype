fn main() {
    // Enable wasm_js for getrandom when building for wasm32
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        println!("cargo:rustc-cfg=wasm_js");
    }
}

