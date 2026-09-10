use std::fs::File;
use std::io::{BufWriter, ErrorKind, Write};
use std::path::{Path, PathBuf};

use chrono::Local;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};
use snapdeck_capture::Frame;

use crate::settings::SaveFormat;

/// How many names a capture may try before giving up on the directory.
///
/// A bound rather than an open loop: every iteration is a `create_new` syscall,
/// and a directory that answers `AlreadyExists` to all of them is a situation
/// to report, not to spin in.
const MAX_NAME_ATTEMPTS: u32 = 10_000;

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

/// Writes a frame as PNG, replacing whatever is at `path`. Row padding is
/// removed and BGRA is converted by `Frame::to_rgba8`, so the file is always
/// tightly packed RGBA.
///
/// Truncating is right for the one caller that uses it: the frozen backdrop is
/// rewritten by every capture and is meant to be. Anything the user keeps goes
/// through `save_png_without_overwriting` instead.
pub fn save_png(frame: &Frame, path: &Path, compression: PngCompression) -> Result<(), String> {
    let file =
        File::create(path).map_err(|e| format!("failed to create {}: {e}", path.display()))?;
    encode_png(frame, file, compression)
}

/// How hard the JPEG encoder works, out of 100.
///
/// High, and deliberately higher than a photograph would be given, for the
/// reason `packages/editor` gives its own 0.92 at length: a screenshot is the
/// worst case for this encoder rather than its best one. JPEG spends its budget
/// on smooth gradients and discards the high-frequency detail a photograph does
/// not miss, and a screenshot is almost entirely that detail, hard edges between
/// flat colours, every one of which is a glyph. The same number as the editor's,
/// because it is the same picture leaving by a different door.
///
/// "The same as the editor's" is a claim, and
/// `the_jpeg_quality_matches_the_one_the_editor_encodes_at` is what makes it a
/// fact: it reads `packages/editor/src/Editor.tsx` and fails if the two have
/// drifted. Without it, moving either number means an edit-and-save
/// re-compresses at a quality the capture was never written at, and nothing
/// anywhere says so.
const JPEG_QUALITY: u8 = 92;

