use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use objc2_app_kit::{NSStatusWindowLevel, NSWindow, NSWindowCollectionBehavior};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use snapdeck_capture::{
    macos::permission::{
        request_screen_capture_permission, screen_capture_permission, SETTINGS_DEEP_LINK,
    },
    CaptureTarget, DisplayInfo, ScreenCapturer,
};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::{
    output::{save_png, PngCompression},
    state::AppState,
};

const OVERLAY_LABEL_PREFIX: &str = "overlay-";

/// Characters that would end the query value or the URL itself. Everything
/// else in a POSIX path, `/` and `.` included, stays readable.
const PATH_QUERY_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'&')
    .add(b'+')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

/// How long the worker waits for the previous overlays to leave the window map.
const CLOSE_DRAIN_TIMEOUT: Duration = Duration::from_millis(500);
const CLOSE_DRAIN_POLL: Duration = Duration::from_millis(4);

pub fn overlay_label(display_id: u32) -> String {
    format!("{OVERLAY_LABEL_PREFIX}{display_id}")
}

pub fn frozen_frame_path(cache_dir: &Path, display_id: u32) -> PathBuf {
    cache_dir.join(format!("frozen-{display_id}.png"))
}

/// The overlay's URL. The frozen frame's absolute path travels with it so the
/// frontend only has to call `convertFileSrc`: no path IPC round trip, and the
/// `frozen-<id>.png` template is written once, in Rust.
pub fn overlay_url(display_id: u32, mode: &str, scale: f32, frame_path: &Path) -> String {
    let raw_path = frame_path.to_string_lossy();
    let path = utf8_percent_encode(&raw_path, PATH_QUERY_ENCODE_SET);
    format!("overlay.html?display={display_id}&mode={mode}&scale={scale}&path={path}")
}

/// Freezes every display and shows one full-screen overlay per display.
///
/// Returns as soon as the work is handed off. The capture and the PNG encode
/// run on a worker thread because every `ScreenCapturer` call blocks for a
/// full platform round trip; the windows are then built back on the main
/// thread, which is the only thread macOS lets create windows.
///
/// Nothing is reported to the caller because there is no caller left by the
/// time the work finishes. The one failure the user must see, a refused screen
/// recording permission, is surfaced by `ensure_permission` instead.
pub fn open_overlays(app: &AppHandle, mode: &str) {
    // Closing has to happen here, on the main thread's current turn, and once
    // for all displays. `WebviewWindow::close` only posts to the event loop
    // proxy, and a label leaves the window map when the `Destroyed` event
    // arrives, so a close issued inside a loop that itself blocks the main
    // thread can never be processed before the next `build` call: the second
    // capture would fail with `WindowLabelAlreadyExists`. Closing first also
    // keeps the previous overlay out of the new frozen frame.
    close_overlays(app);

    let app = app.clone();
    let mode = mode.to_string();
    std::thread::spawn(move || {
        if !ensure_permission() {
            return;
        }
        wait_for_overlays_to_close(&app);

        let frozen = match capture_frozen_frames(&app) {
            Ok(frozen) => frozen,
            Err(err) => {
                eprintln!("snapdeck: capture failed: {err}");
                return;
            }
        };

        let handle = app.clone();
        if let Err(err) = app.run_on_main_thread(move || {
            build_overlay_windows(&handle, &mode, &frozen);
        }) {
            eprintln!("snapdeck: could not reach the main thread: {err}");
        }
    });
}

/// Closes every overlay window. Also wired to Escape and to a finished
/// selection in Task 7.
pub fn close_overlays(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(OVERLAY_LABEL_PREFIX) {
            let _ = window.close();
        }
    }
}

/// Whether capture may proceed, prompting or pointing at System Settings when
/// it may not.
///
/// `eprintln!` reaches nobody here: the app is `LSUIElement` with no windows
/// declared, so it has neither a console the user reads nor a window to raise.
/// The first call prompts; afterwards macOS stays silent, which is why a
/// still-denied permission opens the settings pane instead.
///
/// A granted permission is a necessary condition, never a sufficient one: a
/// running process cannot see a grant made after it started, so every
/// ScreenCaptureKit call can still fail until the app is relaunched. Capture
/// errors are therefore reported verbatim rather than reinterpreted here.
fn ensure_permission() -> bool {
    if screen_capture_permission().is_granted() {
        return true;
    }
    if request_screen_capture_permission().is_granted() {
        return true;
    }
    if let Err(err) = std::process::Command::new("open")
        .arg(SETTINGS_DEEP_LINK)
        .spawn()
    {
        eprintln!("snapdeck: failed to open the screen recording settings: {err}");
    }
    false
}

