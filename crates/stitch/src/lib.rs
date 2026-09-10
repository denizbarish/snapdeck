//! Joining a run of overlapping frames into one picture.
//!
//! Nothing here reads a screen, a window or a file: the input is pixels and
//! the output is pixels, which is what lets every case be built by hand in a
//! test.

pub mod align;
pub mod compose;
pub mod error;

use snapdeck_frame::Frame;

pub use align::{align, Alignment, RowSignatures, ScrollAxis, ScrollHint};
pub use compose::{compose, Image, Tile, MAX_STITCHED_PIXELS};
pub use error::StitchError;

/// Joins a sequence of overlapping frames into one picture.
///
/// The frames must be in scroll order and must all agree about their size and
/// their scale. Nothing here reads a screen, a window or a file: the input is
/// pixels and the output is pixels, which is what lets every case be built by
/// hand.
pub fn stitch(frames: &[Frame], hint: ScrollHint) -> Result<Image, StitchError> {
    let Some(first) = frames.first() else {
        return Err(StitchError::NoFrames);
    };
    // Measured before the frames are read, and so before a single unpacked
    // buffer is allocated: the widest frame by every frame's rows is more
    // picture than the result can possibly be, and a run past the limit is
    // refused rather than half built. Saturating, because a run that
    // overflows the count is past the limit either way.
    let widest = frames
        .iter()
        .map(|frame| frame.width)
        .max()
        .unwrap_or_default();
    let rows = frames
        .iter()
        .map(|frame| u64::from(frame.height))
        .fold(0_u64, u64::saturating_add);
    let pixels = u64::from(widest).saturating_mul(rows);
    if pixels > MAX_STITCHED_PIXELS {
        return Err(StitchError::TooLarge {
            pixels,
            limit: MAX_STITCHED_PIXELS,
        });
    }

    if frames.len() == 1 {
        // One frame is already the whole picture. There is no pair to align,
        // and asking `align` about a frame and itself would answer that
        // nothing scrolled.
        return Ok(Image {
            data: tightly_packed(first, 0)?,
            width: first.width,
            height: first.height,
            scale_factor: first.scale_factor,
        });
    }

    for (index, frame) in frames.iter().enumerate().skip(1) {
        if frame.width != first.width || frame.height != first.height {
            return Err(StitchError::MismatchedFrames(format!(
                "frame {index} is {}x{}, the first is {}x{}",
                frame.width, frame.height, first.width, first.height
            )));
        }
        // A run of mixed scales would paste without complaint and hand back a
        // picture whose points meant two different things.
        if frame.scale_factor != first.scale_factor {
            return Err(StitchError::MismatchedFrames(format!(
                "frame {index} was measured at {}x, the first at {}x",
                frame.scale_factor, first.scale_factor
            )));
        }
    }

    let mut buffers = Vec::with_capacity(frames.len());
    for (index, frame) in frames.iter().enumerate() {
        buffers.push(tightly_packed(frame, index)?);
    }
    let mut signatures = Vec::with_capacity(buffers.len());
    for buffer in &buffers {
        signatures.push(RowSignatures::of(buffer, first.width, first.height)?);
    }

    let mut overlaps = Vec::with_capacity(signatures.len() - 1);
    for index in 1..signatures.len() {
        let alignment = align(&signatures[index - 1], &signatures[index], hint)
            .map_err(|error| place_in_run(error, index))?;
        overlaps.push(alignment.overlap);
    }

    let tiles: Vec<Tile<'_>> = buffers
        .iter()
        .map(|rgba| Tile {
            rgba,
            width: first.width,
            height: first.height,
        })
        .collect();

    Ok(Image {
        scale_factor: first.scale_factor,
        ..compose(&tiles, &overlaps)?
    })
}

/// A frame's pixels, tightly packed, with the capture crate's complaint about
/// a frame that contradicts itself restated as this crate's.
fn tightly_packed(frame: &Frame, index: usize) -> Result<Vec<u8>, StitchError> {
    frame.to_rgba8().map_err(|error| {
        StitchError::MismatchedFrames(format!("frame {index} cannot be read: {error}"))
    })
}

