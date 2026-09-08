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
    report::report_failure,
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

/// How long an overlay may stay hidden after it has been built.
///
/// Generous on purpose. This is the last line of defence, not the fast path:
/// the overlay shows itself the moment its backdrop has painted, so a deadline
/// that expires is always either a broken window nobody can see or a load that
/// was going to succeed and would be killed for no reason. The first costs the
/// user nothing extra by lingering another second; the second is a capture
/// thrown away. The budget covers page load, a multi-megabyte asset read and
/// the decode, on every display at once.
const REVEAL_DEADLINE: Duration = Duration::from_millis(2000);

/// Whether the screen recording pane has already been opened since the last
/// time the preflight was happy.
///
/// Impatient repeat triggers would otherwise steal focus once per press. Reset
/// by `ensure_permission` as soon as the permission reads `Granted`, so a user
/// who fixes the permission and later loses it again is shown the pane a second
/// time rather than being locked out of it for the life of the process.
static SETTINGS_PANE_OPENED: AtomicBool = AtomicBool::new(false);

/// What the user is told when the screen cannot be captured.
///
/// Every wording here is the same one because the cause almost always is: macOS
/// decides what a process may capture when the process starts, so the common
/// first run is a permission granted without relaunching, a preflight that now
/// answers `Granted` and a ScreenCaptureKit call that keeps failing anyway.
const PERMISSION_MESSAGE: &str = "Snapdeck could not capture the screen. Open System Settings > Privacy & Security > Screen Recording, enable Snapdeck, then quit and reopen the app.";

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
    discard_cached_frozen_frames(app);

    let app = app.clone();
    let mode = mode.to_string();
    std::thread::spawn(move || {
        // A panic here would otherwise be completely silent: nothing joins this
        // thread, and the user would see the same nothing as a refused
        // permission.
        //
        // The guard stays in this frame rather than travelling into
        // `run_capture`. An unwind drops everything the panicking frame owns
        // before `catch_unwind` returns, so a guard held down there would free
        // the capture slot while this recovery is still queuing its cleanup: a
        // new capture could claim the slot, write its `frozen-<id>.png`, and
        // then have the recovery's `close_overlays` and frame discard, which
        // the main thread runs first, delete the frames out from under it.
        // `run_capture` takes the guard out only when it hands the finished
        // windows to the main thread.
        let mut guard = Some(guard);
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
            run_capture(&app, &mode, &mut guard);
        }));
        if let Err(payload) = outcome {
            report_failure(
                &app,
                &format!(
                    "Snapdeck could not capture the screen: the capture stopped unexpectedly ({}). Try the shortcut again.",
                    panic_text(payload.as_ref())
                ),
            );
            let handle = app.clone();
            // Any overlay that did get built is unusable now: it is showing
            // a backdrop for a capture that died. Whatever is left of the guard rides
            // into the closure and drops only after the cleanup has run, so the
            // slot stays claimed for the whole recovery.
            let _ = app.run_on_main_thread(move || {
                close_overlays(&handle);
                discard_cached_frozen_frames(&handle);
                drop(guard);
            });
        }
    });
}

