//! Pasting a run of aligned frames into one picture.

use crate::error::StitchError;

/// Bytes per pixel in a tightly packed RGBA8 buffer.
const CHANNELS: usize = 4;

/// The most pixels a stitched picture may hold.
pub const MAX_STITCHED_PIXELS: u64 = 64_000_000;

/// The scale `compose` stamps on what it returns.
///
/// A tile is pixels and nothing else, so composition has no scale of its own
/// to report. `stitch`, which is handed frames that each carry one, restates
/// it on the way out.
const UNMEASURED_SCALE: f32 = 1.0;

/// A stitched picture: tightly packed RGBA8, and the scale it was measured at.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
}

/// One frame reduced to what composition needs.
pub struct Tile<'a> {
    pub rgba: &'a [u8],
    pub width: u32,
    pub height: u32,
}

/// Pastes tiles into one picture, dropping from each tile the `overlaps[i]`
/// rows it repeats from the one before it.
///
/// `overlaps` holds one fewer entry than `tiles`.
///
/// Split out from `stitch` for the reason `lib::adopt_shortcuts` is: the
/// arithmetic worth testing is where each frame lands, and building thirty
/// frames to test one paste is a slower way of asking the same question.
pub fn compose(tiles: &[Tile<'_>], overlaps: &[u32]) -> Result<Image, StitchError> {
    let Some(first) = tiles.first() else {
        return Err(StitchError::NoFrames);
    };

    let expected = tiles.len() - 1;
    if overlaps.len() != expected {
        return Err(StitchError::MismatchedFrames(format!(
            "{} tiles need {expected} overlaps, {} were given",
            tiles.len(),
            overlaps.len()
        )));
    }

    let width = first.width;
    let row_bytes = row_bytes(width)?;
    for (index, tile) in tiles.iter().enumerate() {
        if tile.width != width {
            return Err(StitchError::MismatchedFrames(format!(
                "tile {index} is {} pixels wide, the first is {width}",
                tile.width
            )));
        }
        // Tiles are borrowed buffers whose shape is only claimed, so the
        // claim is measured here rather than trusted by a slice index below.
        let required = row_bytes
            .checked_mul(tile.height as usize)
            .ok_or_else(|| too_wide(width, tile.height))?;
        if tile.rgba.len() < required {
            return Err(StitchError::MismatchedFrames(format!(
                "tile {index} of {width}x{} needs {required} bytes, the buffer holds {}",
                tile.height,
                tile.rgba.len()
            )));
        }
    }

    // The tallest the picture can be is every row of every tile, less the
    // rows the tiles repeat. Counted in `u64` so the sum cannot wrap, and
    // counted before a byte is allocated so a run that cannot fit is refused
    // rather than half built.
    let mut height = u64::from(first.height);
    for (index, (tile, overlap)) in tiles[1..].iter().zip(overlaps).enumerate() {
        if *overlap > tile.height {
            return Err(StitchError::MismatchedFrames(format!(
                "tile {} repeats {overlap} rows of a tile only {} rows tall",
                index + 1,
                tile.height
            )));
        }
        height += u64::from(tile.height - overlap);
    }

    let pixels = u64::from(width) * height;
    let too_large = StitchError::TooLarge {
        pixels,
        limit: MAX_STITCHED_PIXELS,
    };
    if pixels > MAX_STITCHED_PIXELS {
        return Err(too_large);
    }
    // Within the limit and at least one pixel wide, so the height is at most
    // `MAX_STITCHED_PIXELS` and fits. A zero width picture is the one shape
    // that says nothing about its height, and it has no rows to paste.
    let height = u32::try_from(height).map_err(|_| too_large)?;

    let mut data = Vec::with_capacity(row_bytes.saturating_mul(height as usize));
    data.extend_from_slice(&first.rgba[..row_bytes * first.height as usize]);
    for (tile, overlap) in tiles[1..].iter().zip(overlaps) {
        let start = *overlap as usize * row_bytes;
        let end = tile.height as usize * row_bytes;
        data.extend_from_slice(&tile.rgba[start..end]);
    }

    Ok(Image {
        data,
        width,
        height,
        scale_factor: UNMEASURED_SCALE,
    })
}

/// Bytes one row of a `width` pixel picture occupies.
///
/// A picture no pixels wide has no rows to paste and no height that could be
/// read back off it, so it is refused here rather than returned empty.
fn row_bytes(width: u32) -> Result<usize, StitchError> {
    if width == 0 {
        return Err(StitchError::MismatchedFrames(
            "a tile 0 pixels wide has nothing to paste".to_owned(),
        ));
    }
    (width as usize)
        .checked_mul(CHANNELS)
        .ok_or_else(|| too_wide(width, 1))
}

fn too_wide(width: u32, height: u32) -> StitchError {
    StitchError::MismatchedFrames(format!(
        "a tile of {width}x{height} pixels cannot be addressed"
    ))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const WIDTH: u32 = 16;
    const TILE_HEIGHT: u32 = 100;
    const OVERLAP: u32 = 40;
    /// Three 100 row tiles that each repeat 40 rows of the one before it.
    const STITCHED_HEIGHT: u32 = 220;

    /// The greyscale level of one absolute row of an imagined endless page.
    ///
    /// A mixing hash rather than a ramp: neighbouring rows have to look
    /// nothing alike, or a paste that landed a row out would still compare
    /// equal to the page it came from.
    fn row_level(row: u32) -> u8 {
        let mut mixed = row.wrapping_add(0x9E37_79B9);
        mixed ^= mixed >> 16;
        mixed = mixed.wrapping_mul(0x85EB_CA6B);
        mixed ^= mixed >> 13;
        mixed = mixed.wrapping_mul(0xC2B2_AE35);
        mixed ^= mixed >> 16;
        (mixed % 192) as u8
    }

    /// A page whose every row is a flat colour no neighbouring row shares.
    ///
    /// Shared with the tests of `stitch`, which cut their frames out of the
    /// same kind of page and compare the stitched result back against it.
    pub(crate) fn page(width: u32, height: u32) -> Vec<u8> {
        let mut data = Vec::with_capacity(width as usize * height as usize * CHANNELS);
        for row in 0..height {
            let level = row_level(row);
            for _ in 0..width {
                data.extend_from_slice(&[level, level, level, u8::MAX]);
            }
        }
        data
    }

    /// The `height` rows of `page` starting at row `offset`: one frame's worth
    /// of one scroll position.
    pub(crate) fn window(page: &[u8], width: u32, offset: u32, height: u32) -> Vec<u8> {
        let row_bytes = width as usize * CHANNELS;
        let start = offset as usize * row_bytes;
        page[start..start + height as usize * row_bytes].to_vec()
    }

    /// Tiles cut from `page` at a fixed step, every one `TILE_HEIGHT` tall.
    fn cuts(page: &[u8], count: u32, step: u32) -> Vec<Vec<u8>> {
        (0..count)
            .map(|index| window(page, WIDTH, index * step, TILE_HEIGHT))
            .collect()
    }

    fn tiles(cuts: &[Vec<u8>]) -> Vec<Tile<'_>> {
        cuts.iter()
            .map(|rgba| Tile {
                rgba,
                width: WIDTH,
                height: TILE_HEIGHT,
            })
            .collect()
    }

    // N1
    #[test]
    fn pasting_drops_the_rows_a_tile_repeats() {
        let source = page(WIDTH, STITCHED_HEIGHT);
        let cuts = cuts(&source, 3, TILE_HEIGHT - OVERLAP);

        let image = compose(&tiles(&cuts), &[OVERLAP, OVERLAP]).expect("the tiles paste");

        assert_eq!(image.width, WIDTH);
        assert_eq!(image.height, STITCHED_HEIGHT);
        assert_eq!(image.data, source);
    }

    // N2
    #[test]
    fn a_wrong_count_of_overlaps_is_an_error_not_a_panic() {
        let source = page(WIDTH, STITCHED_HEIGHT);
        let cuts = cuts(&source, 3, TILE_HEIGHT - OVERLAP);

        assert!(matches!(
            compose(&tiles(&cuts), &[OVERLAP]),
            Err(StitchError::MismatchedFrames(_))
        ));
        assert!(matches!(
            compose(&tiles(&cuts), &[OVERLAP, OVERLAP, OVERLAP]),
            Err(StitchError::MismatchedFrames(_))
        ));
    }

    // N3
    #[test]
    fn an_overlap_taller_than_its_own_tile_is_refused() {
        let source = page(WIDTH, STITCHED_HEIGHT);
        let cuts = cuts(&source, 2, TILE_HEIGHT - OVERLAP);

        assert!(matches!(
            compose(&tiles(&cuts), &[TILE_HEIGHT + 1]),
            Err(StitchError::MismatchedFrames(_))
        ));
    }
}
