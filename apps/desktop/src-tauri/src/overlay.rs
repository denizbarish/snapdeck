use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use objc2::MainThreadMarker;
#[cfg(target_os = "macos")]
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
    state::{AppState, CaptureGuard},
};

const OVERLAY_LABEL_PREFIX: &str = "overlay-";
/// The frozen frame's filename, written once here and never in TypeScript.
const FROZEN_FRAME_PREFIX: &str = "frozen-";
const FROZEN_FRAME_EXTENSION: &str = ".png";

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

/// Whether the screen recording pane has already been opened in this process.
///
/// Impatient repeat triggers would otherwise steal focus once per press.
static SETTINGS_PANE_OPENED: AtomicBool = AtomicBool::new(false);

pub fn overlay_label(display_id: u32) -> String {
    format!("{OVERLAY_LABEL_PREFIX}{display_id}")
}

pub fn frozen_frame_path(cache_dir: &Path, display_id: u32) -> PathBuf {
    cache_dir.join(format!(
        "{FROZEN_FRAME_PREFIX}{display_id}{FROZEN_FRAME_EXTENSION}"
    ))
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
    // One capture at a time. Claimed before anything else happens, so a second
    // trigger arriving during the capture leaves the first one's overlays and
    // its half-written frozen frame alone. See `AppState::begin_capture`.
    let Some(guard) = app.state::<AppState>().begin_capture() else {
        return;
    };

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
        // A panic here would otherwise be completely silent: nothing joins this
        // thread, and the user would see the same nothing as a refused
        // permission. The guard is moved in, so the slot is freed either way.
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
            run_capture(&app, &mode, guard);
        }));
        if let Err(payload) = outcome {
            eprintln!(
                "snapdeck: the capture worker panicked: {}",
                panic_text(payload.as_ref())
            );
            let handle = app.clone();
            // Any overlay that did get built is unusable now, and Task 7's
            // Escape does not exist yet.
            let _ = app.run_on_main_thread(move || close_overlays(&handle));
        }
    });
}

/// The worker thread's whole job: check permission, wait for the old overlays
/// to leave, capture, then hand the windows back to the main thread.
///
/// The guard travels all the way into the main-thread closure so the capture
/// slot stays claimed until the new windows exist. Releasing it when this
/// function returns would reopen the label race it is there to prevent, because
/// `run_on_main_thread` only queues the closure.
fn run_capture(app: &AppHandle, mode: &str, guard: CaptureGuard) {
    if !ensure_permission() {
        return;
    }
    if !wait_for_overlays_to_close(app) {
        eprintln!(
            "snapdeck: previous overlays did not close within {CLOSE_DRAIN_TIMEOUT:?}, capture aborted"
        );
        return;
    }

    let frozen = match capture_frozen_frames(app) {
        Ok(frozen) => frozen,
        Err(err) => {
            eprintln!("snapdeck: capture failed: {err}");
            return;
        }
    };

    let handle = app.clone();
    let mode = mode.to_string();
    if let Err(err) = app.run_on_main_thread(move || {
        build_overlay_windows(&handle, &mode, &frozen);
        drop(guard);
    }) {
        eprintln!("snapdeck: could not reach the main thread: {err}");
    }
}

/// Best effort rendering of a panic payload, which is `&str` or `String` for
/// every panic raised by this crate or by the standard library.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "non-string panic payload".to_string()
}

/// Closes every overlay window and discards the frozen frames behind them.
/// Also wired to Escape and to a finished selection in Task 7.
pub fn close_overlays(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(OVERLAY_LABEL_PREFIX) {
            let _ = window.close();
        }
    }
    discard_frozen_frames(app);
}

