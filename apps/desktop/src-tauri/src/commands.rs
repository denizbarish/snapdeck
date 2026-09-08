//! Commands the frontend may invoke.
//!
//! Task 10 adds the capture commands here. What lives here now is what the
//! overlay cannot work without: the dismissal, because the overlay covers
//! every display and swallows every click, so without a way to say "never
//! mind" the only exit is the reveal deadline, which does not apply to a
//! window that did show its frame; and the window list, because window mode
//! has nothing to highlight until it knows where the windows are.

use serde::Serialize;
use snapdeck_capture::{ScreenCapturer, WindowInfo};
use tauri::{AppHandle, Manager};

use crate::{overlay, state::AppState};

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

#[cfg(test)]
mod tests {
    use super::*;
    use snapdeck_capture::Rect;

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
