fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // ScreenCaptureKit's Swift bridge links the Swift runtime, which ships with
    // macOS in /usr/lib/swift. Dependencies cannot add rpaths to our binary.
    if target_os == "macos" {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }

    // Windows: application icon, version info and DPI-aware manifest.
    if target_os == "windows" {
        #[cfg(windows)]
        embed_resource::compile("assets/windows/snapcap.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("embed Windows resources");
    }
    println!("cargo:rerun-if-changed=assets/windows");
    println!("cargo:rerun-if-changed=build.rs");
}
