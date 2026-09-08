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

/// Wall-clock parts used by the filename template. Kept as plain fields so the
/// renderer is pure and testable without a clock.
#[derive(Debug, Clone, Copy)]
pub struct OffsetDateTimeParts {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl OffsetDateTimeParts {
    /// The current UTC wall clock, read from the system clock.
    ///
    /// UTC rather than local time, because `std` carries no time zone database
    /// and this crate has no date library. A file therefore gets the UTC hour,
    /// which east of Greenwich is not the hour the user took the screenshot.
    pub fn now() -> Self {
        // std has no calendar math, so derive the parts from the Unix epoch.
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let days = secs.div_euclid(86_400);
        let time_of_day = secs.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        Self {
            year,
            month,
            day,
            hour: (time_of_day / 3600) as u8,
            minute: ((time_of_day % 3600) / 60) as u8,
            second: (time_of_day % 60) as u8,
        }
    }
}

/// Howard Hinnant's days-from-civil inverse; converts a Unix day number to a date.
fn civil_from_days(z: i64) -> (i32, u8, u8) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

/// Expands `{date}`, `{time}`, `{width}` and `{height}` in a filename template.
/// Unknown tokens are left as-is. Path separators are replaced with hyphens so
/// the result is always a single filename.
pub fn render_filename(template: &str, at: OffsetDateTimeParts, width: u32, height: u32) -> String {
    let date = format!("{:04}-{:02}-{:02}", at.year, at.month, at.day);
    // Colons are not usable in macOS filenames, so time uses dots.
    let time = format!("{:02}.{:02}.{:02}", at.hour, at.minute, at.second);
    template
        .replace("{date}", &date)
        .replace("{time}", &time)
        .replace("{width}", &width.to_string())
        .replace("{height}", &height.to_string())
        .replace(['/', '\\'], "-")
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

    fn parts() -> OffsetDateTimeParts {
        OffsetDateTimeParts {
            year: 2026,
            month: 9,
            day: 7,
            hour: 4,
            minute: 5,
            second: 6,
        }
    }

    #[test]
    fn renders_the_default_template() {
        let name = render_filename("Snapdeck {date} at {time}", parts(), 800, 600);
        assert_eq!(name, "Snapdeck 2026-09-07 at 04.05.06");
    }

    #[test]
    fn renders_size_tokens() {
        let name = render_filename("shot-{width}x{height}", parts(), 800, 600);
        assert_eq!(name, "shot-800x600");
    }

    #[test]
    fn leaves_unknown_tokens_untouched() {
        let name = render_filename("{nope}-{width}", parts(), 10, 20);
        assert_eq!(name, "{nope}-10");
    }

    #[test]
    fn strips_path_separators_from_the_result() {
        let name = render_filename("a/b{width}", parts(), 5, 5);
        assert_eq!(name, "a-b5");
    }
}