/// The worker thread's whole job: check permission, wait for the old overlays
/// to leave, capture, then hand the windows back to the main thread.
///
/// The guard is borrowed rather than owned, and moved out only on the success
/// path, into the main-thread closure: the slot has to stay claimed until the
/// new windows actually exist, because `run_on_main_thread` only queues the
/// closure, and it has to stay claimed through an unwind, which is why the
/// caller keeps the `Option` in a frame that does not unwind with this one.
fn run_capture(app: &AppHandle, mode: &str, guard: &mut Option<CaptureGuard>) {
    if !ensure_permission(app) {
        return;
    }
    if !wait_for_overlays_to_close(app) {
        report_failure(
            app,
            &format!(
                "Snapdeck could not capture the screen: the previous overlay did not close within {CLOSE_DRAIN_TIMEOUT:?}. Try the shortcut again."
            ),
        );
        return;
    }

    let frozen = match capture_frozen_frames(app) {
        Ok(frozen) => frozen,
        Err(err) => {
            // The likeliest cause by far, and the only one the user can act on,
            // is a permission this process cannot see yet, which is why the
            // instructions come first and the platform's own words second.
            report_failure(app, &format!("{PERMISSION_MESSAGE} (Details: {err})"));
            return;
        }
    };

    let handle = app.clone();
    let mode = mode.to_string();
    let guard = guard.take();
    if let Err(err) = app.run_on_main_thread(move || {
        let windows = build_overlay_windows(&handle, &mode, &frozen);
        drop(guard);
        schedule_reveal_deadline(&handle, windows);
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

/// Closes every overlay window. Also wired to Escape and to a finished
/// selection in Task 7.
///
/// Deliberately does not touch the frozen frames. Escape can arrive while a
/// capture is in flight, and unlinking the file a worker is halfway through
/// writing would leave the next set of windows opening onto nothing. Whoever
/// knows that no capture is running calls `discard_cached_frozen_frames` as
/// well.
pub fn close_overlays(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(OVERLAY_LABEL_PREFIX) {
            let _ = window.close();
        }
    }
}

/// Discards the frozen frames sitting in the application cache directory.
pub fn discard_cached_frozen_frames(app: &AppHandle) {
    let Ok(cache_dir) = app.path().app_cache_dir() else {
        return;
    };
    discard_frozen_frames(&cache_dir);
}

/// Removes the frozen frames from `dir`.
///
/// Each one is a full-resolution, lossless copy of everything that was on the
/// user's screen, so leaving them in `~/Library/Caches` to accumulate is a
/// privacy cost with no upside: they are throwaway backdrops that the next
/// capture rewrites anyway. Safe to call while the overlays are still on
/// screen, because the webview has already decoded the file into memory by the
/// time it is visible.
///
/// Takes the directory rather than an `AppHandle` so that the one thing worth
/// testing here, which files it is willing to unlink, can be tested against a
/// temporary directory.
fn discard_frozen_frames(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
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
/// The app is `LSUIElement` with no windows declared, so it has neither a
/// console the user reads nor a window to raise: a refusal has to reach them
/// through `report_failure`. The first call prompts; afterwards macOS stays
/// silent, which is why a still-denied permission opens the settings pane as
/// well.
///
/// A granted permission is a necessary condition, never a sufficient one: a
/// running process cannot see a grant made after it started, so every
/// ScreenCaptureKit call can still fail until the app is relaunched. Capture
/// errors are therefore reported by their caller rather than reinterpreted here.
fn ensure_permission(app: &AppHandle) -> bool {
    if screen_capture_permission().is_granted() {
        // The pane has done its job. Arming it again costs nothing while the
        // permission holds, and it is the only thing that keeps a permission
        // revoked later in the session from being a silent dead end.
        SETTINGS_PANE_OPENED.store(false, Ordering::Relaxed);
        return true;
    }
    if request_screen_capture_permission().is_granted() {
        SETTINGS_PANE_OPENED.store(false, Ordering::Relaxed);
        return true;
    }
    open_screen_recording_settings();
    // Unconditionally, unlike the pane: the pane steals focus, a notification
    // does not, so the answer to an impatient second press is still an answer
    // rather than nothing at all.
    report_failure(app, PERMISSION_MESSAGE);
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
///
/// `commands::capture_region` waits on it too, and for the first of those two
/// reasons rather than the second: it re-captures the selected region at native
/// resolution, so an overlay that is merely closing rather than closed lands in
/// the user's file as a picture of the dimmed selection UI.
pub(crate) fn wait_for_overlays_to_close(app: &AppHandle) -> bool {
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
///
/// Returns the windows it built, so the caller can hold them to the reveal
/// deadline. An empty vector means there is nothing left on screen.
fn build_overlay_windows(
    app: &AppHandle,
    mode: &str,
    frozen: &[(DisplayInfo, PathBuf)],
) -> Vec<WebviewWindow> {
    let mut windows = Vec::with_capacity(frozen.len());
    // Read here rather than on demand: this loop is the one place that is both
    // on the main thread `NSWindow` requires and finished with a window the
    // moment it is built. See `AppState::overlay_window_ids`.
    let mut ids = Vec::with_capacity(frozen.len());
    let mut failed = false;
    for (display, path) in frozen {
        match build_overlay_window(app, mode, display, path) {
            Ok(window) => {
                raise_above_menu_bar(&window);
                if let Some(id) = overlay_window_id(&window) {
                    ids.push(id);
                }
                windows.push(window);
            }
            Err(err) => {
                report_failure(
                    app,
                    &format!(
                        "Snapdeck could not cover display {} with the selection overlay, so the capture was cancelled. Try the shortcut again. (Details: {err})",
                        display.id
                    ),
                );
                failed = true;
            }
        }
    }
    // A half-open set of overlays is worse than none: the displays that did open
    // are frozen against displays that are still live, and the user cannot tell
    // which is which.
    // The frames go too: the capture is over, this thread still holds the
    // capture slot, so nothing can be writing them.
    if failed {
        close_overlays(app);
        discard_cached_frozen_frames(app);
        app.state::<AppState>().set_overlay_window_ids(Vec::new());
        return Vec::new();
    }
    app.state::<AppState>().set_overlay_window_ids(ids);
    windows
}

/// Closes any overlay that is still hidden `REVEAL_DEADLINE` after it was
/// built.
///
/// The deadline lives in Rust rather than in the overlay page because the page
/// may never get to run one. The window is built `.visible(false)`, so it is
/// never composited and WebKit puts its DOM timers on the throttled schedule; a
/// timer set for 500 ms is not a promise of 500 ms. Worse, the page can fail
/// before any timer is armed at all: a missing query parameter, a bundle that
/// does not load, a rejected CSP. In each of those the window stays hidden with
/// no `error` event to notice it by, holding its label against the next
/// capture. The window is Rust's, so the promise that it either shows itself or
/// goes away is Rust's too.
///
/// The windows travel into the closure as handles, not labels. The capture slot
/// is released as soon as they exist, so by the time this fires another capture
/// may already own the same `overlay-<id>` labels, and a lookup by label would
/// close that capture's brand new, legitimately still-hidden windows.
fn schedule_reveal_deadline(app: &AppHandle, windows: Vec<WebviewWindow>) {
    if windows.is_empty() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(REVEAL_DEADLINE);
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || close_hidden_overlays(&handle, &windows));
    });
}

/// Main thread only. A window that is already gone reports an error rather than
/// a visibility, which is the outcome this is trying to reach anyway.
///
/// Takes the frozen frames with it when nothing from this batch is left on
/// screen. This is the one leak the project has actually watched happen:
/// closing the windows and stopping there leaves a full-resolution, lossless
/// copy of every display in `~/Library/Caches` with no overlay above it and
/// nobody coming back for it. Guarded exactly the way `commands::close_overlays`
/// is, because the capture slot was released when these windows were built and
/// a newer capture may already be writing the frames this would delete.
fn close_hidden_overlays(app: &AppHandle, windows: &[WebviewWindow]) {
    let mut all_gone = true;
    for window in windows {
        match window.is_visible() {
            Ok(false) => {
                report_failure(
                    app,
                    &format!(
                        "Snapdeck closed the overlay on {} because it never showed the frozen screen within {REVEAL_DEADLINE:?}. Try the shortcut again.",
                        window.label()
                    ),
                );
                let _ = window.close();
            }
            // Showing its frozen frame is the whole contract, and this one met
            // it: the user is looking at it, and the file behind it is still
            // the magnifier's source.
            Ok(true) => all_gone = false,
            // Already destroyed, by Escape, by a capture, or by its own error
            // handler.
            Err(_) => {}
        }
    }
    if all_gone {
        if let Some(_guard) = app.state::<AppState>().begin_capture() {
            discard_cached_frozen_frames(app);
        }
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
    // `NSWindow` is a main-thread-only class in objc2. Taking a reference from
    // a raw pointer bypasses the marker that would normally carry that proof
    // through the type system, and asking for the marker here does not bring
    // the proof back: the cast below stays unchecked. What it does is assert at
    // runtime that this really is the main thread, which is the assumption the
    // cast rests on. Reaching the `else` means a caller broke the contract that
    // this only runs inside `run_on_main_thread`, and returning is the only
    // sane answer: this closure sits outside the worker's `catch_unwind`, so a
    // panic here would unwind the AppKit event loop and take the app with it.
    let Some(_mtm) = MainThreadMarker::new() else {
        eprintln!(
            "snapdeck: overlay setup for {} ran off the main thread, leaving the window at its default level",
            window.label()
        );
        return;
    };
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

/// Window-server id of one overlay window.
///
/// Window mode hovers whatever sits under the pointer, and the overlays are on
/// top of everything and cover their whole display, so ScreenCaptureKit lists
/// them like any other window. Left in, every hover would land on an overlay
/// and every pick would be the overlay itself.
///
/// Identified by id rather than by level. The frontend also drops everything
/// off the normal layer, which happens to exclude these too because
/// `raise_above_menu_bar` puts them at 25, but that is a statement about where
/// the window is stacked, not about whose it is: lower the level and window
/// mode would silently start picking the overlay. `NSWindow`'s `windowNumber`
/// is the window server's own id for the window, which is exactly what
/// `SCWindow.windowID` reports, so the two lists name the same windows.
///
/// Main thread only, for the same reason as `raise_above_menu_bar`. Returns
/// nothing rather than panicking when called from anywhere else, because there
/// is no answer to give and this runs inside an AppKit callback.
#[cfg(target_os = "macos")]
fn overlay_window_id(window: &WebviewWindow) -> Option<u32> {
    let Some(_mtm) = MainThreadMarker::new() else {
        eprintln!(
            "snapdeck: the window id for {} was requested off the main thread",
            window.label()
        );
        return None;
    };
    let handle = window.ns_window().ok()?;
    // SAFETY: `ns_window` hands back this window's live `NSWindow`, which
    // outlives the borrow, and `_mtm` proves this is the thread AppKit requires
    // for every `NSWindow` call.
    let ns_window: &NSWindow = unsafe { &*handle.cast::<NSWindow>() };
    // A window that the server has not assigned yet carries a number of 0 or a
    // negative one, neither of which is an id that could match anything
    // ScreenCaptureKit reports.
    u32::try_from(ns_window.windowNumber())
        .ok()
        .filter(|id| *id != 0)
}

#[cfg(not(target_os = "macos"))]
fn overlay_window_id(_window: &WebviewWindow) -> Option<u32> {
    None
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

    /// The filter decides what is deleted, but only the walk actually deletes,
    /// so the walk is exercised against a real directory.
    #[test]
    fn discard_frozen_frames_unlinks_exactly_the_frozen_frames() {
        let dir = temp_dir("discard");
        std::fs::create_dir_all(&dir).expect("create the temporary cache");
        for name in ["frozen-1.png", "frozen-abc.png", "user-notes.txt"] {
            std::fs::write(dir.join(name), b"x").expect("write the fixture");
        }

        discard_frozen_frames(&dir);

        let mut survivors: Vec<String> = std::fs::read_dir(&dir)
            .expect("read the temporary cache")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        survivors.sort();
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(survivors, vec!["frozen-abc.png", "user-notes.txt"]);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock is after the epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("snapdeck-{}-{unique}-{name}", std::process::id()))
    }

    /// A file the cleanup deletes must be a file the capture wrote.
    #[test]
    fn a_written_frozen_frame_matches_the_cleanup_filter() {
        let path = frozen_frame_path(Path::new("/tmp/cache"), 9);
        let name = path.file_name().and_then(|n| n.to_str()).expect("filename");
        assert!(is_frozen_frame_filename(name));
    }
}
