//! Commands the frontend may invoke.
//!
//! Two of them keep the overlay alive: the dismissal, because the overlay
//! covers every display and swallows every click, so without a way to say
//! "never mind" the only exit is the reveal deadline, which does not apply to
//! a window that did show its frame; and the window list, because window mode
//! has nothing to highlight until it knows where the windows are. The rest end
//! the capture: `capture_region` turns a confirmed selection into a file and a
//! clipboard image, and the two permission commands report what the preflight
//! knows for a settings surface to show.

use serde::Serialize;
use snapdeck_capture::{
    macos::permission::{
        request_screen_capture_permission, screen_capture_permission, PermissionState,
        SETTINGS_DEEP_LINK,
    },
    CaptureTarget, Rect, ScreenCapturer, WindowInfo,
};
use tauri::{image::Image, AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::{
    output::{render_filename, save_png, OffsetDateTimeParts, PngCompression},
    overlay,
    state::AppState,
};

/// The name every capture is saved under, before the `.png` extension.
///
/// `{date}` and `{time}` are the tokens `render_filename` expands; `{width}`
/// and `{height}` exist too and are simply not in the default. Written once
/// here so the settings surface that will let the user change it in Plan 4 has
/// one place to override rather than a literal buried in the capture path.
const DEFAULT_FILENAME_TEMPLATE: &str = "Snapdeck {date} at {time}";

/// A point in the global point space shared by every display.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// Everything window mode needs to highlight the window under the pointer.
///
/// The origin travels with the list because the two are in different spaces:
/// window bounds are in the global point space that spans every display, while
/// the overlay's pointer events are display-local. One of them has to be
/// rebased onto the other, and only Rust knows where this display sits.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowListResult {
    /// On-screen windows in global points, front to back, with this
    /// application's own overlays removed.
    ///
    /// The order is the contract the frontend relies on: it takes the first
    /// window containing the pointer, so a list in any other order picks the
    /// window behind and draws an outline that disagrees with the frozen frame
    /// under it. `SCShareableContent` promises no order and measurably does
    /// not provide one, so `ScreenCapturer::windows` sorts by the window
    /// server's own stacking list before this ever sees it.
    pub windows: Vec<WindowInfo>,
    /// Top-left corner of the display this overlay covers, in global points.
    pub origin: Point,
}

/// What the screen recording preflight knows, plus where to go about it.
///
/// The deep link travels with the state so that the surface showing it never
/// has to spell out a `x-apple.systempreferences:` URL of its own; the one
/// this application uses is defined once, in the capture crate.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReport {
    pub state: PermissionState,
    pub settings_url: &'static str,
}

/// Where a finished capture ended up, and how big it is in pixels.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureResult {
    pub path: String,
    /// Pixels, not points: the region is captured at the display's own
    /// density, so on Retina this is twice the number of points selected.
    pub width: u32,
    pub height: u32,
}

/// The current screen recording permission, without prompting.
///
/// `Granted` is a necessary condition and never a sufficient one. macOS
/// decides what a process may capture when the process starts, so a grant made
/// while Snapdeck is running is invisible to it: the preflight flips to
/// `Granted`, and every ScreenCaptureKit call keeps failing until the app is
/// relaunched. That is why macOS itself offers "Quit & Reopen" on the prompt.
/// Anything rendering this must therefore say "granted, relaunch Snapdeck"
/// rather than "ready", because the second is a claim this cannot support.
#[tauri::command]
pub fn permission_state() -> PermissionReport {
    PermissionReport {
        state: screen_capture_permission(),
        settings_url: SETTINGS_DEEP_LINK,
    }
}

/// Asks macOS for the screen recording permission.
///
/// Only the first call in the life of the system prompts; afterwards macOS
/// stays silent and returns the stored answer, which is why the report carries
/// `settings_url`: a `Denied` here means the user has to be sent to the
/// settings pane by hand. The relaunch caveat on `permission_state` applies
/// verbatim to a `Granted` returned here.
#[tauri::command]
pub fn request_permission() -> PermissionReport {
    PermissionReport {
        state: request_screen_capture_permission(),
        settings_url: SETTINGS_DEEP_LINK,
    }
}

/// Closes every overlay and drops the frozen frames behind them.
///
/// All of them, not just the one that asked. Every overlay is built
/// `.focused(true)`, so on a multi-display setup only the last one built holds
/// the keyboard, and an Escape that closed the focused window alone would
/// leave the other displays covered with nothing left to press Escape in.
///
/// The frames go too. `overlay::close_overlays` deliberately leaves them,
/// because it also runs from inside a capture that is still writing them; here
/// there is no such doubt. A dismissal means the user has finished with this
/// capture, and the frames are full-resolution, lossless copies of everything
/// that was on their screen, so keeping them in `~/Library/Caches` until the
/// next capture happens to overwrite them is a privacy cost with no upside.
///
/// Synchronous on purpose, which is the opposite of `list_windows` next door:
/// `WebviewWindow::close` has to run on the main thread on macOS, and a
/// synchronous Tauri command is the one kind that already does.
#[tauri::command]
pub fn close_overlays(app: AppHandle) {
    overlay::close_overlays(&app);
    overlay::discard_cached_frozen_frames(&app);
}

