use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use snapdeck_capture::macos::MacCapturer;

use crate::settings::Settings;
use crate::shortcuts::Shortcuts;

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
    /// Every capture this process has put into the webviews' asset scope.
    ///
    /// The scope only ever grows, for the reason `editor::release_editor`
    /// gives: Tauri's scope cannot undo a grant without forbidding the pattern,
    /// and a forbidden pattern is permanent and beats every later grant. So a
    /// capture that has been shown once stays readable, and this is the record
    /// of which ones those are.
    ///
    /// It is what keeps a reused path working. The filename template is the
    /// user's, so a template without `{time}` in it renders the same name every
    /// time; delete the file after pasting it and the next capture takes that
    /// name back. Asking here first means the second editor on that path finds
    /// the grant already in place instead of adding a second copy of it.
    granted_captures: Mutex<HashSet<PathBuf>>,
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
    /// The bindings the platform has actually accepted, or `None` when nothing
    /// is bound at all.
    ///
    /// A separate question from `settings.shortcuts`, and the reason this field
    /// exists: the settings are what the file says and what the next save
    /// writes back, while this is what the keyboard does. They disagree
    /// whenever a stored combination cannot be registered, which is a thing
    /// another application can cause between one launch and the next.
    ///
    /// Two things read it. The global shortcut handler matches a press against
    /// it, because a press can only ever come from a binding that is registered,
    /// and matching against the settings instead would leave a fallback
    /// registration firing a handler that recognises nothing. And
    /// `commands::get_settings` reports it to the settings window, so a binding
    /// that is not in force is shown as not in force rather than as a working
    /// key.
    ///
    /// Starts at `None` for the same reason `settings` starts at the defaults:
    /// nothing has been registered yet at the moment this is built.
    registered_shortcuts: Mutex<Option<Shortcuts>>,
    /// The window server's id for the settings window while it is open.
    ///
    /// Recorded for the reason the editors' ids are: the settings window is an
    /// ordinary window in the window list, so window mode would otherwise offer
    /// the user a picture of the window they are choosing settings in, and offer
    /// it first, because it is the frontmost window on screen while it is open.
    settings_window_id: Mutex<Option<u32>>,
    /// The captures the tray's Recent Captures submenu offers, newest first.
    ///
    /// Paths and nothing else; see the `recents` module for why that is the
    /// whole of what this feature is allowed to remember.
    ///
    /// Held here rather than read from disk when the menu is clicked, because a
    /// menu event runs on the main thread and a click has to resolve to a file
    /// without touching the filesystem first. `recents` owns what goes in it.
    ///
    /// Starts empty for the reason `settings` starts at the defaults: there is
    /// no `AppHandle` to find the file with at the moment this is built, and
    /// `lib::run`'s setup replaces it with what was on disk before the tray can
    /// be clicked.
    recent_captures: Mutex<Vec<PathBuf>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            capturer: MacCapturer::new(),
            capture_in_flight: Arc::new(AtomicBool::new(false)),
            overlay_window_ids: Mutex::new(Vec::new()),
            editors: Mutex::new(HashMap::new()),
            granted_captures: Mutex::new(HashSet::new()),
            settings: Mutex::new(Settings::default()),
            registered_shortcuts: Mutex::new(None),
            settings_window_id: Mutex::new(None),
            recent_captures: Mutex::new(Vec::new()),
        }
    }

    /// Reads and replaces the list in one hold of the lock, and answers with
    /// what is now in force.
    ///
    /// One method rather than a getter and a setter, because a read, a decision
    /// and a write done as three steps is two bugs. The lost update is the
    /// obvious one. The worse one is that the caller then renders the list it
    /// computed rather than the list that won: two captures finishing together
    /// leave the state holding one and the menu showing the other, and nothing
    /// corrects it until the next capture. Both are reachable, because a capture
    /// and an editor save are on different threads and only captures are
    /// serialised by `begin_capture`; two editors can be open at once by design.
    ///
    /// The answer is the list to render, and it is deliberately returned rather
    /// than rendered here: rebuilding the menu asks the main thread to do it and
    /// waits, while a click on that menu runs on the main thread, so a caller
    /// still holding this lock while it rebuilt would be the two halves of a
    /// deadlock.
    pub fn update_recent_captures<F>(&self, update: F) -> Vec<PathBuf>
    where
        F: FnOnce(&[PathBuf]) -> Vec<PathBuf>,
    {
        let mut captures = self.recent_captures_lock();
        *captures = update(captures.as_slice());
        captures.clone()
    }

    /// The recent captures lock, with poisoning treated as recoverable for the
    /// reason `overlay_ids` gives: every user replaces or reads the whole list,
    /// and propagating an unrelated panic would leave the menu unable to say
    /// where the last capture went.
    fn recent_captures_lock(&self) -> std::sync::MutexGuard<'_, Vec<PathBuf>> {
        self.recent_captures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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

    /// The bindings actually registered, or `None` when nothing is bound.
    pub fn registered_shortcuts(&self) -> Option<Shortcuts> {
        self.registered_lock().clone()
    }

    /// Records what is bound after a registration attempt.
    ///
    /// `None` is a real answer and not an absence: it says the keyboard is
    /// empty, which is what the settings window has to be able to show.
    pub fn set_registered_shortcuts(&self, shortcuts: Option<Shortcuts>) {
        *self.registered_lock() = shortcuts;
    }

    /// The registered-bindings lock, with poisoning treated as recoverable for
    /// the reason `overlay_ids` gives: every user replaces or reads the whole
    /// value, and propagating an unrelated panic would leave every capture
    /// shortcut dead and the settings window unable to say so.
    fn registered_lock(&self) -> std::sync::MutexGuard<'_, Option<Shortcuts>> {
        self.registered_shortcuts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Records the settings window's id, or forgets it once the window is gone.
    pub fn set_settings_window_id(&self, id: Option<u32>) {
        *self.settings_window_lock() = id;
    }

    /// The settings window's id while it is open, for the window picker to drop
    /// along with the overlays' and the editors'.
    pub fn settings_window_id(&self) -> Option<u32> {
        *self.settings_window_lock()
    }

    /// The settings window's lock, with poisoning treated as recoverable for
    /// the reason `overlay_ids` gives: the value is a whole `Option` that is
    /// replaced or read, and propagating an unrelated panic would put the
    /// settings window back into the capture picker for good.
    fn settings_window_lock(&self) -> std::sync::MutexGuard<'_, Option<u32>> {
        self.settings_window_id
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

    /// Drops the editor at `label`.
    ///
    /// The capture it was opened on is deliberately not answered with and is
    /// not taken back out of the asset scope; `editor::release_editor` says why
    /// there is nothing this can hand a caller to undo.
    pub fn forget_editor(&self, label: &str) {
        self.editors().remove(label);
    }

    /// Records that `capture` is about to be shown, and answers whether the
    /// asset scope still has to be told about it.
    ///
    /// `true` the first time in the life of the process, `false` every time
    /// after. The second answer is the one that matters: a capture whose name a
    /// later capture has taken back is already in the scope, so there is
    /// nothing to add, and nothing was ever removed for this to have to put
    /// back. See `editor::allow_capture`.
    pub fn grant_capture(&self, capture: &Path) -> bool {
        self.granted_lock().insert(capture.to_path_buf())
    }

    /// The granted-captures lock, with poisoning treated as recoverable for the
    /// reason `overlay_ids` gives: every user of it reads or inserts one whole
    /// path, and propagating an unrelated panic would leave every later editor
    /// unable to say whether its picture is readable.
    fn granted_lock(&self) -> std::sync::MutexGuard<'_, HashSet<PathBuf>> {
        self.granted_captures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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

    /// Nothing is offered until a launch has read the file, the update sees what
    /// is there, and what it answers with is what is now in force.
    #[test]
    fn the_recent_captures_start_empty_and_the_update_answers_with_what_it_wrote() {
        let state = AppState::new();
        let seen = state.update_recent_captures(|current| {
            assert_eq!(current, Vec::<PathBuf>::new().as_slice());
            vec![PathBuf::from("/Pictures/a.png")]
        });
        assert_eq!(seen, vec![PathBuf::from("/Pictures/a.png")]);

        let seen = state.update_recent_captures(|current| {
            assert_eq!(current, [PathBuf::from("/Pictures/a.png")]);
            let mut next = vec![PathBuf::from("/Pictures/b.png")];
            next.extend(current.iter().cloned());
            next
        });
        assert_eq!(
            seen,
            vec![
                PathBuf::from("/Pictures/b.png"),
                PathBuf::from("/Pictures/a.png"),
            ]
        );
    }

    /// Two captures finishing at the same time. An editor save runs on a
    /// blocking worker and a capture on another, and `begin_capture` serialises
    /// captures only, so this really is two threads adding to the list at once.
    ///
    /// Both have to survive. A read, a decision and a write done as three steps
    /// loses whichever finished first, and the loop is what makes that
    /// reproducible rather than a race the test happens not to lose.
    #[test]
    fn two_threads_recording_at_once_both_end_up_in_the_list() {
        for _ in 0..200 {
            let state = std::sync::Arc::new(AppState::new());
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let threads: Vec<_> = ["/Pictures/a.png", "/Pictures/b.png"]
                .into_iter()
                .map(|name| {
                    let state = state.clone();
                    let barrier = barrier.clone();
                    std::thread::spawn(move || {
                        barrier.wait();
                        state.update_recent_captures(|current| {
                            crate::recents::remember(current, Path::new(name), 5)
                        });
                    })
                })
                .collect();
            for thread in threads {
                thread.join().expect("the recording thread");
            }

            let mut captures = state.update_recent_captures(|current| current.to_vec());
            captures.sort();
            assert_eq!(
                captures,
                vec![
                    PathBuf::from("/Pictures/a.png"),
                    PathBuf::from("/Pictures/b.png"),
                ],
                "one of the two captures was lost"
            );
        }
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

    /// Nothing is registered until something registers, and `None` has to stay
    /// distinguishable from "the built-in ones": it is what the settings window
    /// reads to say the keyboard is empty.
    #[test]
    fn nothing_is_bound_until_a_registration_says_so() {
        let state = AppState::new();
        assert_eq!(state.registered_shortcuts(), None);

        let bound = Settings::default().shortcuts;
        state.set_registered_shortcuts(Some(bound.clone()));
        assert_eq!(state.registered_shortcuts(), Some(bound));

        // A rebind that fails with nothing to fall back on says so, and must be
        // able to.
        state.set_registered_shortcuts(None);
        assert_eq!(state.registered_shortcuts(), None);
    }

    /// The settings window is frontmost while it is open, so its id has to be
    /// there for the picker to drop, and gone again once the window is, because
    /// the window server hands the number to somebody else.
    #[test]
    fn the_settings_window_id_is_recorded_while_it_is_open_and_not_after() {
        let state = AppState::new();
        assert_eq!(state.settings_window_id(), None);
        state.set_settings_window_id(Some(77));
        assert_eq!(state.settings_window_id(), Some(77));
        state.set_settings_window_id(None);
        assert_eq!(state.settings_window_id(), None);
    }

    /// A window that is not an editor has no capture, which is what makes a
    /// save from one a refusal instead of a write.
    #[test]
    fn a_window_that_is_not_an_editor_has_no_capture() {
        let state = AppState::new();
        assert_eq!(state.editor_capture("overlay-1"), None);
    }

    /// A closed editor stops being an editor: its save is refused from then on
    /// and its window leaves the capture picker. Closing one must not do either
    /// of those to the other window on the same picture.
    #[test]
    fn forgetting_one_editor_leaves_the_other_on_the_same_capture_alone() {
        let state = AppState::new();
        let shared = PathBuf::from("/Pictures/a.png");
        state.register_editor("editor-0".into(), shared.clone(), Some(11));
        state.register_editor("editor-1".into(), shared.clone(), Some(22));

        state.forget_editor("editor-0");
        assert_eq!(state.editor_capture("editor-0"), None);
        assert_eq!(state.editor_capture("editor-1"), Some(shared));
        assert_eq!(state.editor_window_ids(), vec![22]);

        state.forget_editor("editor-1");
        assert!(state.editor_window_ids().is_empty());
        // Forgetting a label twice is what a `Destroyed` event arriving after a
        // window has already been dropped looks like, and it must be quiet.
        state.forget_editor("editor-1");
    }

    /// The invariant the asset scope depends on, and the bug it was written
    /// for.
    ///
    /// A capture path is not opened once and never again. The filename template
    /// is the user's, so `Screenshot` renders the same name every time, and the
    /// ordinary paste-it-then-delete-it habit hands that name straight back to
    /// the next capture. Tauri's scope cannot undo a grant, so the grant has to
    /// survive the editor that asked for it: the second editor on that path
    /// finds it already there, and its picture loads.
    #[test]
    fn a_capture_stays_granted_after_its_editor_has_gone() {
        let state = AppState::new();
        let reused = PathBuf::from("/Pictures/Screenshot.png");

        assert!(
            state.grant_capture(&reused),
            "the first capture on this path has to be added to the scope"
        );
        state.register_editor("editor-0".into(), reused.clone(), Some(11));
        state.forget_editor("editor-0");

        assert!(
            !state.grant_capture(&reused),
            "the grant has to outlive the editor, or the capture that takes this name next would open on a picture the webview cannot read"
        );
        assert!(
            state.grant_capture(Path::new("/Pictures/Screenshot 2.png")),
            "a path nothing has shown yet is still a path the scope has to be told about"
        );
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
