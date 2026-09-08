//! Platform-independent screen capture abstraction.

pub mod error;
pub mod mock;
pub mod types;

#[cfg(target_os = "macos")]
pub mod macos;

pub use error::CaptureError;
pub use types::{CaptureTarget, DisplayInfo, Frame, PixelFormat, Rect, WindowInfo};

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
