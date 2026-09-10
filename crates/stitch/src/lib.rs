//! Joining a run of overlapping frames into one picture.
//!
//! Nothing here reads a screen, a window or a file: the input is pixels and
//! the output is pixels, which is what lets every case be built by hand in a
//! test.

pub mod align;
pub mod error;

pub use align::{align, Alignment, RowSignatures, ScrollAxis, ScrollHint};
pub use error::StitchError;
