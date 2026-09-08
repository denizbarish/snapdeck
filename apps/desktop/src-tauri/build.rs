fn main() {
    // The screencapturekit Swift bridge links against the Swift runtime, so
    // every binary that pulls `snapdeck-capture` in needs an rpath to it. A
    // `rustc-link-arg` applies only to the artifacts of the package whose build
    // script emitted it (its bins, cdylibs, examples, tests and benches) and is
    // not propagated to any dependent, so the rpath has to be re-declared by
    // each crate that links the bridge. Without it this app aborts before
    // `main` with `Library not loaded: @rpath/libswift_Concurrency.dylib`.
    // `crates/capture/build.rs` carries the identical line for that crate's own
    // test binaries: the duplication is deliberate, do not deduplicate it.
    // `/usr/lib/swift` is served from the dyld shared cache, not from disk.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }

    tauri_build::build()
}
