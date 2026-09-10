//! The picture types Snapdeck passes between its crates.
//!
//! These live apart from `snapdeck-capture` because they are plain data and
//! nothing else: a crate that only reasons about pixels, such as
//! `snapdeck-stitch`, can take a `Frame` without linking a platform's capture
//! framework and the Swift runtime behind it.

pub mod error;
pub mod types;

pub use error::CaptureError;
pub use types::{CaptureTarget, DisplayInfo, Frame, PixelFormat, Rect, WindowInfo};
