use crate::{
    error::CaptureError,
    types::{CaptureTarget, DisplayInfo, Frame, WindowInfo},
    ScreenCapturer,
};

/// In-memory capturer used by tests. Never touches platform APIs.
pub struct MockCapturer {
    pub displays: Vec<DisplayInfo>,
    pub windows: Vec<WindowInfo>,
    pub frame: Frame,
}

impl MockCapturer {
    pub fn new(displays: Vec<DisplayInfo>, windows: Vec<WindowInfo>, frame: Frame) -> Self {
        Self {
            displays,
            windows,
            frame,
        }
    }
}

impl ScreenCapturer for MockCapturer {
    fn displays(&self) -> Result<Vec<DisplayInfo>, CaptureError> {
        Ok(self.displays.clone())
    }

    fn windows(&self) -> Result<Vec<WindowInfo>, CaptureError> {
        Ok(self.windows.clone())
    }

    fn capture(&self, target: CaptureTarget) -> Result<Frame, CaptureError> {
        match target {
            CaptureTarget::Display(id) if !self.displays.iter().any(|d| d.id == id) => {
                Err(CaptureError::TargetNotFound(format!("display {id}")))
            }
            CaptureTarget::Window(id) if !self.windows.iter().any(|w| w.id == id) => {
                Err(CaptureError::TargetNotFound(format!("window {id}")))
            }
            _ => Ok(self.frame.clone()),
        }
    }
}
