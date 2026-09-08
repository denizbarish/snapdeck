use serde::Serialize;

/// Deep link to System Settings, Privacy and Security, Screen Recording.
pub const SETTINGS_DEEP_LINK: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionState {
    Granted,
    Denied,
}

impl PermissionState {
    pub fn from_granted(granted: bool) -> Self {
        if granted {
            Self::Granted
        } else {
            Self::Denied
        }
    }

    pub fn is_granted(&self) -> bool {
        matches!(self, Self::Granted)
    }
}

/// Checks the current permission without showing a prompt.
pub fn screen_capture_permission() -> PermissionState {
    // SAFETY: a nullary CoreGraphics query with no arguments to validate and
    // no ownership to transfer.
    PermissionState::from_granted(unsafe { CGPreflightScreenCaptureAccess() })
}

/// Triggers the system prompt the first time it is called. On later calls
/// macOS does not prompt again, so the caller must send the user to
/// `SETTINGS_DEEP_LINK` when this returns `Denied`.
pub fn request_screen_capture_permission() -> PermissionState {
    // SAFETY: a nullary CoreGraphics call with no arguments to validate and
    // no ownership to transfer. The system-wide side effect is the point.
    PermissionState::from_granted(unsafe { CGRequestScreenCaptureAccess() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_granted_maps_true_to_granted() {
        assert_eq!(
            PermissionState::from_granted(true),
            PermissionState::Granted
        );
        assert!(PermissionState::from_granted(true).is_granted());
    }

    #[test]
    fn from_granted_maps_false_to_denied() {
        assert_eq!(
            PermissionState::from_granted(false),
            PermissionState::Denied
        );
        assert!(!PermissionState::from_granted(false).is_granted());
    }

    #[test]
    fn settings_deep_link_points_at_screen_recording_pane() {
        assert_eq!(
            SETTINGS_DEEP_LINK,
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
        );
    }
}