/// Writes a frame under `directory` in `format`, adding a macOS-style ` 2`,
/// ` 3`, … to `stem` until it finds a name nothing holds, and returns the path
/// used.
///
/// `File::create` truncates, so the plain `save_png` next door would destroy a
/// capture that happens to render the same name. The default template's
/// one-second resolution makes that unreachable, but the template is the user's
/// now, and one without `{time}` in it would otherwise leave them with exactly
/// one file no matter how many captures they took.
///
/// The name is claimed with `create_new`, not with an "does it exist" check
/// followed by a write: the check would answer for a moment that has passed by
/// the time the file is opened, and the whole point here is that nothing is
/// overwritten.
pub fn save_capture_without_overwriting(
    frame: &Frame,
    directory: &Path,
    stem: &str,
    format: SaveFormat,
) -> Result<PathBuf, String> {
    let extension = format.extension();
    for attempt in 1..=MAX_NAME_ATTEMPTS {
        let path = directory.join(suffixed_file_name(stem, attempt, extension));
        match File::options().write(true).create_new(true).open(&path) {
            Ok(file) => {
                encode_capture(frame, file, format)?;
                return Ok(path);
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(format!("failed to create {}: {err}", path.display())),
        }
    }
    Err(format!(
        "failed to find a free name for {stem}.{extension} in {} after {MAX_NAME_ATTEMPTS} tries",
        directory.display()
    ))
}

/// The longest a single path component may be on macOS, in bytes.
///
/// APFS and HFS+ both stop here, and the failure is per capture rather than per
/// setting: a template that renders past it is accepted by the settings window
/// and then fails every write from then on, with the error arriving as a failed
/// capture rather than as a refused setting.
const MAX_FILE_NAME_BYTES: usize = 255;

/// Refuses a template that cannot produce a file name, and says by how much.
///
/// The save directory is proved by writing to it because a setting the user has
/// been shown as accepted has to work. The template earns the same treatment,
/// and for the same reason it is checked *here*: it is rendered with the very
/// function a capture renders it with, so the rule cannot drift away from the
/// thing it is a rule about.
///
/// The widest name the template can ever produce, not today's: the sizes go in
/// at `u32::MAX`, and the collision suffix is the last one
/// `save_capture_without_overwriting` would try. A template that fits now and
/// fails on the ten-thousandth capture of the same second is the same bug
/// arriving later.
pub fn check_filename_template(template: &str, format: SaveFormat) -> Result<(), String> {
    let widest = render_filename(template, OffsetDateTimeParts::now(), u32::MAX, u32::MAX);
    let name = suffixed_file_name(&widest, MAX_NAME_ATTEMPTS, format.extension());
    if name.len() <= MAX_FILE_NAME_BYTES {
        return Ok(());
    }
    Err(format!(
        "That file name is too long: it comes out at {} bytes and macOS stops at {MAX_FILE_NAME_BYTES}. Every capture would fail to save.",
        name.len()
    ))
}

/// The file name for the `attempt`-th try at `stem`, counting from one.
///
/// The first attempt is the bare name, so a capture that collides with nothing
/// is named exactly what the template rendered; only a taken name grows a
/// suffix, which is what the Finder does with a duplicate.
fn suffixed_file_name(stem: &str, attempt: u32, extension: &str) -> String {
    if attempt <= 1 {
        format!("{stem}.{extension}")
    } else {
        format!("{stem} {attempt}.{extension}")
    }
}

/// Encodes the picture the user keeps, in the format they asked for.
///
/// PNG gets `Default` rather than the `Fast` the backdrop uses: this is a file
/// they keep, where the measurements in `PngCompression` put `Fast` + `NoFilter`
/// at 29.9 MB against 2.3 MB, and nobody is waiting on the write.
///
/// JPEG has no alpha channel, so the frame's is dropped rather than composited
/// against a colour this module would have to invent. Nothing is lost by it: a
/// display capture is opaque, which is what the format's own rule assumes.
fn encode_capture<W: Write>(frame: &Frame, writer: W, format: SaveFormat) -> Result<(), String> {
    match format {
        SaveFormat::Png => encode_png(frame, writer, PngCompression::Default),
        SaveFormat::Jpeg => encode_jpeg(frame, writer),
    }
}

/// Encodes a frame as JPEG.
///
/// The conversion to RGB is not a preference. `JpegEncoder::write_image` refuses
/// `Rgba8` outright, so the choice is between doing this here and handing the
/// user an error instead of a capture.
fn encode_jpeg<W: Write>(frame: &Frame, writer: W) -> Result<(), String> {
    let rgba = frame.to_rgba8().map_err(|e| e.to_string())?;
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for pixel in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&pixel[..3]);
    }
    JpegEncoder::new_with_quality(BufWriter::new(writer), JPEG_QUALITY)
        .write_image(&rgb, frame.width, frame.height, ExtendedColorType::Rgb8)
        .map_err(|e| format!("failed to save jpeg: {e}"))
}

