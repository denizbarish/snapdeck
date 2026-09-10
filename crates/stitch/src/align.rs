//! Finding how far one frame scrolled past the frame before it.

use crate::error::StitchError;

/// Bytes per pixel in a tightly packed RGBA8 buffer.
const CHANNELS: usize = 4;
/// The leading channels of a pixel that carry picture. Alpha is left out: it
/// describes coverage, not brightness, and a constant opaque alpha would only
/// flatten the contrast the correlation lives on.
const COLOUR_CHANNELS: usize = 3;

const DEFAULT_MIN_SHIFT: u32 = 8;
const DEFAULT_MIN_SCORE: f32 = 0.90;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAxis {
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollHint {
    pub axis: ScrollAxis,
    /// The smallest displacement worth considering, in pixels. Below it two
    /// frames are the same picture and the scroll did nothing.
    pub min_shift: u32,
    /// The correlation a match has to reach to count, 0.0 to 1.0.
    pub min_score: f32,
}

impl Default for ScrollHint {
    /// Vertical, 8 pixels, 0.90.
    fn default() -> Self {
        Self {
            axis: ScrollAxis::Vertical,
            min_shift: DEFAULT_MIN_SHIFT,
            min_score: DEFAULT_MIN_SCORE,
        }
    }
}

/// One row reduced to the numbers a vertical alignment needs.
///
/// A row rather than a pixel, because the displacement being searched for is
/// one-dimensional: a vertical scroll moves every row by the same amount, so
/// the whole width collapses into one number per row without losing the
/// quantity being measured.
pub struct RowSignatures {
    /// Kept so two runs of signatures can be told apart by the shape they
    /// came from; the signatures themselves no longer remember it.
    width: u32,
    /// Mean colour of each row, top to bottom.
    rows: Vec<f32>,
}

impl RowSignatures {
    /// Signatures for a tightly packed RGBA8 image.
    ///
    /// Frames arrive from a browser or a compositor, so the buffer is
    /// measured against the shape it claims rather than trusted: a short
    /// buffer is an error here instead of a panic further down.
    pub fn of(rgba: &[u8], width: u32, height: u32) -> Result<Self, StitchError> {
        let columns = width as usize;
        if columns == 0 {
            return Err(StitchError::MismatchedFrames(format!(
                "a frame {width} pixels wide has no rows to sign"
            )));
        }
        let row_bytes = columns.checked_mul(CHANNELS).ok_or_else(|| {
            StitchError::MismatchedFrames(format!("a row of {width} pixels cannot be addressed"))
        })?;
        let required = row_bytes.checked_mul(height as usize).ok_or_else(|| {
            StitchError::MismatchedFrames(format!(
                "a frame of {width}x{height} pixels cannot be addressed"
            ))
        })?;
        if rgba.len() < required {
            return Err(StitchError::MismatchedFrames(format!(
                "a frame of {width}x{height} needs {required} bytes, the buffer holds {}",
                rgba.len()
            )));
        }

        let colour_count = (columns * COLOUR_CHANNELS) as f64;
        let mut rows = Vec::with_capacity(height as usize);
        for row in 0..height as usize {
            let start = row * row_bytes;
            let pixels = &rgba[start..start + row_bytes];
            let total: f64 = pixels
                .chunks_exact(CHANNELS)
                .map(|pixel| {
                    pixel[..COLOUR_CHANNELS]
                        .iter()
                        .map(|channel| f64::from(*channel))
                        .sum::<f64>()
                })
                .sum();
            rows.push((total / colour_count) as f32);
        }

        Ok(Self { width, rows })
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Alignment {
    /// Rows at the top of `next` that repeat the bottom of `previous`.
    pub overlap: u32,
    /// Zero-normalised correlation of the overlapping rows, 0.0 to 1.0.
    pub score: f32,
}

/// Finds how far `next` scrolled past `previous`.
///
/// Every candidate overlap is scored with a zero-normalised cross correlation
/// of the row signatures, and the best scoring one wins. Subtracting each
/// run's own mean is what makes the answer survive a change of brightness
/// between the two frames, which is the property a phase correlation would
/// have been carried in for; the displacement here is one-dimensional and
/// bounded by the frame height, so the same normalisation is cheaper to do
/// directly.
///
/// The `NoOverlap` this returns carries index 0: a pair of frames does not
/// know where it sits in a sequence. A caller that walks a sequence restates
/// the index of the frame it was aligning.
pub fn align(
    previous: &RowSignatures,
    next: &RowSignatures,
    hint: ScrollHint,
) -> Result<Alignment, StitchError> {
    if previous.width != next.width || previous.len() != next.len() {
        return Err(StitchError::MismatchedFrames(format!(
            "one frame is {}x{}, the other {}x{}",
            previous.width,
            previous.len(),
            next.width,
            next.len()
        )));
    }
    match hint.axis {
        ScrollAxis::Vertical => {}
    }

    let height = previous.len();
    let min_shift = hint.min_shift as usize;
    // An overlap shorter than `min_shift` rows is too little picture to
    // correlate, and an overlap longer than `height - min_shift` means the
    // page moved less than `min_shift`, which is to say it did not move.
    let widest = height.saturating_sub(min_shift);

    let mut best: Option<Alignment> = None;
    for overlap in min_shift..=widest {
        let tail = &previous.rows[height - overlap..];
        let head = &next.rows[..overlap];
        let Some(score) = correlate(tail, head) else {
            continue;
        };
        // `>=` so that among equally good candidates the widest overlap, and
        // so the shortest scroll, wins. A repeating pattern makes every
        // multiple of its period score alike, and the shortest scroll is the
        // only one of them that cannot have skipped a period of the page.
        if best.is_none_or(|found| score >= found.score) {
            best = Some(Alignment {
                overlap: overlap as u32,
                score,
            });
        }
    }

    match best {
        Some(alignment) if alignment.score >= hint.min_score => Ok(alignment),
        _ => Err(StitchError::NoOverlap { index: 0 }),
    }
}

/// Zero-normalised cross correlation of two equally long runs of row
/// signatures, or `None` when a run is flat and so has no shape to match.
fn correlate(left: &[f32], right: &[f32]) -> Option<f32> {
    let count = left.len();
    if count == 0 || count != right.len() {
        return None;
    }
    let scale = count as f64;
    let mean_left = left.iter().map(|row| f64::from(*row)).sum::<f64>() / scale;
    let mean_right = right.iter().map(|row| f64::from(*row)).sum::<f64>() / scale;

    let mut covariance = 0.0_f64;
    let mut spread_left = 0.0_f64;
    let mut spread_right = 0.0_f64;
    for (row_left, row_right) in left.iter().zip(right.iter()) {
        let deviation_left = f64::from(*row_left) - mean_left;
        let deviation_right = f64::from(*row_right) - mean_right;
        covariance += deviation_left * deviation_right;
        spread_left += deviation_left * deviation_left;
        spread_right += deviation_right * deviation_right;
    }

    let denominator = (spread_left * spread_right).sqrt();
    if denominator <= 0.0 {
        return None;
    }
    Some((covariance / denominator) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: u32 = 16;
    const HEIGHT: u32 = 200;
    const SHIFT: u32 = 60;
    /// Well under the headroom the synthetic pattern leaves, so no colour
    /// channel saturates and the brightness change stays a pure offset.
    const BRIGHTNESS_STEP: u8 = 32;
    const BAND_PERIOD: u32 = 20;

    /// The greyscale level of one absolute row of an imagined endless page.
    ///
    /// A mixing hash rather than a ramp: neighbouring rows have to look
    /// nothing alike, or every displacement would correlate with every other.
    /// Levels stay under 192 to leave room for the brightness test.
    fn row_level(row: u32) -> u8 {
        let mut mixed = row.wrapping_add(0x9E37_79B9);
        mixed ^= mixed >> 16;
        mixed = mixed.wrapping_mul(0x85EB_CA6B);
        mixed ^= mixed >> 13;
        mixed = mixed.wrapping_mul(0xC2B2_AE35);
        mixed ^= mixed >> 16;
        (mixed % 192) as u8
    }

    /// A window onto that endless page: `offset` is the absolute row the
    /// frame starts at, so two offsets are two scroll positions of one page.
    fn synthetic(width: u32, height: u32, offset: u32) -> Vec<u8> {
        rows(width, height, |row| row_level(row + offset))
    }

    /// A page of stripes that repeat every `BAND_PERIOD` rows, which is the
    /// shape that tempts an aligner into answering "nothing moved".
    fn banded(width: u32, height: u32) -> Vec<u8> {
        rows(width, height, |row| ((row % BAND_PERIOD) * 12) as u8)
    }

    fn rows(width: u32, height: u32, level: impl Fn(u32) -> u8) -> Vec<u8> {
        let mut data = Vec::with_capacity((width as usize) * (height as usize) * 4);
        for row in 0..height {
            let value = level(row);
            for _ in 0..width {
                data.extend_from_slice(&[value, value, value, u8::MAX]);
            }
        }
        data
    }

    fn signatures(rgba: &[u8], width: u32, height: u32) -> RowSignatures {
        RowSignatures::of(rgba, width, height).expect("well formed frame")
    }

    // A1
    #[test]
    fn reports_the_rows_a_scrolled_frame_repeats() {
        let previous = signatures(&synthetic(WIDTH, HEIGHT, 0), WIDTH, HEIGHT);
        let next = signatures(&synthetic(WIDTH, HEIGHT, SHIFT), WIDTH, HEIGHT);

        let alignment = align(&previous, &next, ScrollHint::default()).expect("frames overlap");

        assert_eq!(alignment.overlap, HEIGHT - SHIFT);
        assert!(alignment.score > 0.99, "score was {}", alignment.score);
    }

    // A2
    #[test]
    fn refuses_frames_that_share_nothing() {
        let previous = signatures(&synthetic(WIDTH, HEIGHT, 0), WIDTH, HEIGHT);
        let next = signatures(&synthetic(WIDTH, HEIGHT, 10_000), WIDTH, HEIGHT);

        assert_eq!(
            align(&previous, &next, ScrollHint::default()),
            Err(StitchError::NoOverlap { index: 0 })
        );
    }

    // A3
    #[test]
    fn a_brightness_change_does_not_move_the_overlap() {
        let previous = signatures(&synthetic(WIDTH, HEIGHT, 0), WIDTH, HEIGHT);
        let mut brighter = synthetic(WIDTH, HEIGHT, SHIFT);
        for channel in &mut brighter {
            *channel = channel.saturating_add(BRIGHTNESS_STEP);
        }
        let next = signatures(&brighter, WIDTH, HEIGHT);

        let alignment = align(&previous, &next, ScrollHint::default()).expect("frames overlap");

        assert_eq!(alignment.overlap, HEIGHT - SHIFT);
        // A plain cross correlation would answer about 0.994 here, because the
        // offset survives in the dot product. Subtracting the mean removes it
        // and leaves the two runs identical.
        assert!(alignment.score > 0.999, "score was {}", alignment.score);
    }

    // A4
    #[test]
    fn a_repeating_pattern_does_not_read_as_an_unscrolled_frame() {
        let page = banded(WIDTH, HEIGHT);
        let previous = signatures(&page, WIDTH, HEIGHT);
        let next = signatures(&page, WIDTH, HEIGHT);

        let alignment = align(&previous, &next, ScrollHint::default()).expect("frames overlap");

        assert_ne!(alignment.overlap, HEIGHT);
        assert_eq!(alignment.overlap, HEIGHT - BAND_PERIOD);
    }

    // A5
    #[test]
    fn frames_of_different_shapes_are_refused() {
        let previous = signatures(&synthetic(WIDTH, HEIGHT, 0), WIDTH, HEIGHT);

        let wider = signatures(&synthetic(WIDTH + 4, HEIGHT, SHIFT), WIDTH + 4, HEIGHT);
        assert!(matches!(
            align(&previous, &wider, ScrollHint::default()),
            Err(StitchError::MismatchedFrames(_))
        ));

        let shorter = signatures(&synthetic(WIDTH, HEIGHT - 40, SHIFT), WIDTH, HEIGHT - 40);
        assert!(matches!(
            align(&previous, &shorter, ScrollHint::default()),
            Err(StitchError::MismatchedFrames(_))
        ));
    }

    // A6
    #[test]
    fn a_buffer_too_short_for_its_shape_is_an_error_not_a_panic() {
        let mut truncated = synthetic(WIDTH, HEIGHT, 0);
        truncated.pop();

        assert!(matches!(
            RowSignatures::of(&truncated, WIDTH, HEIGHT),
            Err(StitchError::MismatchedFrames(_))
        ));
    }

    // A7
    #[test]
    fn two_identical_frames_are_not_an_overlap() {
        let page = synthetic(WIDTH, HEIGHT, 0);
        let previous = signatures(&page, WIDTH, HEIGHT);
        let next = signatures(&page, WIDTH, HEIGHT);

        assert_eq!(
            align(&previous, &next, ScrollHint::default()),
            Err(StitchError::NoOverlap { index: 0 })
        );
    }
}