/// The windows window mode may highlight, plus the origin of the display the
/// asking overlay covers.
///
/// `async` is load-bearing. Tauri runs a synchronous command on the main
/// thread, and `ScreenCapturer` documents every one of its calls as blocking
/// for a full platform round trip; Task 6 measured 108 to 230 ms for a capture
/// on this machine. Running that on the main thread would freeze the AppKit
/// event loop, and with it the very overlay that asked. `spawn_blocking`
/// rather than a plain `async` body because the work inside really is blocking
/// and would otherwise sit on an async worker that has other futures to poll.
#[tauri::command]
pub async fn list_windows(app: AppHandle, display_id: u32) -> Result<WindowListResult, String> {
    tauri::async_runtime::spawn_blocking(move || collect_windows(&app, display_id))
        .await
        .map_err(|err| format!("the window list task did not finish: {err}"))?
}

/// Blocking worker only; see `list_windows`.
fn collect_windows(app: &AppHandle, display_id: u32) -> Result<WindowListResult, String> {
    let state = app.state::<AppState>();
    // Recorded when the overlays were built, on the main thread that
    // `NSWindow` requires. Reading it here is a lock and a clone, with no hop
    // to a main thread that is busy creating the very windows being asked
    // about; see `AppState::overlay_window_ids`.
    let ours = state.overlay_window_ids();

    let display = state
        .capturer
        .displays()
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|display| display.id == display_id)
        // Reachable: a display can be unplugged between the capture that built
        // this overlay and the overlay asking about it. Refusing beats
        // guessing an origin, because a wrong origin highlights the wrong
        // rectangle rather than failing visibly.
        .ok_or_else(|| format!("no display with id {display_id}"))?;

    let windows = state.capturer.windows().map_err(|err| err.to_string())?;
    Ok(WindowListResult {
        windows: without_windows(windows, &ours),
        origin: Point {
            x: display.bounds.x,
            y: display.bounds.y,
        },
    })
}

/// Drops the windows whose ids are in `excluded`, keeping the rest in order.
///
/// Split out from `collect_windows` because it is the whole of that function
/// that can be tested: everything around it needs an `AppHandle`, a live
/// display and a screen recording grant. Order is preserved because the list
/// is front to back and the frontend takes the first match.
fn without_windows(windows: Vec<WindowInfo>, excluded: &[u32]) -> Vec<WindowInfo> {
    windows
        .into_iter()
        .filter(|window| !excluded.contains(&window.id))
        .collect()
}

/// Turns a confirmed selection into a PNG in the pictures directory and an
/// image on the clipboard.
///
/// `rect` is in the display-local points the overlay works in, because only
/// Rust knows where that display sits in the global space.
///
/// `async` plus `spawn_blocking` for the same reason as `list_windows`: a
/// synchronous command runs on the main thread, and everything below blocks
/// for a platform round trip.
#[tauri::command]
pub async fn capture_region(
    app: AppHandle,
    display_id: u32,
    rect: Rect,
) -> Result<CaptureResult, String> {
    let result =
        tauri::async_runtime::spawn_blocking(move || capture_selection(&app, display_id, rect))
            .await
            .map_err(|err| format!("the capture task did not finish: {err}"))?;
    // The overlay that asked is deliberately closed before the capture runs, so
    // an `Err` returned from here has no webview left to reach and no console
    // to land in. This is the only trace it can leave, and it is what the rest
    // of the capture path already does with a failure nobody is waiting on.
    if let Err(err) = &result {
        eprintln!("snapdeck: the region capture failed: {err}");
    }
    result
}

