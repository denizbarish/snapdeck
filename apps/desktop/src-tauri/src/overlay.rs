use std::path::{Path, PathBuf};

use snapdeck_capture::{
    macos::permission::screen_capture_permission, CaptureTarget, ScreenCapturer,
};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::{output::save_png, state::AppState};

pub fn overlay_label(display_id: u32) -> String {
    format!("overlay-{display_id}")
}

pub fn frozen_frame_path(cache_dir: &Path, display_id: u32) -> PathBuf {
    cache_dir.join(format!("frozen-{display_id}.png"))
}

pub fn overlay_url(display_id: u32, mode: &str, scale: f32) -> String {
    format!("overlay.html?display={display_id}&mode={mode}&scale={scale}")
}

/// Captures every display, writes each frame to the cache directory, and opens
/// one transparent always-on-top window per display showing that frame.
///
/// The preflight below is a necessary condition, never a sufficient one: a
/// running process cannot see a screen recording grant made after it started,
/// so `screen_capture_permission` can report `Granted` while every
/// ScreenCaptureKit call still fails until the app is relaunched. The capture
/// errors are therefore reported verbatim, and a `Platform` error coming out of
/// `displays` or `capture` must not be read as proof that the grant is usable.
///
/// Every `ScreenCapturer` call blocks the caller for a full platform round
/// trip, and both call sites (the tray menu and the global shortcut handler)
/// are on the Tauri main thread, so this freezes the UI for the length of the
/// capture.
pub fn open_overlays(app: &AppHandle, mode: &str) -> Result<(), String> {
    if !screen_capture_permission().is_granted() {
        // The settings window owns the permission guidance UI.
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }
        return Err("screen recording permission denied".to_string());
    }

    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("no cache dir: {e}"))?;
    std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;

    let state = app.state::<AppState>();
    let displays = state.capturer.displays().map_err(|e| e.to_string())?;

    for display in displays {
        let frame = state
            .capturer
            .capture(CaptureTarget::Display(display.id))
            .map_err(|e| e.to_string())?;
        save_png(&frame, &frozen_frame_path(&cache_dir, display.id))?;

        let label = overlay_label(display.id);
        if let Some(existing) = app.get_webview_window(&label) {
            let _ = existing.close();
        }

        // Points, not pixels: `position` and `inner_size` take logical
        // coordinates, and `DisplayInfo::bounds` is already in the global point
        // space, so the frozen frame lines up with the live screen on Retina.
        WebviewWindowBuilder::new(
            app,
            &label,
            WebviewUrl::App(overlay_url(display.id, mode, display.scale_factor).into()),
        )
        .position(display.bounds.x, display.bounds.y)
        .inner_size(display.bounds.width, display.bounds.height)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .resizable(false)
        .focused(true)
        .build()
        .map_err(|e| format!("failed to create overlay window: {e}"))?;
    }

    Ok(())
}

/// Closes every overlay window. Wired to Escape and to a finished selection in
/// Task 7, which is the first caller.
#[allow(dead_code)]
pub fn close_overlays(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with("overlay-") {
            let _ = window.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn overlay_label_is_unique_per_display() {
        assert_eq!(overlay_label(1), "overlay-1");
        assert_ne!(overlay_label(1), overlay_label(2));
    }

    #[test]
    fn frozen_frame_path_lives_in_cache_dir() {
        let path = frozen_frame_path(Path::new("/tmp/cache"), 7);
        assert_eq!(path, Path::new("/tmp/cache/frozen-7.png"));
    }

    #[test]
    fn overlay_url_carries_display_mode_and_scale() {
        let url = overlay_url(3, "region", 2.0);
        assert_eq!(url, "overlay.html?display=3&mode=region&scale=2");
    }
}
