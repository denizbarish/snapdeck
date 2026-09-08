#![cfg(target_os = "macos")]

use snapdeck_capture::{macos::MacCapturer, CaptureTarget, Rect, ScreenCapturer};

/// Smoke test for the whole ScreenCaptureKit round trip: it proves a region
/// capture returns a self-consistent frame at the display's native
/// resolution. It does not prove the coordinate arithmetic, which the
/// `source_rect_for` unit tests in `macos::tests` cover with synthetic
/// multi-display geometry and no permission grant.
///
/// Requires screen recording permission; run manually with
/// `cargo test -p snapdeck-capture -- --ignored`.
#[test]
#[ignore]
fn a_region_capture_round_trip_returns_a_consistent_frame() {
    let capturer = MacCapturer::new();
    let displays = capturer.displays().expect("displays");
    let primary = displays
        .iter()
        .find(|d| d.is_primary)
        .expect("primary display");

    // Offset from the display origin so the request is not the degenerate
    // case where the local and global coordinates happen to coincide.
    let region = Rect {
        x: primary.bounds.x + 40.0,
        y: primary.bounds.y + 30.0,
        width: 100.0,
        height: 50.0,
    };
    let frame = capturer
        .capture(CaptureTarget::Region(region))
        .expect("capture");

    let scale = primary.scale_factor;
    assert_eq!(frame.width, (100.0 * scale).round() as u32);
    assert_eq!(frame.height, (50.0 * scale).round() as u32);
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
