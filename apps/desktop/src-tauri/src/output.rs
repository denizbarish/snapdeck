use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};
use snapdeck_capture::Frame;

/// How hard the PNG encoder works.
///
/// Both settings are lossless. Only the deflate stream and the per-scanline
/// filter change, never a pixel value, so a colour read back from either file
/// is the colour that was on screen.
///
/// Measured in a release build on a real 3420x2224 frame (30.4 MB of RGBA),
/// best of three, encode only:
///
/// | compression | filter | encode | file |
/// | --- | --- | --- | --- |
/// | `Fast` | `Adaptive` | 32.7 ms | 2.3 MB |
/// | `Fast` | `NoFilter` | 48.0 ms | 29.9 MB |
/// | `Uncompressed` | `NoFilter` | 71.3 ms | 30.4 MB |
/// | `Default` | `Adaptive` | 273.5 ms | 1.4 MB |
///
/// `Adaptive` is both the fastest and by far the smallest here: the filter
/// search costs less than the deflate work it saves, and the file the webview
/// then has to read back is a thirteenth of the size. `Fast` + `Adaptive` is
/// `image`'s own default, so `PngCompression::Fast` is the plain
/// `PngEncoder::new` behaviour, spelled out rather than inherited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PngCompression {
    /// The overlay backdrop, where the user is blocked on the encode and on the
    /// webview's read of the result.
    Fast,
    /// The artifact the user keeps, where the file is half the size again and
    /// nobody is waiting on the write.
    // Task 10 saves the user's screenshot with this; nothing constructs it yet.
    #[allow(dead_code)]
    Default,
}

impl PngCompression {
    fn encoder_settings(self) -> (CompressionType, FilterType) {
        match self {
            Self::Fast => (CompressionType::Fast, FilterType::Adaptive),
            Self::Default => (CompressionType::Default, FilterType::Adaptive),
        }
    }
}

/// Writes a frame as PNG. Row padding is removed and BGRA is converted by
/// `Frame::to_rgba8`, so the file is always tightly packed RGBA.
pub fn save_png(frame: &Frame, path: &Path, compression: PngCompression) -> Result<(), String> {
    let rgba = frame.to_rgba8().map_err(|e| e.to_string())?;
    let file =
        File::create(path).map_err(|e| format!("failed to create {}: {e}", path.display()))?;
    let (compression_type, filter) = compression.encoder_settings();
    PngEncoder::new_with_quality(BufWriter::new(file), compression_type, filter)
        .write_image(&rgba, frame.width, frame.height, ExtendedColorType::Rgba8)
        .map_err(|e| format!("failed to save png: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::{SystemTime, UNIX_EPOCH};

    use snapdeck_capture::PixelFormat;

    /// 2x2 BGRA with two padding bytes per row, so a stride bug and a channel
    /// order bug both show up in the decoded pixels.
    fn sample_frame() -> Frame {
        #[rustfmt::skip]
        let data = vec![
            // row 0: opaque red, opaque green, padding
            0, 0, 255, 255, /**/ 0, 255, 0, 255, /**/ 0, 0,
            // row 1: opaque blue, half-transparent white, padding
            255, 0, 0, 255, /**/ 255, 255, 255, 128, /**/ 0, 0,
        ];
        Frame {
            data,
            width: 2,
            height: 2,
            stride: 10,
            pixel_format: PixelFormat::Bgra8,
            scale_factor: 2.0,
            captured_at: SystemTime::now(),
        }
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after the epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("snapdeck-{}-{unique}-{name}", std::process::id()))
    }

    #[test]
    fn save_png_writes_rgba_pixels_without_stride_padding() {
        let path = temp_path("fast.png");
        save_png(&sample_frame(), &path, PngCompression::Fast).expect("save");

        let decoded = image::open(&path).expect("decode").to_rgba8();
        std::fs::remove_file(&path).ok();

        assert_eq!(decoded.dimensions(), (2, 2));
        #[rustfmt::skip]
        assert_eq!(
            decoded.into_raw(),
            vec![
                255, 0, 0, 255, /**/ 0, 255, 0, 255,
                0, 0, 255, 255, /**/ 255, 255, 255, 128,
            ]
        );
    }

    #[test]
    fn compression_level_does_not_change_the_pixels() {
        let fast_path = temp_path("level-fast.png");
        let default_path = temp_path("level-default.png");
        save_png(&sample_frame(), &fast_path, PngCompression::Fast).expect("save fast");
        save_png(&sample_frame(), &default_path, PngCompression::Default).expect("save default");

        let fast = image::open(&fast_path).expect("decode fast").to_rgba8();
        let default = image::open(&default_path)
            .expect("decode default")
            .to_rgba8();
        std::fs::remove_file(&fast_path).ok();
        std::fs::remove_file(&default_path).ok();

        assert_eq!(fast.into_raw(), default.into_raw());
    }
}
