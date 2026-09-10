use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use snapdeck_capture::macos::MacCapturer;

use crate::settings::Settings;

/// One open editor window, keyed by its label.
///
/// Both fields answer a question that only Rust can answer, and neither can be
/// asked of the window itself later: the capture is what the window was opened
/// on, and the id is only readable on the main thread while the window is
/// alive.
struct EditorSession {
    /// The capture the window was opened on, absolute and exactly as this
    /// application wrote it.
    ///
    /// This is the whole of what `save_edited` may write. The path in a save
    /// request is a string the page chooses, and the only legitimate choices
    /// are this file or the same name in another format the editor encodes.
    capture: PathBuf,
    /// The window server's id for the window, so `list_windows` can drop it.
    ///
    /// `None` when the server had not numbered the window yet, which leaves the
    /// editor in the picker exactly as it was before this was recorded.
    window_id: Option<u32>,
}

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
    /// The editor windows that are open, by label.
    ///
    /// More than one, because a capture taken while an editor is open no longer
    /// closes it: the annotations in it are unsaved work that the user cannot
    /// get back. Keyed by label because that is what a command knows about the
    /// window that invoked it.
    editors: Mutex<HashMap<String, EditorSession>>,
    /// The settings currently in force.
    ///
    /// Held here rather than read from disk on demand, because the two hottest
    /// readers cannot afford a file read: the global shortcut handler runs on
    /// the main thread on every press, and the capture path runs while the
    /// user is waiting. It is also what makes a change take effect without a
    /// relaunch, since `commands::save_settings` replaces this the moment the
    /// file is written.
    ///
    /// Starts at the defaults rather than at the stored settings: this is
    /// built before there is an `AppHandle` to find the file with, and
    /// `lib::run`'s setup replaces it with what was on disk before the tray or
    /// any shortcut can read it.
    settings: Mutex<Settings>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            capturer: MacCapturer::new(),
            capture_in_flight: Arc::new(AtomicBool::new(false)),
            overlay_window_ids: Mutex::new(Vec::new()),
            editors: Mutex::new(HashMap::new()),
            settings: Mutex::new(Settings::default()),
        }
    }

    /// The settings in force, cloned so no caller holds the lock across a
    /// capture.
    pub fn settings(&self) -> Settings {
        self.settings_lock().clone()
    }

    /// Puts new settings into force. The next capture and the next shortcut
    /// press read these; nothing is relaunched.
    pub fn set_settings(&self, settings: Settings) {
        *self.settings_lock() = settings;
    }

    /// The settings lock, with poisoning treated as recoverable for the reason
    /// `overlay_ids` gives: every user of it replaces or reads the whole value,
    /// and propagating an unrelated panic would leave the application unable to
    /// answer what its own save folder is.
    fn settings_lock(&self) -> std::sync::MutexGuard<'_, Settings> {
        self.settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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

    /// Records an editor window that has just been built.
    pub fn register_editor(&self, label: String, capture: PathBuf, window_id: Option<u32>) {
        self.editors()
            .insert(label, EditorSession { capture, window_id });
    }

    /// The capture the editor at `label` was opened on, if it is still open.
    ///
    /// `None` for any other window, which is what makes a save from a window
    /// that is not an editor a refusal rather than a write.
    pub fn editor_capture(&self, label: &str) -> Option<PathBuf> {
        self.editors()
            .get(label)
            .map(|session| session.capture.clone())
    }

    /// Drops the editor at `label`, and answers whether its capture is now
    /// closed everywhere.
    ///
    /// `Some(capture)` only when no other editor is still open on the same
    /// file, because the caller uses this to take the file back out of the
    /// asset protocol scope and doing that under a window still showing it
    /// would blank the picture. One locked operation rather than a removal
    /// followed by a question, so a second window cannot appear in between.
    pub fn forget_editor(&self, label: &str) -> Option<PathBuf> {
        let mut editors = self.editors();
        let session = editors.remove(label)?;
        let still_open = editors
            .values()
            .any(|other| other.capture == session.capture);
        (!still_open).then_some(session.capture)
    }

    /// The window-server ids of the open editors, for the window picker to
    /// drop along with the overlays'.
    pub fn editor_window_ids(&self) -> Vec<u32> {
        self.editors()
            .values()
            .filter_map(|session| session.window_id)
            .collect()
    }

    /// The editor lock, with poisoning treated as recoverable for the reason
    /// `overlay_ids` gives: every user of it replaces or reads whole entries,
    /// and propagating an unrelated panic would leave every later save refused
    /// and every editor window in the capture picker.
    fn editors(&self) -> std::sync::MutexGuard<'_, HashMap<String, EditorSession>> {
        self.editors
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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

    /// A capture taken while an editor is open opens a second editor rather
    /// than closing the first, so the registry has to hold both.
    #[test]
    fn two_editors_are_open_at_once_and_each_knows_its_own_capture() {
        let state = AppState::new();
        state.register_editor(
            "editor-0".into(),
            PathBuf::from("/Pictures/a.png"),
            Some(11),
        );
        state.register_editor(
            "editor-1".into(),
            PathBuf::from("/Pictures/b.png"),
            Some(22),
        );

        assert_eq!(
            state.editor_capture("editor-0"),
            Some(PathBuf::from("/Pictures/a.png"))
        );
        assert_eq!(
            state.editor_capture("editor-1"),
            Some(PathBuf::from("/Pictures/b.png"))
        );
        let mut ids = state.editor_window_ids();
        ids.sort_unstable();
        assert_eq!(ids, vec![11, 22]);
    }

    /// A saved change has to be the thing the next capture and the next
    /// shortcut press read, which is what "no relaunch" means here.
    #[test]
    fn the_settings_in_force_are_replaced_by_a_save() {
        let state = AppState::new();
        assert_eq!(
            state.settings(),
            Settings::default(),
            "nothing has been loaded yet, so the built-in defaults are in force"
        );
        let changed = Settings {
            filename_template: "shot-{time}".to_string(),
            ..Settings::default()
        };
        state.set_settings(changed.clone());
        assert_eq!(state.settings(), changed);
    }

    /// A window that is not an editor has no capture, which is what makes a
    /// save from one a refusal instead of a write.
    #[test]
    fn a_window_that_is_not_an_editor_has_no_capture() {
        let state = AppState::new();
        assert_eq!(state.editor_capture("overlay-1"), None);
    }

    /// Closing one editor must not take the other one's picture out of the
    /// asset scope, and must take its own out once nothing shows it.
    #[test]
    fn a_capture_is_released_only_once_no_editor_is_left_on_it() {
        let state = AppState::new();
        let shared = PathBuf::from("/Pictures/a.png");
        state.register_editor("editor-0".into(), shared.clone(), Some(11));
        state.register_editor("editor-1".into(), shared.clone(), Some(22));

        assert_eq!(
            state.forget_editor("editor-0"),
            None,
            "the other editor is still showing this capture"
        );
        assert_eq!(state.forget_editor("editor-1"), Some(shared));
        assert!(state.editor_window_ids().is_empty());
        assert_eq!(state.forget_editor("editor-1"), None);
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
