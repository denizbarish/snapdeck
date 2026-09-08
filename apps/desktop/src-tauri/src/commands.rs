//! Commands the frontend may invoke.
//!
//! Task 10 adds the capture commands here. What lives here now is what the
//! overlay cannot work without: the dismissal, because the overlay covers
//! every display and swallows every click, so without a way to say "never
//! mind" the only exit is the reveal deadline, which does not apply to a
//! window that did show its frame; and the window list, because window mode
//! has nothing to highlight until it knows where the windows are.

use std::sync::mpsc;
use std::time::Duration;

use serde::Serialize;
use snapdeck_capture::{ScreenCapturer, WindowInfo};
use tauri::{AppHandle, Manager};

use crate::{overlay, state::AppState};

/// How long the window list waits for the main thread to say which windows are
/// the overlays' own.
///
/// A bound rather than a plain `recv` because this runs on a blocking worker
/// while the main thread is running the AppKit event loop: if that loop is
/// wedged, the honest answer is an error the overlay can log, not a worker
/// parked forever. Generous, because the closure it waits on does nothing but
/// read a handful of window numbers.
const OVERLAY_ID_TIMEOUT: Duration = Duration::from_millis(500);

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
    /// On-screen windows in global points, with this application's own
    /// overlays removed.
    ///
    /// The order is whatever `SCShareableContent` produced, and that is
    /// measurably **not** z-order: on this machine it returned two overlapping
    /// Terminal windows in the opposite order to
    /// `CGWindowListCopyWindowInfo`, which is the API that does report
    /// front-to-back. The frontend takes the first window containing the
    /// pointer, so until this list is sorted a pick among overlapping windows
    /// can land on the one behind. Sorting it means reading the z-order from
    /// `CGWindowListCopyWindowInfo` and is deliberately not done here.
    pub windows: Vec<WindowInfo>,
    /// Top-left corner of the display this overlay covers, in global points.
    pub origin: Point,
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
    let ours = overlay_window_ids(app)?;
    let state = app.state::<AppState>();

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

    let windows = state
        .capturer
        .windows()
        .map_err(|err| err.to_string())?
        .into_iter()
        .filter(|window| !ours.contains(&window.id))
        .collect();

    Ok(WindowListResult {
        windows,
        origin: Point {
            x: display.bounds.x,
            y: display.bounds.y,
        },
    })
}

/// Asks the main thread which windows are the overlays' own.
///
/// The hop is unavoidable: the ids come from `NSWindow`, which is
/// main-thread-only, while the enumeration around it has to stay off the main
/// thread. Blocking here is safe because this is already a blocking worker.
fn overlay_window_ids(app: &AppHandle) -> Result<Vec<u32>, String> {
    let (sender, receiver) = mpsc::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        // The receiver is gone only if this worker timed out first, in which
        // case there is nobody left to tell.
        let _ = sender.send(overlay::overlay_window_ids(&handle));
    })
    .map_err(|err| format!("could not reach the main thread: {err}"))?;
    receiver
        .recv_timeout(OVERLAY_ID_TIMEOUT)
        .map_err(|err| format!("the main thread did not report the overlay windows: {err}"))
}
