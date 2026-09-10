//! Platform-independent screen capture abstraction.

pub mod mock;

pub use snapdeck_frame::{error, types};

#[cfg(target_os = "macos")]
pub mod macos;

pub use snapdeck_frame::{
    CaptureError, CaptureTarget, DisplayInfo, Frame, PixelFormat, Rect, WindowInfo,
};

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
}
