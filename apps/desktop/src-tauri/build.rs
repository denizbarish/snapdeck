fn main() {
    // `snapdeck-capture` links a Swift bridge that needs the Swift concurrency
    // runtime at load time. Cargo only forwards a build script's link args to
    // the *direct* dependents of that script's package, so the `-rpath` that
    // `screencapturekit` emits reaches `snapdeck-capture` but not this binary,
    // and the app aborts at startup with
    // "Library not loaded: @rpath/libswift_Concurrency.dylib".
    // `/usr/lib/swift` is served from the dyld shared cache, not from disk.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }

    tauri_build::build()
}
