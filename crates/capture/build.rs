fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // The screencapturekit Swift bridge links against the Swift runtime, so
    // every binary that pulls this crate in needs an rpath to it. Cargo does
    // not propagate a dependency's `rustc-link-arg` to downstream artifacts,
    // so the rpath has to be re-declared by each crate that links the bridge.
    // Without it the test binaries abort at load with
    // `Library not loaded: @rpath/libswift_Concurrency.dylib`.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
