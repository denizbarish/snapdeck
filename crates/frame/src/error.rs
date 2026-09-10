use serde::Serialize;

#[derive(Debug, thiserror::Error, Serialize, Clone, PartialEq, Eq)]
#[serde(tag = "kind", content = "detail", rename_all = "camelCase")]
pub enum CaptureError {
    /// macOS screen recording permission is missing or was revoked.
    #[error("screen recording permission denied")]
    PermissionDenied,
    #[error("capture target not found: {0}")]
    TargetNotFound(String),
    #[error("platform capture failed: {0}")]
    Platform(String),
}