/// Names the frame a failed alignment was about.
///
/// `align` compares two frames and so cannot know where they sit in a run: it
/// answers `NoOverlap { index: 0 }` whichever pair it was handed. The walk
/// over the sequence is the only side that knows, so it says so, and a caller
/// can tell the user where the page came apart.
fn place_in_run(error: StitchError, index: usize) -> StitchError {
    match error {
        StitchError::NoOverlap { .. } => StitchError::NoOverlap { index },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::tests::{page, window};
    use snapdeck_frame::PixelFormat;
    use std::time::SystemTime;

    const WIDTH: u32 = 16;
    const FRAME_HEIGHT: u32 = 200;
    /// How far the page moved between each pair of frames in the end to end
    /// case. All three are under `FRAME_HEIGHT` and over the default
    /// `min_shift`, so every pair really does overlap.
    const SHIFTS: [u32; 3] = [60, 45, 70];
    const SCALE: f32 = 2.0;
    /// Far enough down the page that no row of it resembles a row near the
    /// top, so two frames cut this far apart share nothing.
    const UNRELATED_OFFSET: u32 = 9_000;
    /// Two frames of this shape hold far more than `MAX_STITCHED_PIXELS`.
    const HUGE_SIDE: u32 = 20_000;

    fn frame(rgba: Vec<u8>, width: u32, height: u32, scale_factor: f32) -> Frame {
        Frame {
            data: rgba,
            width,
            height,
            stride: width as usize * 4,
            pixel_format: PixelFormat::Rgba8,
            scale_factor,
            captured_at: SystemTime::UNIX_EPOCH,
        }
    }

    /// A frame cut out of `source` at row `offset`.
    fn cut(source: &[u8], offset: u32) -> Frame {
        frame(
            window(source, WIDTH, offset, FRAME_HEIGHT),
            WIDTH,
            FRAME_HEIGHT,
            SCALE,
        )
    }

    /// The absolute row each frame of the end to end case starts at.
    fn offsets() -> Vec<u32> {
        let mut offsets = vec![0];
        for shift in SHIFTS {
            let previous = offsets.last().copied().unwrap_or_default();
            offsets.push(previous + shift);
        }
        offsets
    }

    // N4
    #[test]
    fn a_lone_frame_is_returned_as_it_came() {
        let source = page(WIDTH, FRAME_HEIGHT);
        let frames = [cut(&source, 0)];

        let image = stitch(&frames, ScrollHint::default()).expect("a lone frame needs no aligning");

        assert_eq!(image.width, WIDTH);
        assert_eq!(image.height, FRAME_HEIGHT);
        assert_eq!(image.scale_factor, SCALE);
        assert_eq!(image.data, source);
    }

    // N5
    #[test]
    fn an_empty_run_is_an_error_not_a_panic() {
        assert_eq!(
            stitch(&[], ScrollHint::default()),
            Err(StitchError::NoFrames)
        );
    }

    // N6
    #[test]
    fn four_frames_rebuild_the_page_they_were_cut_from() {
        let offsets = offsets();
        let source_height = offsets.last().copied().unwrap_or_default() + FRAME_HEIGHT;
        let source = page(WIDTH, source_height);
        let frames: Vec<Frame> = offsets.iter().map(|offset| cut(&source, *offset)).collect();

        let image = stitch(&frames, ScrollHint::default()).expect("consecutive frames overlap");

        assert_eq!(image.width, WIDTH);
        assert_eq!(image.height, source_height);
        assert_eq!(image.scale_factor, SCALE);
        assert_eq!(image.data, source);
    }

    // N7
    #[test]
    fn frames_measured_at_different_scales_are_refused() {
        let source = page(WIDTH, FRAME_HEIGHT + SHIFTS[0]);
        let first = cut(&source, 0);
        let retina = frame(
            window(&source, WIDTH, SHIFTS[0], FRAME_HEIGHT),
            WIDTH,
            FRAME_HEIGHT,
            SCALE * 2.0,
        );

        assert!(matches!(
            stitch(&[first, retina], ScrollHint::default()),
            Err(StitchError::MismatchedFrames(_))
        ));
    }

    // N8
    #[test]
    fn a_run_too_large_is_refused_before_its_frames_are_read() {
        // Both too large and misshapen: the frames disagree about their width
        // and their height, and neither carries the pixels it claims to. What
        // comes back says which of the two was measured first.
        let frames = [
            frame(Vec::new(), HUGE_SIDE, HUGE_SIDE, SCALE),
            frame(Vec::new(), HUGE_SIDE - 1_000, HUGE_SIDE + 1_000, SCALE),
        ];

        assert_eq!(
            stitch(&frames, ScrollHint::default()),
            Err(StitchError::TooLarge {
                pixels: u64::from(HUGE_SIDE) * u64::from(HUGE_SIDE + HUGE_SIDE + 1_000),
                limit: MAX_STITCHED_PIXELS,
            })
        );
    }

    #[test]
    fn a_lone_frame_too_large_is_refused_as_well() {
        // A single frame takes the shortcut that skips alignment, but not the
        // one that skips the limit: unpacking it allocates as much memory as
        // any run of the same size would.
        let frames = [frame(Vec::new(), HUGE_SIDE, HUGE_SIDE, SCALE)];

        assert_eq!(
            stitch(&frames, ScrollHint::default()),
            Err(StitchError::TooLarge {
                pixels: u64::from(HUGE_SIDE) * u64::from(HUGE_SIDE),
                limit: MAX_STITCHED_PIXELS,
            })
        );
    }

    // N9
    #[test]
    fn a_frame_that_shares_nothing_is_named_by_its_place_in_the_run() {
        let source = page(WIDTH, UNRELATED_OFFSET + SHIFTS[1] + FRAME_HEIGHT);
        let frames = [
            cut(&source, 0),
            cut(&source, SHIFTS[0]),
            cut(&source, UNRELATED_OFFSET),
            cut(&source, UNRELATED_OFFSET + SHIFTS[1]),
        ];

        assert_eq!(
            stitch(&frames, ScrollHint::default()),
            Err(StitchError::NoOverlap { index: 2 })
        );
    }
}
