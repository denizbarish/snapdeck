//! What happens to a page once the bridge has accepted it.
//!
//! The picture arrives as base64 PNG in a JSON frame and leaves as a file in
//! the user's pictures folder, on the clipboard, and, if they asked for one, in
//! an editor window. Nothing here is a second capture path: the frame this
//! module builds is the frame `commands::capture_and_write` builds, and it is
//! handed to the same `output` functions in the same order, so a full page
//! obeys the format, the filename template and the editor setting exactly as a
//! region capture does.
//!
//! The decode is the other half. A message that carried PNG bytes straight to
//! disk would be three things at once: a save that ignores the user's format
//! setting, a clipboard with nothing to put on it, and a file whose contents
//! nothing on this side ever looked at. Decoding answers all three, and it is
//! the only thing that checks that the picture a message declares is the
//! picture it carries.

use std::cell::Cell;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use image::{ImageReader, Limits};
use snapdeck_capture::{Frame, PixelFormat};
use tauri::{AppHandle, Manager};

use crate::bridge::protocol::{AppInfo, FullPage, ImagePayload, MAX_IMAGE_PIXELS, MAX_PNG_BYTES};
use crate::bridge::session::BridgePolicy;
use crate::editor;
use crate::output::{render_filename, save_capture_without_overwriting, OffsetDateTimeParts};
use crate::recents;
use crate::report::report_failure;
use crate::settings::{self, Settings};
use crate::state::AppState;

/// Bytes one RGBA pixel occupies once decoded.
///
/// The bridge between a pixel budget and the decoder's byte budget: `image`
/// counts allocations, the protocol counts pixels, and this is the exchange
/// rate between them.
const BYTES_PER_PIXEL: u64 = 4;

/// How many characters of base64 carry three bytes.
const BASE64_CHARS_PER_GROUP: usize = 4;
const BASE64_BYTES_PER_GROUP: usize = 3;

/// What the user is told when the extension gave up before the bottom of the
/// page.
///
/// Reported even when the save succeeded. A picture that silently stops short
/// of the content the user asked for is worse than a failure they can see.
const TRUNCATED_PAGE: &str =
    "The page was too long to capture in full, so Snapdeck saved as much of it as the browser could give.";

/// What the extension is told when a page reached neither the disk nor the
/// clipboard, and nothing more specific survived.
const NOTHING_DELIVERED: &str = "the page reached neither the disk nor the clipboard";

#[derive(Debug, Clone, Copy)]
pub struct IntakeLimits {
    pub max_png_bytes: usize,
    pub max_pixels: u64,
}

impl Default for IntakeLimits {
    /// The protocol's own numbers.
    fn default() -> Self {
        Self {
            max_png_bytes: MAX_PNG_BYTES,
            max_pixels: MAX_IMAGE_PIXELS,
        }
    }
}

