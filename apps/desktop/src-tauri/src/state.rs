use snapdeck_capture::macos::MacCapturer;

/// Shared application state. The capturer is stateless and cheap to share.
pub struct AppState {
    /// Read by `overlay::open_overlays` and by the capture commands added in
    /// Task 10.
    pub capturer: MacCapturer,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            capturer: MacCapturer::new(),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
