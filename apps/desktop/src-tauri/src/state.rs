use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use snapdeck_capture::macos::MacCapturer;

/// Shared application state. The capturer is stateless and cheap to share.
pub struct AppState {
    /// Read by `overlay::open_overlays` and by the capture commands added in
    /// Task 10.
    pub capturer: MacCapturer,
    /// Set while a capture is on its way from the trigger to the overlay
    /// windows. Shared with the guard rather than borrowed, so the worker
    /// thread can hold the claim without borrowing the managed state.
    capture_in_flight: Arc<AtomicBool>,
    /// Window-server ids of the overlays built by the current capture.
    ///
    /// Recorded by `overlay::build_overlay_windows`, which is already on the
    /// main thread that `NSWindow` requires, and read by `list_windows` from
    /// its blocking worker. Storing them beats asking the main thread for them
    /// on demand: window creation blocks that thread once per display, and the
    /// first overlay's page is already mounted and asking by the time the
    /// second one is being built, so a request would queue behind the very
    /// work that produces the answer. On a multi-display setup that queue is
    /// unbounded from the worker's point of view, and window mode would fail
    /// with nothing to highlight and no retry.
    overlay_window_ids: Mutex<Vec<u32>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            capturer: MacCapturer::new(),
            capture_in_flight: Arc::new(AtomicBool::new(false)),
            overlay_window_ids: Mutex::new(Vec::new()),
        }
    }

    /// Records the overlays a capture just built. Replaces the previous set,
    /// which belonged to a capture whose windows are already closed.
    pub fn set_overlay_window_ids(&self, ids: Vec<u32>) {
        *self.overlay_ids() = ids;
    }

    /// The overlays' own window ids, so the window list can drop them.
    pub fn overlay_window_ids(&self) -> Vec<u32> {
        self.overlay_ids().clone()
    }

    /// The lock, with poisoning treated as recoverable.
    ///
    /// Nothing under it can be left half written: both users replace or read
    /// the whole vector. Propagating the panic instead would turn one unrelated
    /// crash into a window mode that never highlights anything again.
    fn overlay_ids(&self) -> std::sync::MutexGuard<'_, Vec<u32>> {
        self.overlay_window_ids
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Claims the single capture slot, or returns `None` when a capture is
    /// already in flight.
    ///
    /// Nothing appears on screen for the length of a capture, so pressing the
    /// shortcut twice is the natural thing to do. Two concurrent captures write
    /// the same `frozen-<id>.png` from two threads, each from offset zero, and
    /// the second one to reach the window builder loses the label race: the
    /// user presses twice and ends up with a corrupt file and no overlay at
    /// all. Dropping the second request is the only outcome that leaves a
    /// working overlay on screen.
    pub fn begin_capture(&self) -> Option<CaptureGuard> {
        self.capture_in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| CaptureGuard {
                flag: Arc::clone(&self.capture_in_flight),
            })
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Holds the capture slot for as long as it lives.
///
/// The slot is released on drop rather than by an explicit call so that an
/// early return, a `?`, or a panic anywhere in the worker cannot wedge the
/// application into refusing every later capture.
pub struct CaptureGuard {
    flag: Arc<AtomicBool>,
}

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_capture_is_refused_while_the_first_is_in_flight() {
        let state = AppState::new();
        let first = state
            .begin_capture()
            .expect("the first capture claims the slot");
        assert!(
            state.begin_capture().is_none(),
            "a rapid double trigger must not start two captures"
        );
        drop(first);
    }

    #[test]
    fn dropping_the_guard_frees_the_slot_for_the_next_capture() {
        let state = AppState::new();
        drop(
            state
                .begin_capture()
                .expect("the first capture claims the slot"),
        );
        assert!(
            state.begin_capture().is_some(),
            "the slot must be reusable once the capture finishes or unwinds"
        );
    }

    #[test]
    fn the_overlay_ids_start_empty_and_are_replaced_by_each_capture() {
        let state = AppState::new();
        assert!(
            state.overlay_window_ids().is_empty(),
            "no capture has built an overlay yet"
        );
        state.set_overlay_window_ids(vec![101, 102]);
        assert_eq!(state.overlay_window_ids(), vec![101, 102]);
        // The previous capture's windows are gone, so its ids must not linger
        // and exclude a window the user could otherwise pick.
        state.set_overlay_window_ids(vec![203]);
        assert_eq!(state.overlay_window_ids(), vec![203]);
    }

    /// The guard exists so that an unwinding worker still frees the slot.
    #[test]
    fn a_panic_while_holding_the_guard_frees_the_slot() {
        let state = AppState::new();
        let guard = state
            .begin_capture()
            .expect("the first capture claims the slot");
        // The panic is the point of the test, so its message and the backtrace
        // note are noise on every `cargo test` run. Silenced only around the
        // call: the hook is process wide, and a panic in another test running
        // in parallel deserves its message.
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let panicked = std::panic::catch_unwind(move || {
            let _guard = guard;
            panic!("the capture worker died");
        });
        std::panic::set_hook(previous_hook);
        assert!(panicked.is_err());
        assert!(
            state.begin_capture().is_some(),
            "a panicked capture must not wedge every later capture"
        );
    }
}
