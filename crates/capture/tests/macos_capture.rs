#![cfg(target_os = "macos")]

use snapdeck_capture::{macos::MacCapturer, CaptureTarget, Rect, ScreenCapturer};

/// Requires screen recording permission; run manually with
/// `cargo test -p snapdeck-capture -- --ignored`.
#[test]
#[ignore]
fn captures_a_region_at_native_resolution() {
    let capturer = MacCapturer::new();
    let displays = capturer.displays().expect("displays");
    let primary = displays
        .iter()
        .find(|d| d.is_primary)
        .expect("primary display");

    let region = Rect {
        x: primary.bounds.x,
        y: primary.bounds.y,
        width: 100.0,
        height: 50.0,
    };
    let frame = capturer
        .capture(CaptureTarget::Region(region))
        .expect("capture");

    let scale = primary.scale_factor;
    assert_eq!(frame.width, (100.0 * scale) as u32);
    assert_eq!(frame.height, (50.0 * scale) as u32);
    assert_eq!(frame.scale_factor, scale);
    assert_eq!(frame.data.len(), frame.stride * frame.height as usize);
}

#[test]
#[ignore]
fn lists_at_least_one_display_with_a_sane_scale_factor() {
    let capturer = MacCapturer::new();
    let displays = capturer.displays().expect("displays");
    assert!(!displays.is_empty());
    for d in &displays {
        assert!(
            d.scale_factor >= 1.0 && d.scale_factor <= 4.0,
            "bad scale: {}",
            d.scale_factor
        );
        assert!(!d.bounds.is_empty());
    }
}
