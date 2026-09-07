//! Platform-independent screen capture abstraction.

pub mod error;
pub mod mock;
pub mod types;

#[cfg(target_os = "macos")]
pub mod macos;

pub use error::CaptureError;
pub use types::{CaptureTarget, DisplayInfo, Frame, PixelFormat, Rect, WindowInfo};

/// Enumerates capture targets and produces frames from them.
pub trait ScreenCapturer {
    fn displays(&self) -> Result<Vec<DisplayInfo>, CaptureError>;
    fn windows(&self) -> Result<Vec<WindowInfo>, CaptureError>;
    fn capture(&self, target: CaptureTarget) -> Result<Frame, CaptureError>;
}
