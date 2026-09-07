use std::time::SystemTime;

use snapdeck_capture::{
    mock::MockCapturer, CaptureError, CaptureTarget, DisplayInfo, Frame, PixelFormat, Rect,
    ScreenCapturer,
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

#[test]
fn capture_unknown_display_reports_target_not_found() {
    let display = DisplayInfo {
        id: 1,
        bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        },
        scale_factor: 2.0,
        is_primary: true,
    };
    let capturer = MockCapturer::new(vec![display], vec![], frame());

    let err = capturer.capture(CaptureTarget::Display(99)).unwrap_err();

    assert_eq!(err, CaptureError::TargetNotFound("display 99".to_string()));
}
