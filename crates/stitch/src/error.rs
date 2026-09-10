use serde::Serialize;

#[derive(Debug, thiserror::Error, Serialize, Clone, PartialEq, Eq)]
#[serde(tag = "kind", content = "detail", rename_all = "camelCase")]
pub enum StitchError {
    #[error("there is nothing to stitch")]
    NoFrames,
    #[error("the frames disagree about their shape: {0}")]
    MismatchedFrames(String),
    #[error("no overlap was found between frame {index} and the one before it")]
    NoOverlap { index: usize },
    #[error("the stitched picture would hold {pixels} pixels, past the limit of {limit}")]
    TooLarge { pixels: u64, limit: u64 },
}
