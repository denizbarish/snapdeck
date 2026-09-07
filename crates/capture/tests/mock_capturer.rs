use std::time::SystemTime;

use snapdeck_capture::{
    mock::MockCapturer, CaptureError, CaptureTarget, DisplayInfo, Frame, PixelFormat, Rect,
    ScreenCapturer, WindowInfo,
};

fn frame() -> Frame {
    Frame {
        data: vec![0, 0, 0, 255],
        width: 1,
        height: 1,
        stride: 4,
        pixel_format: PixelFormat::Rgba8,
        scale_factor: 1.0,
        captured_at: SystemTime::now(),
    }
}

fn display() -> DisplayInfo {
    DisplayInfo {
        id: 1,
        bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        },
        scale_factor: 2.0,
        is_primary: true,
    }
}

fn window() -> WindowInfo {
    WindowInfo {
        id: 7,
        title: Some("Editor".to_string()),
        app_name: Some("Snapdeck".to_string()),
        bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        },
        layer: 0,
        is_on_screen: true,
    }
}

#[test]
fn capture_unknown_display_reports_target_not_found() {
    let capturer = MockCapturer::new(vec![display()], vec![], frame());

    let err = capturer.capture(CaptureTarget::Display(99)).unwrap_err();

    assert_eq!(err, CaptureError::TargetNotFound("display 99".to_string()));
}

#[test]
fn capture_unknown_window_reports_target_not_found() {
    let capturer = MockCapturer::new(vec![display()], vec![window()], frame());

    let err = capturer.capture(CaptureTarget::Window(42)).unwrap_err();

    assert_eq!(err, CaptureError::TargetNotFound("window 42".to_string()));
}

#[test]
fn injected_error_fails_every_capture() {
    // Consumers must be able to exercise the permission-denied path, which
    // never yields an empty or black frame.
    let capturer = MockCapturer::new(vec![display()], vec![window()], frame())
        .with_error(CaptureError::PermissionDenied);

    assert_eq!(
        capturer.capture(CaptureTarget::Display(1)).unwrap_err(),
        CaptureError::PermissionDenied
    );
    assert_eq!(
        capturer.capture(CaptureTarget::Window(7)).unwrap_err(),
        CaptureError::PermissionDenied
    );
}
