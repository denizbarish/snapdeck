//! Commands the frontend may invoke.
//!
//! Task 10 adds the capture commands here. Only the dismissal lives here for
//! now, because the overlay needs it to be usable at all: it covers every
//! display and swallows every click, so without a way to say "never mind" the
//! only exit is the reveal deadline, which does not apply to a window that did
//! show its frame.

use tauri::AppHandle;

use crate::overlay;

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
#[tauri::command]
pub fn close_overlays(app: AppHandle) {
    overlay::close_overlays(&app);
    overlay::discard_cached_frozen_frames(&app);
}
