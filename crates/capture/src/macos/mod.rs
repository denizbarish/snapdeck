// Task 4 adds: pub mod permission;

use core_graphics::display::{CGDisplay, CGMainDisplayID};
use screencapturekit::error::SCStreamErrorCode;
use screencapturekit::prelude::*;
use screencapturekit::screenshot_manager::{CGImageExt, SCScreenshotManager};
use screencapturekit::CGImage;

use crate::{
    error::CaptureError,
    types::{CaptureTarget, DisplayInfo, Frame, PixelFormat, Rect, WindowInfo},
    ScreenCapturer,
};

/// ScreenCaptureKit-backed capturer. Every capture path goes through
/// `SCScreenshotManager`, which Apple added in macOS 14.0.
pub struct MacCapturer;

impl MacCapturer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MacCapturer {
    fn default() -> Self {
        Self::new()
    }
}

/// Translates a ScreenCaptureKit failure into a capture error.
///
/// Only the variant is inspected, never the message. macOS localizes
/// `NSError`'s `localizedDescription`, so a Turkish system reports declined
/// consent in Turkish and any substring test would misclassify a denial as a
/// generic platform failure on every non-English system.
fn map_err(err: SCError) -> CaptureError {
    match err {
        // Unambiguous: ScreenCaptureKit named the cause itself.
        SCError::PermissionDenied(_)
        | SCError::SCStreamError {
            code: SCStreamErrorCode::UserDeclined,
            ..
        } => CaptureError::PermissionDenied,
        // A failed shareable-content request arrives as this variant carrying
        // only the localized description, so the variant itself is the signal.
        // Missing screen recording approval is what makes that request fail.
        // This over-reports a transient XPC failure as a permission problem;
        // Task 4 revisits it once a real preflight check exists.
        SCError::NoShareableContent(_) => CaptureError::PermissionDenied,
        other => CaptureError::Platform(other.to_string()),
    }
}

fn to_rect(frame: CGRect) -> Rect {
    Rect {
        x: frame.origin.x,
        y: frame.origin.y,
        width: frame.size.width,
        height: frame.size.height,
    }
}

/// Pixels per point for a display, read from its current display mode.
fn scale_factor_for(display_id: u32) -> f32 {
    let display = CGDisplay::new(display_id);
    match display.display_mode() {
        Some(mode) if mode.width() > 0 => mode.pixel_width() as f32 / mode.width() as f32,
        _ => 1.0,
    }
}

/// `sourceRect` for a region, expressed in the target display's own
/// coordinate space.
///
/// Regions arrive in the global point space shared by every display, but
/// ScreenCaptureKit measures `sourceRect` from the display's own origin. A
/// display placed left of or above the primary one carries a negative origin,
/// which is exactly what has to be subtracted out.
fn source_rect_for(region: Rect, display_bounds: Rect) -> CGRect {
    CGRect {
        origin: CGPoint {
            x: region.x - display_bounds.x,
            y: region.y - display_bounds.y,
        },
        size: CGSize {
            width: region.width,
            height: region.height,
        },
    }
}

/// Index of the candidate rectangle that overlaps `region` the most, or `None`
/// when none of them overlap it.
///
/// `SCShareableContent::displays()` has no documented order, so taking the
/// first intersecting display would leave the winner to chance whenever a
/// rectangle touches two of them.
fn largest_overlap_index(region: Rect, candidates: &[Rect]) -> Option<usize> {
    candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            candidate
                .intersect(&region)
                .map(|overlap| (index, overlap.width * overlap.height))
        })
        .max_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(index, _)| index)
}

/// Whether `region` lies entirely inside `bounds`.
fn contains(bounds: Rect, region: Rect) -> bool {
    region.x >= bounds.x
        && region.y >= bounds.y
        && region.x + region.width <= bounds.x + bounds.width
        && region.y + region.height <= bounds.y + bounds.height
}