/// Removes the frozen frames from the application cache.
///
/// Each one is a full-resolution, lossless copy of everything that was on the
/// user's screen, so leaving them in `~/Library/Caches` to accumulate is a
/// privacy cost with no upside: they are throwaway backdrops that the next
/// capture rewrites anyway. Safe to call while the overlays are still on
/// screen, because the webview has already decoded the file into memory by the
/// time it is visible.
fn discard_frozen_frames(app: &AppHandle) {
    let Ok(cache_dir) = app.path().app_cache_dir() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&cache_dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_str()
            .is_some_and(is_frozen_frame_filename)
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Whether a cache entry is one of our frozen frames. Deliberately strict: the
/// application cache is shared with Tauri's own plugins, so only the exact
/// `frozen-<digits>.png` shape this module writes may be deleted.
fn is_frozen_frame_filename(name: &str) -> bool {
    name.strip_prefix(FROZEN_FRAME_PREFIX)
        .and_then(|rest| rest.strip_suffix(FROZEN_FRAME_EXTENSION))
        .is_some_and(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
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
    open_screen_recording_settings();
    false
}

/// Opens the screen recording pane, at most once per process.
///
/// Opening it on every denied trigger would steal focus once per press, and
/// three impatient presses would raise System Settings three times. Once is
/// enough to satisfy the rule this exists for: the user has been shown where
/// to go.
fn open_screen_recording_settings() {
    if SETTINGS_PANE_OPENED.swap(true, Ordering::Relaxed) {
        return;
    }
    // `status` rather than `spawn`: nothing ever waits on a spawned child, so
    // it would linger as a zombie for the life of the process. This runs on the
    // worker thread, where blocking for `open` to exit costs nothing.
    match std::process::Command::new("open")
        .arg(SETTINGS_DEEP_LINK)
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("snapdeck: `open` exited with {status}"),
        Err(err) => eprintln!("snapdeck: failed to open the screen recording settings: {err}"),
    }
}

/// Waits for the closes queued by `open_overlays` to reach `Destroyed`, which
/// is when Tauri drops the label from the window map and when the old overlay
/// actually leaves the screen. Both matter: the label has to be free before
/// `build`, and the pixels have to be gone before `capture`.
///
/// Returns whether the map actually drained. A timeout aborts the capture: a
/// frozen frame taken now would have the previous overlay baked into it, which
/// is exactly the recursive screenshot this wait exists to prevent, and
/// `build` would then fail on the still-taken label anyway.
fn wait_for_overlays_to_close(app: &AppHandle) -> bool {
    let deadline = Instant::now() + CLOSE_DRAIN_TIMEOUT;
    while has_overlay_windows(app) {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(CLOSE_DRAIN_POLL);
    }
    true
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
        // tao's `set_visible_on_all_workspaces` sets exactly the
        // `CanJoinAllSpaces` collection-behaviour bit, so the bit is owned here
        // and `raise_above_menu_bar` never touches it.
        .visible_on_all_workspaces(true)
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

/// Lifts the overlay over the menu bar and lets it sit over a full-screen
/// Space.
///
/// `always_on_top` leaves the window at CGWindowLevel 5, below the Dock (20)
/// and the menu bar (24), so both draw over the frozen frame and the top and
/// bottom strips of the screen cannot be selected. `FullScreenAuxiliary` is
/// what lets the window appear over a full-screen Space at all; joining every
/// Space is the window builder's job, not this function's.
#[cfg(target_os = "macos")]
fn raise_above_menu_bar(window: &WebviewWindow) {
    let handle = match window.ns_window() {
        Ok(handle) => handle,
        Err(err) => {
            eprintln!("snapdeck: no NSWindow for {}: {err}", window.label());
            return;
        }
    };
    // `NSWindow` is a main-thread-only class in objc2, and taking a reference
    // from a raw pointer skips the marker that would normally prove it. Asking
    // for the marker restores that proof: this is called from
    // `build_overlay_windows`, which itself runs inside `run_on_main_thread`,
    // so failing here means a caller broke that contract.
    let _mtm = MainThreadMarker::new().expect("AppKit requires the main thread");
    // SAFETY: `ns_window` hands back this window's live `NSWindow`, which
    // outlives the borrow, and `_mtm` proves this is the thread AppKit
    // requires for every `NSWindow` call.
    let ns_window: &NSWindow = unsafe { &*handle.cast::<NSWindow>() };
    ns_window.setLevel(NSStatusWindowLevel);
    ns_window.setCollectionBehavior(
        ns_window.collectionBehavior() | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
}

#[cfg(not(target_os = "macos"))]
fn raise_above_menu_bar(_window: &WebviewWindow) {}

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

    /// The cleanup unlinks files from a directory Tauri's plugins also write
    /// to, so the match has to be exactly what `frozen_frame_path` produces.
    #[test]
    fn only_this_modules_frozen_frames_are_deleted() {
        for name in ["frozen-1.png", "frozen-42.png"] {
            assert!(is_frozen_frame_filename(name), "should delete {name}");
        }
        for name in [
            "frozen-.png",
            "frozen-1.jpg",
            "frozen-abc.png",
            "frozen-1.png.bak",
            "screenshot-1.png",
            ".DS_Store",
        ] {
            assert!(!is_frozen_frame_filename(name), "should keep {name}");
        }
    }

    /// A file the cleanup deletes must be a file the capture wrote.
    #[test]
    fn a_written_frozen_frame_matches_the_cleanup_filter() {
        let path = frozen_frame_path(Path::new("/tmp/cache"), 9);
        let name = path.file_name().and_then(|n| n.to_str()).expect("filename");
        assert!(is_frozen_frame_filename(name));
    }
}
