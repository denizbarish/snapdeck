//! Platform-independent screen capture abstraction.

use std::path::Path;

pub mod mock;

pub use snapdeck_frame::{error, types};

#[cfg(target_os = "macos")]
pub mod macos;

pub use snapdeck_frame::{
    AudioSources, CaptureError, CaptureTarget, DisplayInfo, Frame, PixelFormat, RecordingProgress,
    RecordingSummary, Rect, WindowInfo,
};

/// A recording that has started and is writing to a file.
///
/// `Send` because the session lives in the application's shared state and is
/// stopped from a different thread than the one that started it. Every method
/// blocks the calling thread for a platform round trip, exactly as
/// `ScreenCapturer`'s do.
pub trait Recording: Send {
    /// What the stream has delivered so far. Cheap: two atomic loads.
    fn progress(&self) -> RecordingProgress;

    /// Finishes the movie and answers with what was written.
    ///
    /// Consumes the recording, because a stream taken apart cannot be
    /// restarted and a handle that outlived its stream is a handle whose every
    /// method is a lie.
    fn stop(self: Box<Self>) -> Result<RecordingSummary, CaptureError>;

    /// Finishes the movie and throws the result away.
    ///
    /// The file is **not** deleted here. This crate was handed a path and does
    /// not own the folder it is in; the caller that chose the name is the one
    /// that takes it back. The movie is still finalised first, because an
    /// unfinalised file can still be written to after this returns.
    fn cancel(self: Box<Self>) -> Result<(), CaptureError>;
}

/// Enumerates capture targets and produces frames from them.
///
/// Every method blocks the calling thread for the whole of a platform round
/// trip: enumerating displays or windows waits on a system query, and a
/// capture waits on the compositor. Call them from a worker thread, never
/// from a UI thread such as the Tauri main thread.
pub trait ScreenCapturer {
    /// Blocks the calling thread; see the trait documentation.
    fn displays(&self) -> Result<Vec<DisplayInfo>, CaptureError>;
    /// Blocks the calling thread; see the trait documentation.
    fn windows(&self) -> Result<Vec<WindowInfo>, CaptureError>;
    /// Blocks the calling thread; see the trait documentation.
    fn capture(&self, target: CaptureTarget) -> Result<Frame, CaptureError>;

    /// Whether this capturer can record at all on this machine.
    ///
    /// Asked before a recording is offered, so the answer can be a disabled
    /// menu item rather than a failure the user runs into. A default of
    /// `false` for the same reason `record` has one: a capturer that has not
    /// said otherwise cannot, and the caller learns that without knowing which
    /// framework would have been asked.
    fn can_record(&self) -> bool {
        false
    }

    /// Starts recording `target` into `output`, which must not already exist.
    ///
    /// Blocks the calling thread for a platform round trip; see the trait
    /// documentation.
    ///
    /// A default body rather than a required method, and that is the whole of
    /// why the v1 design's `stream()` became this: every existing
    /// implementation, `mock.rs` included, compiles unchanged, and a platform
    /// with no recording says so out loud instead of doing nothing quietly.
    ///
    /// `audio` says which sound goes into the same file as the picture.
    fn record(
        &self,
        target: CaptureTarget,
        audio: AudioSources,
        output: &Path,
    ) -> Result<Box<dyn Recording>, CaptureError> {
        let _ = (target, audio, output);
        Err(CaptureError::Unsupported(
            "this platform cannot record the screen".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::SystemTime;

    use super::*;
    use crate::mock::MockCapturer;

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
    fn a_capturer_that_did_not_override_record_says_it_cannot() {
        // Asked before a recording is offered, so a machine that cannot record
        // shows a disabled menu item instead of a failure someone runs into.
        assert!(!MockCapturer::new(vec![display()], vec![window()], frame()).can_record());
    }

    #[test]
    fn a_capturer_that_did_not_override_record_refuses_out_loud() {
        let capturer = MockCapturer::new(vec![display()], vec![window()], frame());

        // `Box<dyn Recording>` is not `Debug`, so the success arm is named
        // here rather than unwrapped.
        match capturer.record(
            CaptureTarget::Display(1),
            AudioSources::default(),
            Path::new("/tmp/snapdeck.mp4"),
        ) {
            Err(CaptureError::Unsupported(message)) => assert!(
                message.contains("record"),
                "the refusal has to name what was refused, got {message:?}"
            ),
            Err(other) => panic!("expected Unsupported, got {other:?}"),
            Ok(_) => panic!("the default body must refuse rather than do nothing"),
        }
    }

    #[test]
    fn the_mock_capturer_answers_exactly_what_it_answered_before_record_existed() {
        // The point of a default body is that `mock.rs` did not have to
        // change, so the claim worth holding is that its answers did not
        // either. Every expectation is written out rather than read back off
        // the mock's own fields, which would agree with any change to them.
        let capturer = MockCapturer::new(vec![display()], vec![window()], frame());

        let displays = capturer.displays().expect("displays");
        assert_eq!(displays.len(), 1);
        assert_eq!(displays[0].id, 1);
        assert_eq!(displays[0].scale_factor, 2.0);
        assert!(displays[0].is_primary);

        let windows = capturer.windows().expect("windows");
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].id, 7);
        assert_eq!(windows[0].title.as_deref(), Some("Editor"));
        assert_eq!(windows[0].app_name.as_deref(), Some("Snapdeck"));

        let captured = capturer
            .capture(CaptureTarget::Display(1))
            .expect("a known display captures");
        assert_eq!(captured.width, 1);
        assert_eq!(captured.height, 1);
        assert_eq!(captured.stride, 4);
        assert_eq!(captured.data, vec![0, 0, 0, 255]);

        assert_eq!(
            capturer.capture(CaptureTarget::Display(99)).unwrap_err(),
            CaptureError::TargetNotFound("display 99".to_string())
        );
        assert_eq!(
            capturer.capture(CaptureTarget::Window(42)).unwrap_err(),
            CaptureError::TargetNotFound("window 42".to_string())
        );
    }

    #[test]
    fn an_unsupported_error_reaches_the_frontend_under_the_same_rule_as_the_rest() {
        assert_eq!(
            serde_json::to_string(&CaptureError::Unsupported("x".to_string()))
                .expect("a capture error serialises"),
            r#"{"kind":"unsupported","detail":"x"}"#
        );
    }
}
