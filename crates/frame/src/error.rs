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
    /// This machine cannot do this at all: recording needs macOS 15.0, and
    /// the application itself runs on 14.0.
    ///
    /// Its own variant rather than a `Platform`, because the two ask the user
    /// for different things. A `Platform` failure is something that went
    /// wrong and might not next time; this one will never succeed on this
    /// machine, and saying so is the only useful thing to say.
    #[error("not supported on this system: {0}")]
    Unsupported(String),
}