/// Pixel count for a length in points.
///
/// Rounds instead of truncating, because truncation loses up to a pixel per
/// axis on a fractional selection, and never returns zero, because a
/// sub-point region passes `Rect::is_empty` and would otherwise ask
/// ScreenCaptureKit for a 0-wide capture.
fn pixels(points: f64, scale: f32) -> u32 {
    let scaled = (points * f64::from(scale)).round();
    if scaled < 1.0 {
        1
    } else {
        scaled as u32
    }
}

fn frame_from_image(image: &CGImage, scale_factor: f32) -> Result<Frame, CaptureError> {
    let data = image.rgba_data().map_err(map_err)?;
    let width = image.width() as u32;
    let height = image.height() as u32;
    Ok(Frame {
        data,
        width,
        height,
        // rgba_data returns tightly packed rows.
        stride: width as usize * 4,
        pixel_format: PixelFormat::Rgba8,
        scale_factor,
        captured_at: std::time::SystemTime::now(),
    })
}

impl ScreenCapturer for MacCapturer {
    fn displays(&self) -> Result<Vec<DisplayInfo>, CaptureError> {
        // Load-bearing for the permission contract: see `map_err`. A missing
        // grant has to fail here, never later.
        let content = SCShareableContent::get().map_err(map_err)?;
        let main_id = unsafe { CGMainDisplayID() };
        Ok(content
            .displays()
            .into_iter()
            .map(|d| {
                let id = d.display_id();
                DisplayInfo {
                    id,
                    bounds: to_rect(d.frame()),
                    scale_factor: scale_factor_for(id),
                    is_primary: id == main_id,
                }
            })
            .collect())
    }

    fn windows(&self) -> Result<Vec<WindowInfo>, CaptureError> {
        // Load-bearing for the permission contract: see `map_err`. A missing
        // grant has to fail here, never later.
        let content = SCShareableContent::get().map_err(map_err)?;
        Ok(content
            .windows()
            .into_iter()
            .filter(|w| w.is_on_screen())
            .map(|w| WindowInfo {
                id: w.window_id(),
                title: w.title(),
                app_name: w.owning_application().map(|a| a.application_name()),
                bounds: to_rect(w.frame()),
                layer: w.window_layer(),
                is_on_screen: true,
            })
            .collect())
    }

