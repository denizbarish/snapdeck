use snapdeck_capture::macos::MacCapturer;

/// Shared application state. The capturer is stateless and cheap to share.
pub struct AppState {
    /// Read by the capture commands added in Task 10.
    #[allow(dead_code)]
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