/// Waits for the closes queued by `open_overlays` to reach `Destroyed`, which
/// is when Tauri drops the label from the window map and when the old overlay
/// actually leaves the screen. Both matter: the label has to be free before
/// `build`, and the pixels have to be gone before `capture`.
fn wait_for_overlays_to_close(app: &AppHandle) {
    let deadline = Instant::now() + CLOSE_DRAIN_TIMEOUT;
    while has_overlay_windows(app) {
        if Instant::now() >= deadline {
            eprintln!("snapdeck: previous overlays did not close within {CLOSE_DRAIN_TIMEOUT:?}");
            return;
        }
        std::thread::sleep(CLOSE_DRAIN_POLL);
    }
}

fn has_overlay_windows(app: &AppHandle) -> bool {
    app.webview_windows()
        .keys()
        .any(|label| label.starts_with(OVERLAY_LABEL_PREFIX))
}

/// Captures every display and writes each frame to the cache directory.
/// Blocking, worker thread only.
fn capture_frozen_frames(app: &AppHandle) -> Result<Vec<(DisplayInfo, PathBuf)>, String> {
    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("no cache dir: {e}"))?;
    std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;

    let state = app.state::<AppState>();
    let displays = state.capturer.displays().map_err(|e| e.to_string())?;

    let mut frozen = Vec::with_capacity(displays.len());
    for display in displays {
        let frame = state
            .capturer
            .capture(CaptureTarget::Display(display.id))
            .map_err(|e| e.to_string())?;
        let path = frozen_frame_path(&cache_dir, display.id);
        save_png(&frame, &path, PngCompression::Fast)?;
        frozen.push((display, path));
    }
    Ok(frozen)
}

/// Main thread only: macOS requires window creation there.
fn build_overlay_windows(app: &AppHandle, mode: &str, frozen: &[(DisplayInfo, PathBuf)]) {
    let mut failed = false;
    for (display, path) in frozen {
        match build_overlay_window(app, mode, display, path) {
            Ok(window) => raise_above_menu_bar(&window),
            Err(err) => {
                eprintln!(
                    "snapdeck: failed to create the overlay for display {}: {err}",
                    display.id
                );
                failed = true;
            }
        }
    }
    // A half-open set of overlays is worse than none: the windows that did open
    // swallow every click with no way to dismiss them before Task 7's Escape.
    if failed {
        close_overlays(app);
    }
}

fn build_overlay_window(
    app: &AppHandle,
    mode: &str,
    display: &DisplayInfo,
    frame_path: &Path,
) -> tauri::Result<WebviewWindow> {
    let url = overlay_url(display.id, mode, display.scale_factor, frame_path);
    // Points, not pixels: `position` and `inner_size` take logical coordinates,
    // and `DisplayInfo::bounds` is already in the global point space, so the
    // frozen frame lines up with the live screen on Retina.
    WebviewWindowBuilder::new(app, overlay_label(display.id), WebviewUrl::App(url.into()))
        .position(display.bounds.x, display.bounds.y)
        .inner_size(display.bounds.width, display.bounds.height)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .resizable(false)
        .focused(true)
        // Shown by the frontend once the frozen frame has painted. A visible
        // window would otherwise be a fully transparent, click-swallowing
        // rectangle over a screen that is still moving, which is the opposite
        // of what freezing is for.
        .visible(false)
        .build()
}

/// Lifts the overlay over the menu bar and onto every Space.
///
/// `always_on_top` leaves the window at CGWindowLevel 5, below the Dock (20)
/// and the menu bar (24), so both draw over the frozen frame and the top and
/// bottom strips of the screen cannot be selected. `CanJoinAllSpaces` keeps the
/// overlay present when the user switches Space, and `FullScreenAuxiliary` is
/// what lets it appear over a fullscreen Space at all.
fn raise_above_menu_bar(window: &WebviewWindow) {
    let handle = match window.ns_window() {
        Ok(handle) => handle,
        Err(err) => {
            eprintln!("snapdeck: no NSWindow for {}: {err}", window.label());
            return;
        }
    };
    // SAFETY: `ns_window` hands back this window's live `NSWindow`, which
    // outlives the borrow, and this runs on the main thread, where AppKit
    // requires every `NSWindow` call to happen.
    let ns_window: &NSWindow = unsafe { &*(handle as *const NSWindow) };
    ns_window.setLevel(NSStatusWindowLevel);
    ns_window.setCollectionBehavior(
        ns_window.collectionBehavior()
            | NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
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
    fn overlay_url_carries_display_mode_scale_and_frame_path() {
        let url = overlay_url(3, "region", 2.0, Path::new("/tmp/cache/frozen-3.png"));
        assert_eq!(
            url,
            "overlay.html?display=3&mode=region&scale=2&path=/tmp/cache/frozen-3.png"
        );
    }

    #[test]
    fn overlay_url_escapes_characters_that_would_end_the_query() {
        let url = overlay_url(1, "region", 1.0, Path::new("/tmp/a b&c#d/frozen-1.png"));
        assert!(
            url.ends_with("&path=/tmp/a%20b%26c%23d/frozen-1.png"),
            "unexpected url: {url}"
        );
    }
}