    fn capture(&self, target: CaptureTarget) -> Result<Frame, CaptureError> {
        match target {
            CaptureTarget::Region(rect) => {
                if rect.is_empty() {
                    return Err(CaptureError::Platform("empty region".to_string()));
                }
                // Load-bearing for the permission contract: a missing screen
                // recording grant fails here as `NoShareableContent`, which
                // `map_err` turns into `PermissionDenied`. Every failure of
                // `SCScreenshotManager::capture_image` collapses into the flat
                // `SCError::ScreenshotError`, so a denial that first surfaced
                // there would reach the caller as `CaptureError::Platform`. Do
                // not cache or skip this call.
                let content = SCShareableContent::get().map_err(map_err)?;
                let displays = content.displays();
                let display_bounds: Vec<Rect> =
                    displays.iter().map(|d| to_rect(d.frame())).collect();
                // The region arrives in global points. Capture it from the
                // display it covers the most.
                let index = largest_overlap_index(rect, &display_bounds).ok_or_else(|| {
                    CaptureError::TargetNotFound("no display intersects the region".to_string())
                })?;
                let display = &displays[index];
                let bounds = display_bounds[index];
                // A region has to lie inside a single display. The overlay
                // opens one window per display and clamps the selection to
                // that window, so the UI already guarantees it. Refuse
                // anything wider instead of clamping: ScreenCaptureKit clips
                // `sourceRect` to the display but still stretches the result
                // to the requested output size, so a straddling region would
                // return a partly black, mis-scaled frame that looks like a
                // successful capture.
                if !contains(bounds, rect) {
                    return Err(CaptureError::Platform(
                        "region is not contained in a single display".to_string(),
                    ));
                }
                let scale = scale_factor_for(display.display_id());
                let filter = SCContentFilter::create()
                    .with_display(display)
                    .with_excluding_windows(&[])
                    .build();
                let config = SCStreamConfiguration::new()
                    .with_source_rect(source_rect_for(rect, bounds))
                    .with_width(pixels(rect.width, scale))
                    .with_height(pixels(rect.height, scale))
                    .with_shows_cursor(false);
                let image =
                    SCScreenshotManager::capture_image(&filter, &config).map_err(map_err)?;
                frame_from_image(&image, scale)
            }
            CaptureTarget::Display(id) => {
                // Load-bearing for the permission contract: see `map_err`. A
                // missing grant has to fail here, never later.
                let content = SCShareableContent::get().map_err(map_err)?;
                let display = content
                    .displays()
                    .into_iter()
                    .find(|d| d.display_id() == id)
                    .ok_or_else(|| CaptureError::TargetNotFound(format!("display {id}")))?;
                let scale = scale_factor_for(id);
                let filter = SCContentFilter::create()
                    .with_display(&display)
                    .with_excluding_windows(&[])
                    .build();
                let config = SCStreamConfiguration::new()
                    .with_width(pixels(f64::from(display.width()), scale))
                    .with_height(pixels(f64::from(display.height()), scale))
                    .with_shows_cursor(false);
                let image =
                    SCScreenshotManager::capture_image(&filter, &config).map_err(map_err)?;
                frame_from_image(&image, scale)
            }
            CaptureTarget::Window(id) => {
                // Load-bearing for the permission contract: see `map_err`. A
                // missing grant has to fail here, never later.
                let content = SCShareableContent::get().map_err(map_err)?;
                let window = content
                    .windows()
                    .into_iter()
                    .find(|w| w.window_id() == id)
                    .ok_or_else(|| CaptureError::TargetNotFound(format!("window {id}")))?;
                let bounds = to_rect(window.frame());
                // Same reasoning as the region arm: a window straddling two
                // displays takes the scale factor of the one it sits on the
                // most, not of whichever display the list happens to name
                // first.
                let displays = content.displays();
                let display_bounds: Vec<Rect> =
                    displays.iter().map(|d| to_rect(d.frame())).collect();
                let scale = largest_overlap_index(bounds, &display_bounds)
                    .map(|index| scale_factor_for(displays[index].display_id()))
                    .unwrap_or(1.0);
                let filter = SCContentFilter::create().with_window(&window).build();
                let config = SCStreamConfiguration::new()
                    .with_width(pixels(bounds.width, scale))
                    .with_height(pixels(bounds.height, scale))
                    .with_shows_cursor(false);
                let image =
                    SCScreenshotManager::capture_image(&filter, &config).map_err(map_err)?;
                frame_from_image(&image, scale)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn source_rect_is_relative_to_the_display_origin() {
        // A region inside the primary display, away from its origin.
        let got = source_rect_for(
            rect(300.0, 200.0, 100.0, 50.0),
            rect(0.0, 0.0, 1920.0, 1080.0),
        );
        assert_eq!(
            got,
            CGRect {
                origin: CGPoint { x: 300.0, y: 200.0 },
                size: CGSize {
                    width: 100.0,
                    height: 50.0
                },
            }
        );
    }

    #[test]
    fn source_rect_subtracts_a_negative_horizontal_display_origin() {
        // Secondary display placed to the left of the primary one.
        let got = source_rect_for(
            rect(-1620.0, 100.0, 100.0, 50.0),
            rect(-1920.0, 0.0, 1920.0, 1080.0),
        );
        assert_eq!(
            got,
            CGRect {
                origin: CGPoint { x: 300.0, y: 100.0 },
                size: CGSize {
                    width: 100.0,
                    height: 50.0
                },
            }
        );
    }

    #[test]
    fn source_rect_subtracts_a_negative_vertical_display_origin() {
        // Secondary display stacked above the primary one.
        let got = source_rect_for(
            rect(50.0, -880.0, 100.0, 50.0),
            rect(0.0, -1080.0, 1920.0, 1080.0),
        );
        assert_eq!(
            got,
            CGRect {
                origin: CGPoint { x: 50.0, y: 200.0 },
                size: CGSize {
                    width: 100.0,
                    height: 50.0
                },
            }
        );
    }

    #[test]
    fn largest_overlap_wins_over_list_order() {
        // The region touches both displays but sits mostly on the second one.
        let displays = [
            rect(-1920.0, 0.0, 1920.0, 1080.0),
            rect(0.0, 0.0, 1920.0, 1080.0),
        ];
        let region = rect(-20.0, 100.0, 100.0, 50.0);
        assert_eq!(largest_overlap_index(region, &displays), Some(1));
    }

    #[test]
    fn largest_overlap_is_none_when_nothing_intersects() {
        let displays = [rect(0.0, 0.0, 1920.0, 1080.0)];
        assert_eq!(
            largest_overlap_index(rect(5000.0, 5000.0, 10.0, 10.0), &displays),
            None
        );
    }

    #[test]
    fn containment_rejects_a_region_reaching_past_the_display() {
        let display = rect(0.0, 0.0, 1920.0, 1080.0);
        assert!(contains(display, rect(1820.0, 0.0, 100.0, 50.0)));
        assert!(!contains(display, rect(1870.0, 0.0, 100.0, 50.0)));
        assert!(!contains(display, rect(-10.0, 0.0, 100.0, 50.0)));
    }

    #[test]
    fn containment_rejects_a_region_reaching_past_the_display_vertically() {
        // A display stacked above and left of the primary one, so both
        // vertical terms are measured against a non-zero origin rather than
        // against zero, where a y/x copy-paste slip would go unnoticed.
        let display = rect(-1920.0, -1080.0, 1920.0, 1080.0);
        // Starts above the top edge.
        assert!(!contains(display, rect(-1820.0, -1090.0, 100.0, 50.0)));
        // Reaches past the bottom edge.
        assert!(!contains(display, rect(-1820.0, -40.0, 100.0, 50.0)));
        // Ends exactly on the bottom edge.
        assert!(contains(display, rect(-1820.0, -50.0, 100.0, 50.0)));
    }

    #[test]
    fn maps_a_named_permission_denial() {
        assert_eq!(
            map_err(SCError::PermissionDenied("Screen Recording".to_string())),
            CaptureError::PermissionDenied
        );
    }

    #[test]
    fn maps_the_user_declined_stream_error_code() {
        assert_eq!(
            map_err(SCError::SCStreamError {
                code: SCStreamErrorCode::UserDeclined,
                message: None,
            }),
            CaptureError::PermissionDenied
        );
    }

    #[test]
    fn maps_a_non_declined_stream_error_code_to_platform() {
        // UserDeclined is the only stream error code that means consent was
        // refused. Widening the arm to the whole variant would report every
        // stream failure as a permission problem.
        let err = SCError::SCStreamError {
            code: SCStreamErrorCode::FailedToStart,
            message: Some("stream failed to start".to_string()),
        };
        let message = err.to_string();
        assert_eq!(map_err(err), CaptureError::Platform(message));
    }

    #[test]
    fn maps_a_failed_shareable_content_request() {
        assert_eq!(
            map_err(SCError::NoShareableContent("xpc failed".to_string())),
            CaptureError::PermissionDenied
        );
    }

    #[test]
    fn maps_every_other_variant_to_platform_whatever_the_message_says() {
        // The message deliberately carries wording an English substring test
        // would have matched. Only the variant decides. The message is the
        // only diagnostic such a failure carries, so it has to survive the
        // mapping intact.
        let err = SCError::ScreenshotError("the user declined the request".to_string());
        let message = err.to_string();
        assert_eq!(map_err(err), CaptureError::Platform(message));
    }

    #[test]
    fn pixel_lengths_round_to_nearest_and_never_reach_zero() {
        // Rounds rather than truncating or always climbing.
        assert_eq!(pixels(100.4, 1.0), 100);
        // Half away from zero, not half to even.
        assert_eq!(pixels(100.5, 1.0), 101);
        // A sub-point length still asks for one pixel, never zero.
        assert_eq!(pixels(0.4, 1.0), 1);
    }
}