/// Turns a bridge payload into the frame the capture path already knows.
///
/// The PNG is decoded rather than written straight through, for three reasons
/// stated once here: the user's format setting has to apply to a full page
/// exactly as it applies to a region; the clipboard needs pixels; and a decode
/// is the only thing that checks that the picture a message declares is the
/// picture it carries.
pub fn frame_from_png(
    png_base64: &str,
    declared: &ImagePayload,
    limits: IntakeLimits,
) -> Result<Frame, String> {
    // First, and on the encoded length, because a limit checked after the
    // allocation is not a limit. Base64 is a fixed expansion, so the length of
    // the text is an upper bound on the bytes it would become, and the bound
    // can be taken before anything is held twice.
    let estimated = png_base64.len() / BASE64_CHARS_PER_GROUP * BASE64_BYTES_PER_GROUP;
    if estimated > limits.max_png_bytes {
        return Err(format!(
            "the page is about {estimated} bytes of png, past the {} bytes this bridge accepts",
            limits.max_png_bytes
        ));
    }

    let png = STANDARD
        .decode(png_base64)
        .map_err(|err| format!("the page is not valid base64: {err}"))?;

    let mut reader = ImageReader::new(Cursor::new(png))
        .with_guessed_format()
        .map_err(|err| format!("failed to read the delivered page: {err}"))?;
    reader.limits(decoding_limits(limits.max_pixels));
    let decoded = reader.decode().map_err(|err| {
        format!(
            "failed to decode the delivered page, which may be larger than the {} pixels this bridge accepts: {err}",
            limits.max_pixels
        )
    })?;

    // The only check that the picture a message declares is the picture it
    // carries. Everything downstream sizes itself from the declared numbers,
    // so a header and a body that disagree are refused rather than reconciled:
    // there is no way to tell which of the two the extension meant.
    let (width, height) = (decoded.width(), decoded.height());
    if width != declared.width || height != declared.height {
        return Err(format!(
            "the message says the page is {} by {} pixels and it decoded to {width} by {height}",
            declared.width, declared.height
        ));
    }

    Ok(Frame {
        data: decoded.into_rgba8().into_raw(),
        width,
        height,
        // Tightly packed: `into_rgba8` has no row padding, and a stride that
        // disagrees with the width is a frame the save path cannot read.
        stride: width as usize * 4,
        pixel_format: PixelFormat::Rgba8,
        // The browser's, not this machine's. The page was rendered at the
        // ratio the tab had, which is what turns these pixels back into the
        // points an editor window is measured in.
        scale_factor: declared.device_pixel_ratio,
        captured_at: SystemTime::now(),
    })
}

/// The decoder's budget for one delivered page, in the terms `image` counts in.
///
/// A pixel count becomes two things here. `max_alloc` is the real gate: the
/// png decoder is handed it as its own byte limit, so a picture whose declared
/// dimensions would need more than the budget fails before the buffer exists,
/// which is what stops a few hundred bytes of PNG from asking for gigabytes.
/// The dimension limits are the cheap half, refused from the header alone: a
/// picture wider or taller than the whole pixel budget cannot fit inside it.
fn decoding_limits(max_pixels: u64) -> Limits {
    let side = u32::try_from(max_pixels).unwrap_or(u32::MAX);
    let mut limits = Limits::default();
    limits.max_image_width = Some(side);
    limits.max_image_height = Some(side);
    limits.max_alloc = Some(max_pixels.saturating_mul(BYTES_PER_PIXEL));
    limits
}

/// What a delivered page did to the disk and the clipboard.
#[derive(Debug, PartialEq)]
pub struct Delivered {
    pub path: Option<PathBuf>,
    /// What the user has to be told, if anything.
    pub complaint: Option<String>,
}

/// Saves and copies one delivered page, with the clipboard injected.
///
/// Split out for the reason `write_edited` is: the claim worth testing is that
/// a full page follows exactly the rules a region capture follows, and
/// everything around it needs a live application.
pub fn deliver(
    frame: &Frame,
    directory: &Path,
    settings: &Settings,
    clipboard: impl FnOnce(&Frame) -> Result<(), String>,
) -> Delivered {
    // Best effort, and deliberately not a `?`, for the reason
    // `commands::capture_and_write` gives: a directory that cannot be created
    // shows up as a failed save below, which keeps the clipboard.
    let _ = std::fs::create_dir_all(directory);
    // The clipboard first, and both outcomes collected rather than the first
    // failure returned. A full disk or a read-only folder takes the file away,
    // and returning here would take the clipboard with it: the page is in hand
    // and one paste from being useful. Only losing both is a failed delivery.
    let clipboard = clipboard(frame);
    // The application's own save path, called the way `write_capture` calls it,
    // so the template, the collision suffix and the format are the user's
    // rather than this module's idea of them.
    let name = render_filename(
        &settings.filename_template,
        OffsetDateTimeParts::now(),
        frame.width,
        frame.height,
    );
    let saved = save_capture_without_overwriting(frame, directory, &name, settings.default_format);

    match (saved, clipboard) {
        (Ok(path), Ok(())) => Delivered {
            path: Some(path),
            complaint: None,
        },
        (Err(save_err), Ok(())) => Delivered {
            path: None,
            complaint: Some(format!(
                "Snapdeck could not save the page to {} ({save_err}), so it is only on the clipboard. Paste it before you copy anything else.",
                directory.display()
            )),
        },
        (Ok(path), Err(clipboard_err)) => {
            let complaint = format!(
                "Snapdeck saved the page to {} but could not put it on the clipboard ({clipboard_err}).",
                path.display()
            );
            Delivered {
                path: Some(path),
                complaint: Some(complaint),
            }
        }
        (Err(save_err), Err(clipboard_err)) => Delivered {
            path: None,
            complaint: Some(format!("{save_err}, and {clipboard_err}")),
        },
    }
}