/// Blocking worker only; see `capture_region`.
fn capture_selection(
    app: &AppHandle,
    display_id: u32,
    rect: Rect,
) -> Result<CaptureResult, String> {
    let state = app.state::<AppState>();
    // The same one-at-a-time slot the trigger claims. Held for the whole of
    // this, not because two captures would race for the file, but because the
    // cleanup at the end unlinks `frozen-<id>.png`: without the claim, a
    // shortcut pressed during these few hundred milliseconds would start a
    // capture, write fresh frozen frames, and have them deleted underneath its
    // half-built overlays. `None` means such a capture is already under way,
    // and it has closed these overlays and will draw its own.
    let _guard = state
        .begin_capture()
        .ok_or_else(|| "another capture started before this selection was confirmed".to_string())?;

    dismiss_overlays_and_wait(app)?;

    let display = state
        .capturer
        .displays()
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|display| display.id == display_id)
        .ok_or_else(|| format!("no display with id {display_id}"))?;

    // The overlay reports display-local points; capture expects global points.
    let global = Rect {
        x: display.bounds.x + rect.x,
        y: display.bounds.y + rect.y,
        width: rect.width,
        height: rect.height,
    };
    let frame = state
        .capturer
        .capture(CaptureTarget::Region(global))
        .map_err(|err| err.to_string())?;

    let directory = app
        .path()
        .picture_dir()
        .map_err(|err| format!("no pictures directory: {err}"))?;
    std::fs::create_dir_all(&directory)
        .map_err(|err| format!("failed to create {}: {err}", directory.display()))?;
    let name = render_filename(
        DEFAULT_FILENAME_TEMPLATE,
        OffsetDateTimeParts::now(),
        frame.width,
        frame.height,
    );
    let path = directory.join(format!("{name}.png"));
    // `Default`, not the `Fast` the backdrop uses. The backdrop is a throwaway
    // the user is blocked on; this is a file they keep, where Task 6 measured
    // `Fast` + `NoFilter` at 29.9 MB against 2.3 MB here, and nobody is
    // waiting on the write.
    save_png(&frame, &path, PngCompression::Default)?;

    let rgba = frame.to_rgba8().map_err(|err| err.to_string())?;
    app.clipboard()
        .write_image(&Image::new(&rgba, frame.width, frame.height))
        .map_err(|err| format!("failed to copy the capture to the clipboard: {err}"))?;

    // Only now: the frames are the backdrop of the overlay this capture just
    // closed, and they are full-resolution copies of everything that was on
    // screen. Leaving them in `~/Library/Caches` until the next capture happens
    // to overwrite them is the same privacy cost the dismissal exists to avoid.
    overlay::discard_cached_frozen_frames(app);

    Ok(CaptureResult {
        path: path.to_string_lossy().into_owned(),
        width: frame.width,
        height: frame.height,
    })
}

/// Takes the overlays off the screen and waits until they are actually gone.
///
/// Both halves matter. The region is captured again at native resolution
/// rather than cropped out of the frozen frame, so an overlay still on screen
/// is not a cosmetic problem: it is what the user's file would contain, a
/// picture of the dimmed selection UI instead of their content. And closing is
/// only a request. `WebviewWindow::close` posts to the event loop, the label
/// leaves the window map when `Destroyed` arrives, and the pixels leave with
/// it, so capturing on the next line would photograph a window that is still
/// up. A timeout abandons the capture rather than saving that picture.
fn dismiss_overlays_and_wait(app: &AppHandle) -> Result<(), String> {
    let handle = app.clone();
    // On the main thread, which is where macOS requires window teardown and
    // where `commands::close_overlays` already gets to be by being synchronous.
    // This one is not: it runs on a blocking worker, so it has to ask.
    app.run_on_main_thread(move || overlay::close_overlays(&handle))
        .map_err(|err| format!("could not reach the main thread to close the overlays: {err}"))?;
    if overlay::wait_for_overlays_to_close(app) {
        return Ok(());
    }
    Err("the overlays did not leave the screen in time, so nothing was captured".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: u32) -> WindowInfo {
        WindowInfo {
            id,
            title: None,
            app_name: None,
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            layer: 0,
            is_on_screen: true,
        }
    }

    fn ids(windows: &[WindowInfo]) -> Vec<u32> {
        windows.iter().map(|w| w.id).collect()
    }

    #[test]
    fn the_overlays_own_windows_are_dropped() {
        let windows = vec![window(11), window(22), window(33)];
        assert_eq!(ids(&without_windows(windows, &[22])), vec![11, 33]);
    }

    #[test]
    fn one_overlay_per_display_is_dropped() {
        let windows = vec![window(11), window(22), window(33), window(44)];
        assert_eq!(ids(&without_windows(windows, &[22, 44])), vec![11, 33]);
    }

    #[test]
    fn nothing_is_dropped_when_no_overlay_was_recorded() {
        let windows = vec![window(11), window(22)];
        assert_eq!(ids(&without_windows(windows, &[])), vec![11, 22]);
    }

    /// The list is front to back and the frontend takes the first match, so
    /// removing a window may not reshuffle the ones around it.
    #[test]
    fn the_surviving_windows_keep_their_front_to_back_order() {
        let windows = vec![window(5), window(9), window(1), window(7)];
        assert_eq!(ids(&without_windows(windows, &[9])), vec![5, 1, 7]);
    }
}
