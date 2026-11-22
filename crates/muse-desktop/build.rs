fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    // Keep a generous main-thread stack for deep GPUI layout trees.
    println!("cargo:rustc-link-arg-bins=/STACK:16777216");

    // Application icon (same artwork as the Android launcher icon).
    // GPUI loads icon resource #1 for the window and taskbar; Explorer and
    // shortcuts use the same resource. The .res is prebuilt from
    // assets/app/muse.svg, so no resource compiler is needed.
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = manifest.join("../../assets/app/muse-icon.res");
    println!("cargo:rerun-if-changed={}", icon.display());
    if icon.is_file() {
        println!("cargo:rustc-link-arg-bins={}", icon.display());
    }
}