/// The `BridgePolicy` the application runs with.
///
/// The handle is what makes the difference between this and a policy a test can
/// drive: the settings, the clipboard, the recents menu, the editor and the
/// failure report all hang off it, and none of them exists without a running
/// application. Everything that does not need it lives in the two functions
/// above, which is where the rules a delivered page follows are actually
/// checked.
pub struct AppPolicy {
    app: AppHandle,
    info: AppInfo,
}

impl AppPolicy {
    pub fn new(app: AppHandle, info: AppInfo) -> Self {
        Self { app, info }
    }
}

impl BridgePolicy for AppPolicy {
    /// The token in force at this moment, read for every handshake rather than
    /// copied when the bridge was opened.
    ///
    /// A copy would outlive the token it copied. `Regenerate` exists so that a
    /// user who believes their token has got out can replace it, and a bridge
    /// still answering to the old one would make that button a lie: the very
    /// sessions it is meant to lock out would keep pairing until the next
    /// relaunch. Reading here is what makes a new token take effect on the next
    /// connection.
    fn token(&self) -> String {
        self.app.state::<AppState>().settings().bridge_token
    }

    fn app_info(&self) -> AppInfo {
        self.info.clone()
    }

    /// Everything `commands::capture_and_write` does with a finished capture,
    /// in the order it does it, because a page delivered over the bridge is one
    /// of the user's captures and the rules it obeys have to be the same rules.
    fn deliver(&self, message: &FullPage) -> Result<Option<String>, String> {
        let frame = frame_from_png(
            &message.image.png_base64,
            &message.image,
            IntakeLimits::default(),
        )?;

        // The settings in force, read once, so a save that lands in the
        // settings window halfway through this delivery cannot split it
        // between two folders.
        let settings = self.app.state::<AppState>().settings();
        // Never a reason to lose the picture. A folder that has gone away or an
        // unplugged volume puts the page in the pictures directory instead and
        // says so, rather than failing over a setting.
        let (directory, complaint) = settings::resolve_save_directory(&self.app, &settings);
        if let Some(complaint) = complaint {
            report_failure(&self.app, &complaint);
        }

        // Whether the clipboard took it. `Delivered` does not carry it and the
        // extension has to know: a page that reached neither the disk nor the
        // clipboard is a failed delivery, and a page that reached one of them
        // is not.
        let copied = Cell::new(false);
        let Delivered { path, complaint } = deliver(&frame, &directory, &settings, |frame| {
            let outcome = crate::commands::copy_to_clipboard(&self.app, frame);
            copied.set(outcome.is_ok());
            outcome
        });

        // Here rather than inside `deliver`, which is handed a directory and a
        // frame so that it can be tested without an application, and here
        // rather than in the editor branch below, because a page that reached
        // the disk belongs in the menu whether or not an editor was asked for.
        if let Some(path) = &path {
            recents::record(&self.app, path);
        }
        // Said even when everything else worked. Handing the user a picture
        // that is missing its bottom half without telling them is the one
        // outcome this feature must not have.
        if message.truncated {
            report_failure(&self.app, TRUNCATED_PAGE);
        }
        if let Some(complaint) = &complaint {
            report_failure(&self.app, complaint);
        }

        match path {
            Some(path) => {
                // Only when the user wants one, and with the scale the page was
                // rendered at rather than this machine's primary display: the
                // browser's ratio is what turns these pixels back into the
                // points the window is measured in.
                if settings.open_editor_after_capture {
                    editor::open_editor(
                        &self.app,
                        &path,
                        frame.width,
                        frame.height,
                        frame.scale_factor,
                    );
                }
                Ok(Some(path.to_string_lossy().into_owned()))
            }
            // On the clipboard and nowhere else, which is the half success a
            // region capture can have too. The user has already been told.
            None if copied.get() => Ok(None),
            // Neither the disk nor the clipboard, which is the only failed
            // delivery there is, and the extension is told so it can offer the
            // download instead.
            None => Err(complaint.unwrap_or_else(|| NOTHING_DELIVERED.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::Cell;
    use std::time::UNIX_EPOCH;

    use image::codecs::png::PngEncoder;
    use image::{ExtendedColorType, ImageEncoder};

    use crate::settings::SaveFormat;

    /// The picture every test here hands the bridge: four columns, three rows,
    /// each pixel a different colour so a row or a channel out of place shows
    /// up in the bytes rather than only in the dimensions.
    const SAMPLE_WIDTH: u32 = 4;
    const SAMPLE_HEIGHT: u32 = 3;

    /// The scale the extension reports for a Retina capture.
    const SAMPLE_SCALE: f32 = 2.0;

    fn sample_pixels() -> Vec<u8> {
        (0..SAMPLE_WIDTH * SAMPLE_HEIGHT)
            .flat_map(|index| {
                let step = u8::try_from(index).expect("twelve pixels fit in a byte");
                [step * 20, 255 - step * 20, step * 8, 255]
            })
            .collect()
    }

    /// The sample picture as PNG bytes.
    fn sample_png() -> Vec<u8> {
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(
                &sample_pixels(),
                SAMPLE_WIDTH,
                SAMPLE_HEIGHT,
                ExtendedColorType::Rgba8,
            )
            .expect("the sample picture encodes");
        bytes
    }

    fn sample_base64() -> String {
        STANDARD.encode(sample_png())
    }

    /// What the message says about the picture, which the tests bend to see
    /// what the decode does about it.
    fn declared(width: u32, height: u32, png_base64: &str) -> ImagePayload {
        ImagePayload {
            png_base64: png_base64.to_owned(),
            width,
            height,
            device_pixel_ratio: SAMPLE_SCALE,
        }
    }

    /// A frame with the sample picture in it, for the tests that start after
    /// the decode.
    fn sample_frame() -> Frame {
        Frame {
            data: sample_pixels(),
            width: SAMPLE_WIDTH,
            height: SAMPLE_HEIGHT,
            stride: SAMPLE_WIDTH as usize * 4,
            pixel_format: PixelFormat::Rgba8,
            scale_factor: SAMPLE_SCALE,
            captured_at: SystemTime::now(),
        }
    }

    /// The same size as `sample_frame`, and none of the same pixels.
    fn another_frame() -> Frame {
        Frame {
            data: sample_pixels().iter().map(|byte| !byte).collect(),
            ..sample_frame()
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let directory = temp_path(name);
        std::fs::create_dir_all(&directory).expect("create the temporary directory");
        directory
    }

    fn temp_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after the epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("snapdeck-{}-{unique}-{name}", std::process::id()))
    }

    /// Settings with the two fields these tests care about set, and everything
    /// else left at the application's own defaults.
    fn settings_with(template: &str, format: SaveFormat) -> Settings {
        Settings {
            filename_template: template.to_owned(),
            default_format: format,
            ..Settings::default()
        }
    }

    /// I1. The happy path, and the field that is easy to get wrong in a way
    /// nothing else notices: `stride`. A frame whose stride disagrees with its
    /// width is not a frame the save path can use, and the failure would land
    /// on the user rather than here.
    #[test]
    fn a_declared_picture_decodes_into_the_frame_the_save_path_takes() {
        let base64 = sample_base64();
        let payload = declared(SAMPLE_WIDTH, SAMPLE_HEIGHT, &base64);

        let frame = frame_from_png(&base64, &payload, IntakeLimits::default())
            .expect("a well formed page decodes");

        assert_eq!(frame.width, 4, "the width the picture carries");
        assert_eq!(frame.height, 3, "and the height");
        assert_eq!(
            frame.stride, 16,
            "four pixels of four bytes, with no padding"
        );
        assert_eq!(frame.pixel_format, PixelFormat::Rgba8);
        assert!(
            (frame.scale_factor - 2.0).abs() < f32::EPSILON,
            "the scale is the browser's, not this machine's: {}",
            frame.scale_factor
        );
        assert_eq!(
            frame.to_rgba8().expect("the frame's own fields agree"),
            sample_pixels(),
            "and the pixels survive the round trip"
        );
    }

    /// I2. Security. A message is not trusted about its own payload. The
    /// declared size is what the rest of the application would size buffers and
    /// windows from, so a message whose header and body disagree is refused
    /// rather than reconciled.
    #[test]
    fn a_picture_that_is_not_the_size_it_claims_is_refused() {
        let base64 = sample_base64();
        let payload = declared(8, SAMPLE_HEIGHT, &base64);

        let refusal = frame_from_png(&base64, &payload, IntakeLimits::default())
            .expect_err("a lying header is refused");

        assert!(
            refusal.contains("8 by 3"),
            "the refusal has to say what the message claimed: {refusal}"
        );
        assert!(
            refusal.contains("4 by 3"),
            "and what it actually carried: {refusal}"
        );
    }

    /// I3. Security. The size gate runs on the base64 length, before a single
    /// byte is decoded: a limit checked after the allocation is not a limit.
    #[test]
    fn a_payload_past_the_size_limit_is_refused_before_it_is_decoded() {
        let oversized = "A".repeat(1024);
        let payload = declared(SAMPLE_WIDTH, SAMPLE_HEIGHT, &oversized);
        let limits = IntakeLimits {
            max_png_bytes: 16,
            ..IntakeLimits::default()
        };

        let refusal =
            frame_from_png(&oversized, &payload, limits).expect_err("a huge payload is refused");

        assert!(
            refusal.contains("16"),
            "the refusal has to name the limit it enforced: {refusal}"
        );
    }

    /// I4. A payload that is not base64 at all fails as a payload that is not
    /// base64, rather than as an unrecognisable picture two steps later.
    #[test]
    fn a_payload_that_is_not_base64_is_refused_as_such() {
        let broken = "not base64!!";
        let payload = declared(SAMPLE_WIDTH, SAMPLE_HEIGHT, broken);

        let refusal = frame_from_png(broken, &payload, IntakeLimits::default())
            .expect_err("broken encoding is refused");

        assert!(
            refusal.contains("base64"),
            "the refusal has to say which step failed: {refusal}"
        );
    }

    /// I5. Security. The decompression bomb gate: a few hundred bytes of PNG
    /// can declare an image far larger than this machine's memory, and the
    /// decoder will try to allocate every pixel of it. Snapdeck is a menu bar
    /// agent, so the process that dies takes the tray and the user's next
    /// capture with it.
    #[test]
    fn a_picture_past_the_pixel_limit_is_refused_by_the_decoder() {
        let base64 = sample_base64();
        let payload = declared(SAMPLE_WIDTH, SAMPLE_HEIGHT, &base64);
        let limits = IntakeLimits {
            max_pixels: 4,
            ..IntakeLimits::default()
        };

        let refusal =
            frame_from_png(&base64, &payload, limits).expect_err("twelve pixels is past four");

        assert!(
            refusal.contains("4 pixels"),
            "the refusal has to name the budget the picture was measured against: {refusal}"
        );
    }

    /// I6. The user's format setting is a setting about their captures, and a
    /// page delivered over the bridge is one of their captures.
    #[test]
    fn the_format_setting_decides_what_a_delivered_page_is_written_as() {
        let directory = temp_dir("format");

        let png = deliver(
            &sample_frame(),
            &directory,
            &settings_with("as-png", SaveFormat::Png),
            |_| Ok(()),
        );
        let jpeg = deliver(
            &sample_frame(),
            &directory,
            &settings_with("as-jpeg", SaveFormat::Jpeg),
            |_| Ok(()),
        );

        let png_path = png.path.expect("the png is written");
        let jpeg_path = jpeg.path.expect("the jpeg is written");
        let jpeg_format = image::ImageReader::open(&jpeg_path)
            .expect("open the jpeg")
            .format()
            .expect("a recognised format");
        std::fs::remove_dir_all(&directory).ok();

        assert_eq!(png_path.extension().and_then(|e| e.to_str()), Some("png"));
        assert_eq!(jpeg_path.extension().and_then(|e| e.to_str()), Some("jpg"));
        assert_eq!(
            jpeg_format,
            image::ImageFormat::Jpeg,
            "a .jpg holding png bytes is the failure this asserts against"
        );
    }

    /// I7. The other half of the same claim, and the proof that this is the
    /// save path the application already had rather than a second one: the
    /// template is rendered, and a name already taken grows a suffix instead of
    /// destroying the file that holds it.
    #[test]
    fn the_template_names_the_file_and_a_second_page_does_not_overwrite_the_first() {
        let directory = temp_dir("template");
        let settings = settings_with("page {width}x{height}", SaveFormat::Png);

        let first = deliver(&sample_frame(), &directory, &settings, |_| Ok(()));
        let first_path = first.path.expect("the first page is written");
        let first_bytes = std::fs::read(&first_path).expect("read the first file");

        // A different picture at the same size, so it renders the same name and
        // a save that overwrote rather than suffixed would leave the first
        // file holding the second page's pixels.
        let second = deliver(&another_frame(), &directory, &settings, |_| Ok(()));
        let second_path = second.path.expect("the second page is written");
        let first_bytes_now = std::fs::read(&first_path).expect("the first file is still there");
        std::fs::remove_dir_all(&directory).ok();

        assert_eq!(
            first_bytes_now, first_bytes,
            "the first page is untouched by the second"
        );
        assert_eq!(
            first_path.file_name().and_then(|name| name.to_str()),
            Some("page 4x3.png"),
            "the template is the user's, tokens and all"
        );
        assert_eq!(
            second_path.file_name().and_then(|name| name.to_str()),
            Some("page 4x3 2.png"),
            "and a taken name is suffixed"
        );
    }

    /// I8. Half a delivery is still a delivery. A clipboard that refused the
    /// picture must not take the file with it; the user is told and keeps the
    /// file.
    #[test]
    fn a_failed_clipboard_still_leaves_the_file_on_disk() {
        let directory = temp_dir("clipboard");

        let delivered = deliver(
            &sample_frame(),
            &directory,
            &settings_with("clipboard", SaveFormat::Png),
            |_| Err("the clipboard is busy".to_owned()),
        );

        let path = delivered.path.clone();
        std::fs::remove_dir_all(&directory).ok();

        assert!(
            path.is_some(),
            "the file is written even though the clipboard refused: {delivered:?}"
        );
        let complaint = delivered
            .complaint
            .expect("a half delivery is something the user has to be told about");
        assert!(
            complaint.contains("clipboard"),
            "and the complaint has to say which half was lost: {complaint}"
        );
    }

    /// I9. The other half, and the one that used to throw away a capture the
    /// user had already asked for: a directory that cannot be written to takes
    /// the file, and nothing else. The clipboard is written first and keeps the
    /// picture one paste away.
    #[test]
    fn a_directory_that_cannot_be_written_to_does_not_cost_the_clipboard() {
        let root = temp_dir("unwritable");
        let blocker = root.join("a-file-not-a-directory");
        std::fs::write(&blocker, b"not a directory").expect("write the blocker");
        // A directory whose parent is a regular file: `create_dir_all` cannot
        // make it and `File::create_new` cannot open anything inside it, on any
        // machine and whatever the test process's privileges are.
        let directory = blocker.join("inside");

        let copied = Cell::new(false);
        let delivered = deliver(
            &sample_frame(),
            &directory,
            &settings_with("lost", SaveFormat::Png),
            |_| {
                copied.set(true);
                Ok(())
            },
        );
        std::fs::remove_dir_all(&root).ok();

        assert_eq!(delivered.path, None, "nothing reached the disk");
        assert!(
            copied.get(),
            "but the clipboard was written before the disk was tried, so the picture is not lost"
        );
        let complaint = delivered
            .complaint
            .expect("a page that reached only the clipboard is something the user has to be told");
        assert!(
            complaint.contains("clipboard"),
            "and the complaint has to tell them where the picture is: {complaint}"
        );
    }
}
