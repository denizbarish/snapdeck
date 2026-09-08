use std::path::Path;

use snapdeck_capture::Frame;

/// Writes a frame as PNG. Row padding is removed and BGRA is converted by
/// `Frame::to_rgba8`, so the file is always tightly packed RGBA.
pub fn save_png(frame: &Frame, path: &Path) -> Result<(), String> {
    let rgba = frame.to_rgba8().map_err(|e| e.to_string())?;
    image::save_buffer(
        path,
        &rgba,
        frame.width,
        frame.height,
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|e| format!("failed to save png: {e}"))
}
