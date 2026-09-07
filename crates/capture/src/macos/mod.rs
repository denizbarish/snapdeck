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

/// ScreenCaptureKit-backed capturer. Requires macOS 15.2 or newer: region
/// capture goes through `SCScreenshotManager::capture_image_in_rect`, which
/// Apple added in 15.2.
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

fn map_err(err: SCError) -> CaptureError {
    match err {
        // Unambiguous: ScreenCaptureKit named the cause itself.
        SCError::PermissionDenied(_)
        | SCError::SCStreamError {
            code: SCStreamErrorCode::UserDeclined,
            ..
        } => CaptureError::PermissionDenied,
        // A failed shareable-content request arrives as this variant carrying
        // only NSError's localizedDescription, which macOS translates into the
        // user's language, so the message text cannot be matched. Missing
        // screen recording approval is what makes that request fail, so the
        // variant is the signal. Every capture path queries shareable content
        // first, which keeps a denial from reaching the caller as a platform
        // error or an empty frame.
        SCError::NoShareableContent(_) => CaptureError::PermissionDenied,
        other => {
            let text = other.to_string();
            // Only reachable when macOS runs in English; a best-effort last
            // resort, not the primary check.
            if text.contains("not authorized")
                || text.contains("declined")
                || text.contains("-3801")
            {
                CaptureError::PermissionDenied
            } else {
                CaptureError::Platform(text)
            }
        }
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
                let scale = self
                    .displays()?
                    .into_iter()
                    .find(|d| d.bounds.intersect(&rect).is_some())
                    .map(|d| d.scale_factor)
                    .unwrap_or(1.0);
                let cg_rect = CGRect {
                    origin: CGPoint {
                        x: rect.x,
                        y: rect.y,
                    },
                    size: CGSize {
                        width: rect.width,
                        height: rect.height,
                    },
                };
                let image = SCScreenshotManager::capture_image_in_rect(cg_rect).map_err(map_err)?;
                frame_from_image(&image, scale)
            }
            CaptureTarget::Display(id) => {
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
                    .with_width((display.width() as f32 * scale) as u32)
                    .with_height((display.height() as f32 * scale) as u32)
                    .with_shows_cursor(false);
                let image =
                    SCScreenshotManager::capture_image(&filter, &config).map_err(map_err)?;
                frame_from_image(&image, scale)
            }
            CaptureTarget::Window(id) => {
                let content = SCShareableContent::get().map_err(map_err)?;
                let window = content
                    .windows()
                    .into_iter()
                    .find(|w| w.window_id() == id)
                    .ok_or_else(|| CaptureError::TargetNotFound(format!("window {id}")))?;
                let bounds = to_rect(window.frame());
                let scale = self
                    .displays()?
                    .into_iter()
                    .find(|d| d.bounds.intersect(&bounds).is_some())
                    .map(|d| d.scale_factor)
                    .unwrap_or(1.0);
                let filter = SCContentFilter::create().with_window(&window).build();
                let config = SCStreamConfiguration::new()
                    .with_width((bounds.width * scale as f64) as u32)
                    .with_height((bounds.height * scale as f64) as u32)
                    .with_shows_cursor(false);
                let image =
                    SCScreenshotManager::capture_image(&filter, &config).map_err(map_err)?;
                frame_from_image(&image, scale)
            }
        }
    }
}