/// Encodes a frame into an open sink, so both save paths share one encoder and
/// one conversion and differ only in how they got the file.
fn encode_png<W: Write>(
    frame: &Frame,
    writer: W,
    compression: PngCompression,
) -> Result<(), String> {
    let rgba = frame.to_rgba8().map_err(|e| e.to_string())?;
    let (compression_type, filter) = compression.encoder_settings();
    PngEncoder::new_with_quality(BufWriter::new(writer), compression_type, filter)
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
    /// The current local wall clock, zone and daylight saving included.
    ///
    /// Local rather than UTC, because the offset moves the date and not only
    /// the hour: an evening capture west of Greenwich lands on tomorrow's date,
    /// so a day's screenshots sort into two folders' worth of names for no
    /// reason the user can see.
    ///
    /// `chrono` is asked for one thing, how far this machine is from UTC at
    /// this instant, which is the part that needs a zone database. The calendar
    /// maths below is unchanged and stays pure, so `civil_from_days` is still
    /// testable without a clock.
    pub fn now() -> Self {
        let now = Local::now();
        // Shifting the epoch by the offset turns the UTC instant into the local
        // wall clock, which the civil maths then decomposes exactly as before.
        let secs = now.timestamp() + i64::from(now.offset().local_minus_utc());
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

    /// A directory of this test's own, so the collision tests only ever meet
    /// the files they wrote themselves.
    fn temp_dir(name: &str) -> std::path::PathBuf {
        let directory = temp_path(name);
        std::fs::create_dir_all(&directory).expect("create the temporary directory");
        directory
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

    /// The shape the built-in template has, written here as a literal of this
    /// test's own rather than by reaching for the default: the default belongs
    /// to `settings`, and a copy of it here would be exactly the second source
    /// of truth that module exists to prevent.
    #[test]
    fn renders_the_date_and_time_tokens() {
        let name = render_filename("Capture {date} at {time}", parts(), 800, 600);
        assert_eq!(name, "Capture 2026-09-07 at 04.05.06");
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

    /// The quality is one number about one picture, written in two languages
    /// because the picture leaves by two doors: the capture is encoded here and
    /// an edited save is encoded by `packages/editor`. Nothing but this ties
    /// them together, so if either moves the other has to move with it.
    ///
    /// The TypeScript is read rather than mirrored, for the reason
    /// `settings::the_extensions_match_the_ones_the_editor_writes` gives:
    /// asserting a Rust constant against a second Rust constant would pass right
    /// up until the moment it stopped mattering.
    #[test]
    fn the_jpeg_quality_matches_the_one_the_editor_encodes_at() {
        let editor = include_str!("../../../../packages/editor/src/Editor.tsx");
        // The editor's scale is 0 to 1, this one is 0 to 100, and the constant
        // is derived here rather than typed so that changing `JPEG_QUALITY`
        // moves the expectation with it.
        let expected = format!(
            "const JPEG_QUALITY = {:.2}",
            f64::from(JPEG_QUALITY) / 100.0
        );
        assert!(
            editor.contains(&expected),
            "the editor no longer encodes JPEG at {JPEG_QUALITY}/100, so an edited save would re-compress the capture at a quality it was never written at; looked for {expected:?}"
        );
    }

    /// A template that cannot produce a file name is refused where the user can
    /// still do something about it, rather than at every capture from then on.
    #[test]
    fn a_template_that_renders_past_the_file_name_limit_is_refused() {
        let long = "x".repeat(MAX_FILE_NAME_BYTES);
        let err = check_filename_template(&long, SaveFormat::Png)
            .expect_err("a name this long fails every write");
        assert!(
            err.contains(&MAX_FILE_NAME_BYTES.to_string()),
            "the user has to be told the limit: {err}"
        );
    }

    /// The widest substitution, not today's. Twelve token pairs are 192 bytes
    /// of template and render to 84 for an 800x600 capture, so a check against
    /// either of those numbers would wave this through; at `u32::MAX` the same
    /// template renders to 252, and the collision suffix and the extension take
    /// it past the limit.
    #[test]
    fn a_template_is_measured_at_its_widest_substitution() {
        let template = "{width}x{height}".repeat(12);
        assert!(template.len() < MAX_FILE_NAME_BYTES, "as text it fits");
        assert!(
            render_filename(&template, parts(), 800, 600).len() < MAX_FILE_NAME_BYTES,
            "and at an ordinary capture's size it fits"
        );
        check_filename_template(&template, SaveFormat::Png)
            .expect_err("a template that fits only while the numbers are short is not usable");
    }

    /// And the other half: the default template, and a plain one, have to be
    /// accepted in both formats.
    #[test]
    fn a_workable_template_is_accepted_in_either_format() {
        for format in [SaveFormat::Png, SaveFormat::Jpeg] {
            check_filename_template(
                &crate::settings::Settings::default().filename_template,
                format,
            )
            .expect("the built-in template has to be usable");
            check_filename_template("shot-{width}x{height}", format)
                .expect("a short template with both size tokens has to be usable");
        }
    }

    /// The three dates the calendar maths can get wrong on its own. Day numbers
    /// are Unix days, and each expectation is the date that day number is, not
    /// a value read back out of the function it is testing.
    #[test]
    fn civil_from_days_handles_a_leap_day() {
        // 29 February 2024: the extra day of a leap year, which a plain
        // 365-day year would render as 1 March.
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(19_783), (2024, 3, 1));
    }

    #[test]
    fn civil_from_days_handles_the_century_rules() {
        // 2000 is a leap year (divisible by 400) and 1900 is not (divisible by
        // 100 but not 400), so both centuries have to be crossed, and the two
        // rules disagree about the same February. 1900 is also before the
        // epoch, so its day number is negative and the rebasing onto the
        // proleptic era has to absorb the sign before anything divides.
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
        assert_eq!(civil_from_days(-25_509), (1900, 2, 28));
        assert_eq!(civil_from_days(-25_508), (1900, 3, 1));
    }

    #[test]
    fn civil_from_days_handles_a_year_end() {
        assert_eq!(civil_from_days(20_818), (2026, 12, 31));
        assert_eq!(civil_from_days(20_819), (2027, 1, 1));
    }

    /// The epoch itself, as a fixed point that pins the whole day numbering.
    #[test]
    fn civil_from_days_puts_day_zero_at_the_epoch() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn the_first_capture_of_a_name_keeps_the_name() {
        let directory = temp_dir("first");
        let path = save_capture_without_overwriting(
            &sample_frame(),
            &directory,
            "Capture 2026-09-07 at 04.05.06",
            SaveFormat::Png,
        )
        .expect("save");

        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("Capture 2026-09-07 at 04.05.06.png")
        );
        std::fs::remove_dir_all(&directory).ok();
    }

    /// The format setting has to reach the file, not only the extension on it.
    /// A `.jpg` holding PNG bytes is exactly the failure the editor's own
    /// WebP removal was about.
    #[test]
    fn the_chosen_format_decides_the_extension_and_the_bytes() {
        let directory = temp_dir("format");
        let png = save_capture_without_overwriting(
            &sample_frame(),
            &directory,
            "as-png",
            SaveFormat::Png,
        )
        .expect("save png");
        let jpeg = save_capture_without_overwriting(
            &sample_frame(),
            &directory,
            "as-jpeg",
            SaveFormat::Jpeg,
        )
        .expect("save jpeg");

        let png_format = image::ImageReader::open(&png)
            .expect("open png")
            .format()
            .expect("a recognised format");
        let jpeg_format = image::ImageReader::open(&jpeg)
            .expect("open jpeg")
            .format()
            .expect("a recognised format");
        let jpeg_size = image::open(&jpeg).expect("decode jpeg").to_rgba8();
        std::fs::remove_dir_all(&directory).ok();

        assert_eq!(png.extension().and_then(|e| e.to_str()), Some("png"));
        assert_eq!(jpeg.extension().and_then(|e| e.to_str()), Some("jpg"));
        assert_eq!(png_format, image::ImageFormat::Png);
        assert_eq!(jpeg_format, image::ImageFormat::Jpeg);
        // Not only a readable file: the JPEG carries the same picture, which is
        // what the RGBA-to-RGB conversion could get wrong by a row.
        assert_eq!(jpeg_size.dimensions(), (2, 2));
    }

    /// The point of the whole function: a second capture rendering the same
    /// name must not truncate the first one.
    #[test]
    fn a_colliding_name_is_suffixed_rather_than_overwritten() {
        let directory = temp_dir("collision");
        let first =
            save_capture_without_overwriting(&sample_frame(), &directory, "shot", SaveFormat::Png)
                .expect("first save");
        let first_bytes = std::fs::read(&first).expect("read the first file");

        let second =
            save_capture_without_overwriting(&sample_frame(), &directory, "shot", SaveFormat::Png)
                .expect("second save");
        let third =
            save_capture_without_overwriting(&sample_frame(), &directory, "shot", SaveFormat::Png)
                .expect("third save");

        assert_eq!(
            second.file_name().and_then(|name| name.to_str()),
            Some("shot 2.png")
        );
        assert_eq!(
            third.file_name().and_then(|name| name.to_str()),
            Some("shot 3.png")
        );
        // Not merely a different path: the first file still holds its own
        // pixels, which is the claim a truncating create would break.
        assert_eq!(
            std::fs::read(&first).expect("re-read the first file"),
            first_bytes
        );
        assert!(first.exists() && second.exists() && third.exists());
        std::fs::remove_dir_all(&directory).ok();
    }
}
