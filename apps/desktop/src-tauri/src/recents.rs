//! The captures the menu bar offers to show in the Finder.
//!
//! Paths and nothing else. The list is a route back to files the user already
//! has, so a thumbnail, a copy of the pixels or the size of the picture would
//! all be a second copy of a screenshot living somewhere the user did not put
//! it and cannot see. What is written down here is what the Finder would show
//! them anyway.
//!
//! Its own file beside `settings.json`, for the reason `update_history` gives
//! for its own: this is not a setting. The user did not choose it, the settings
//! window does not show it, and a hand edit of `settings.json` that drops a line
//! must not be able to drop this. It is also written on every capture, and
//! rewriting the settings file that often would put a save the user is making in
//! the settings window in the way of a capture they are taking.
//!
//! Nothing here is fatal. A list that cannot be read is an empty menu, and a
//! list that cannot be written costs the memory of this run rather than the
//! capture: refusing to save a screenshot because a file in the configuration
//! directory is unhappy is worse than forgetting where the last one went.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// How many captures the menu remembers.
///
/// Five, because the menu is a shortcut back to the capture the user has just
/// taken and the two or three before it, not a file browser: the Finder is one
/// click away and is better at being one. A named constant rather than a
/// literal because it is the one number the whole feature is shaped by, and
/// `remember` has to be testable at other limits.
pub const RECENT_CAPTURE_LIMIT: usize = 5;

/// The file inside `app_config_dir()`, beside `settings.json`.
const RECENTS_FILE_NAME: &str = "recent-captures.json";

/// What the file holds.
///
/// A named object rather than a bare array, so that a later version can add a
/// field without every installed build failing to parse the file it wrote.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Recents {
    /// Newest first, which is the order the menu renders in.
    captures: Vec<PathBuf>,
}

/// `saved` at the front, with the rest behind it and nothing listed twice.
///
/// Newest first, because that is the one the user has just taken and the one
/// they are reaching for. A path already in the list moves to the front rather
/// than appearing a second time: an edited save writes back over the capture it
/// was opened on, so the same path arrives here again a few seconds after the
/// capture did, and two identical lines in a menu are a menu the user cannot
/// read.
pub(crate) fn remember(captures: &[PathBuf], saved: &Path, limit: usize) -> Vec<PathBuf> {
    let mut listed = Vec::with_capacity(captures.len() + 1);
    listed.push(saved.to_path_buf());
    listed.extend(captures.iter().cloned());
    normalise(listed, limit)
}

/// The list with anything that is no longer on disk taken out.
///
/// The predicate is injected rather than being `Path::is_file` so that the rule
/// can be tested without a temporary directory, which is what
/// `crates/capture` does with its own pure helpers.
pub(crate) fn retain_existing(
    captures: Vec<PathBuf>,
    exists: impl Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    captures.into_iter().filter(|path| exists(path)).collect()
}

/// The list without `gone`, for the capture a click found had been deleted.
pub(crate) fn forget(captures: &[PathBuf], gone: &Path) -> Vec<PathBuf> {
    captures
        .iter()
        .filter(|path| path.as_path() != gone)
        .cloned()
        .collect()
}

/// `captures` with nothing listed twice and nothing past `limit`, order kept.
///
/// Applied to what is read from disk as well as to what is added, because the
/// file is plain JSON in the user's configuration directory: a hand edit that
/// puts fifty lines in it must not put fifty lines in the menu bar.
fn normalise(captures: Vec<PathBuf>, limit: usize) -> Vec<PathBuf> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut kept = Vec::with_capacity(limit.min(captures.len()));
    for path in captures {
        if kept.len() >= limit {
            break;
        }
        if seen.insert(path.clone()) {
            kept.push(path);
        }
    }
    kept
}

/// How many stored entries each line of the menu is worth a `stat` for.
///
/// The stored file is JSON in the user's configuration directory, so its length
/// is not this application's to assume, and every entry that survives the cap
/// costs a filesystem call on the launch path. Two rather than one, so that a
/// list whose newest entries have been deleted still fills the menu from further
/// down; a bound rather than none, because a save folder on a network volume
/// that is not mounted answers every `is_file` at the speed of a mount timeout,
/// and this runs before the application has a menu bar item to say so with.
const STORED_CAPTURES_PER_MENU_LINE: usize = 2;

/// The menu's view of a stored list: capped, pruned of what has been deleted,
/// and capped again.
///
/// The order of the three is the whole of it. Capping first bounds how many
/// paths the filesystem is asked about; pruning then drops what is gone; capping
/// again is what fills the menu back up from further down the list rather than
/// leaving it short by however many entries were deleted.
pub(crate) fn shortlist(
    stored: Vec<PathBuf>,
    limit: usize,
    exists: impl Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    let candidates = normalise(stored, limit.saturating_mul(STORED_CAPTURES_PER_MENU_LINE));
    normalise(retain_existing(candidates, exists), limit)
}

/// The path as the filesystem knows it, or the path itself when it cannot say.
///
/// The two places a capture is recorded from disagree about this otherwise. A
/// direct capture is written under the save directory exactly as the settings
/// hold it, while an edited save goes through `commands::resolve_save_target`,
/// which canonicalises the directory before it joins the file name onto it. One
/// symlink anywhere above the capture, an obvious one being a save folder moved
/// onto another volume, and the same file arrives here under two names and is
/// listed twice. Normalising on the way in is the cheapest place to settle it:
/// one call per capture, against a file that has just been written and is
/// therefore in the cache.
pub(crate) fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Puts the stored list into force at launch, dropping captures that have been
/// deleted since they were written down.
///
/// The pruning is the answer to the deleted-file case at the one moment it can
/// be answered cheaply: the menu is built once and the user may open it hours
/// later, so a check at build time is the freshest one available without asking
/// the filesystem about five paths every time a menu opens.
pub fn restore(app: &AppHandle) {
    let stored = recents_path(app)
        .map(|path| read(&path))
        .unwrap_or_default();
    // Outside the lock, because every `is_file` in it can block on a volume that
    // is not there.
    let shortlisted = shortlist(stored.clone(), RECENT_CAPTURE_LIMIT, |path| path.is_file());
    let captures = app
        .state::<AppState>()
        .update_recent_captures(|_| shortlisted);
    crate::tray::refresh_recent_captures(app, &captures);
    // Only when the pruning changed something, so an ordinary launch does not
    // rewrite a file it agrees with.
    if captures != stored {
        store(app, &captures);
    }
}

/// Records a capture that has just been written.
///
/// Called from the two places a capture reaches the disk, which are the only
/// two places that know it did.
pub fn record(app: &AppHandle, saved: &Path) {
    let saved = canonical(saved);
    let captures = app
        .state::<AppState>()
        .update_recent_captures(|current| remember(current, &saved, RECENT_CAPTURE_LIMIT));
    put_into_force(app, &captures);
}

/// Drops a capture a click found was no longer there.
pub fn forget_missing(app: &AppHandle, gone: &Path) {
    let captures = app
        .state::<AppState>()
        .update_recent_captures(|current| forget(current, gone));
    put_into_force(app, &captures);
}

/// Renders and writes down the list that won.
///
/// Takes the list rather than reading it back, and is called with the lock
/// already released: `refresh_recent_captures` waits on the main thread, which
/// is the thread a click on this very menu runs on, so holding the lock across
/// it would be one half of a deadlock.
fn put_into_force(app: &AppHandle, captures: &[PathBuf]) {
    crate::tray::refresh_recent_captures(app, captures);
    store(app, captures);
}

/// Writes the list, and says so in the log if it could not.
///
/// The log rather than `report::report_failure`, for the reason
/// `update_history` writes its own failures there: this runs on the back of a
/// capture that succeeded, and putting an exclamation mark in the menu bar
/// after a screenshot the user has in hand would be reporting the wrong thing.
fn store(app: &AppHandle, captures: &[PathBuf]) {
    let Some(path) = recents_path(app) else {
        return;
    };
    if let Err(err) = write(&path, captures) {
        crate::report::append_to_log(
            app,
            &format!("Snapdeck could not record its recent captures ({err})."),
        );
    }
}

/// The stored list, or nothing when there is nothing usable to read.
///
/// Every failure is the same answer, as in `update_history::read`: a first run
/// has no file and a corrupt file has no list in it, and neither is worth
/// saying anything about. The menu is simply empty.
fn read(path: &Path) -> Vec<PathBuf> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Recents>(&contents)
        .map(|recents| recents.captures)
        .unwrap_or_default()
}

/// Replaces the list, or leaves the one that is there.
///
/// `commands::write_atomically` rather than `std::fs::write`, which truncates:
/// this file is rewritten on every capture, so a crash or a full disk during a
/// write is the likeliest moment for it to be half a file, and a half-written
/// list reads back as no list at all.
fn write(path: &Path, captures: &[PathBuf]) -> Result<(), String> {
    let recents = Recents {
        captures: captures.to_vec(),
    };
    let directory = path
        .parent()
        .ok_or_else(|| format!("{} has no directory", path.display()))?;
    std::fs::create_dir_all(directory)
        .map_err(|err| format!("failed to create {}: {err}", directory.display()))?;
    let json = serde_json::to_string_pretty(&recents)
        .map_err(|err| format!("failed to render the recent captures: {err}"))?;
    crate::commands::write_atomically(path, json.as_bytes())
}

fn recents_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|directory| directory.join(RECENTS_FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    /// A directory of this test's own, so the symlink test only ever meets the
    /// files it made itself.
    fn temp_dir(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "snapdeck-recents-{}-{}-{name}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&directory).expect("the test's own temporary directory");
        directory
    }

    /// The ordinary case: the capture the user has just taken is the one at the
    /// top of the menu.
    #[test]
    fn the_newest_capture_is_first() {
        let captures = remember(&paths(&["/shots/b.png"]), Path::new("/shots/a.png"), 5);
        assert_eq!(captures, paths(&["/shots/a.png", "/shots/b.png"]));
    }

    /// An edited save writes back over the capture it was opened on, so the
    /// same path arrives a second time within seconds of the first. It moves to
    /// the front; it does not appear twice.
    #[test]
    fn a_path_that_is_already_listed_moves_to_the_front() {
        let captures = remember(
            &paths(&["/shots/a.png", "/shots/b.png", "/shots/c.png"]),
            Path::new("/shots/c.png"),
            5,
        );
        assert_eq!(
            captures,
            paths(&["/shots/c.png", "/shots/a.png", "/shots/b.png"])
        );
    }

    /// The limit is what keeps the submenu a shortcut rather than a file
    /// browser, and the entry that falls off is the oldest one.
    #[test]
    fn the_list_never_grows_past_the_limit() {
        let mut captures = Vec::new();
        for index in 0..10 {
            captures = remember(&captures, Path::new(&format!("/shots/{index}.png")), 3);
            assert!(captures.len() <= 3, "{captures:?}");
        }
        assert_eq!(
            captures,
            paths(&["/shots/9.png", "/shots/8.png", "/shots/7.png"])
        );
    }

    /// A capture the user has deleted or moved is not offered. The predicate
    /// stands in for the filesystem so the rule is the same every run.
    #[test]
    fn a_capture_that_is_no_longer_on_disk_is_dropped() {
        let kept = retain_existing(paths(&["/shots/gone.png", "/shots/here.png"]), |path| {
            path == Path::new("/shots/here.png")
        });
        assert_eq!(kept, paths(&["/shots/here.png"]));
    }

    /// And the order of what survives is the order it had: pruning must not
    /// shuffle the menu.
    #[test]
    fn pruning_keeps_the_order_of_what_is_left() {
        let kept = retain_existing(
            paths(&["/shots/a.png", "/shots/gone.png", "/shots/b.png"]),
            |path| path != Path::new("/shots/gone.png"),
        );
        assert_eq!(kept, paths(&["/shots/a.png", "/shots/b.png"]));
    }

    /// The click-time half of the same case: one entry goes, the rest stay
    /// exactly as they were.
    #[test]
    fn forgetting_one_capture_leaves_the_others_alone() {
        let kept = forget(
            &paths(&["/shots/a.png", "/shots/b.png", "/shots/c.png"]),
            Path::new("/shots/b.png"),
        );
        assert_eq!(kept, paths(&["/shots/a.png", "/shots/c.png"]));
    }

    /// The file is JSON in the user's configuration directory, so it is
    /// something they can edit and something another build could have written.
    /// Neither may put a list in the menu that the rules above forbid.
    #[test]
    fn a_hand_edited_file_is_held_to_the_same_rules() {
        let normalised = normalise(
            paths(&[
                "/shots/a.png",
                "/shots/a.png",
                "/shots/b.png",
                "/shots/c.png",
            ]),
            2,
        );
        assert_eq!(normalised, paths(&["/shots/a.png", "/shots/b.png"]));
    }

    /// The composition `restore` depends on, and the one neither half could be
    /// trusted for alone: a list whose newest entries have been deleted still
    /// fills the menu, from further down, and still stops at the limit.
    #[test]
    fn deleted_entries_are_replaced_from_further_down_the_list() {
        let stored = paths(&[
            "/shots/gone-1.png",
            "/shots/gone-2.png",
            "/shots/a.png",
            "/shots/b.png",
            "/shots/c.png",
            "/shots/d.png",
            "/shots/e.png",
            "/shots/f.png",
        ]);
        let kept = shortlist(stored, 5, |path| !path.to_string_lossy().contains("gone-"));
        assert_eq!(
            kept,
            paths(&[
                "/shots/a.png",
                "/shots/b.png",
                "/shots/c.png",
                "/shots/d.png",
                "/shots/e.png",
            ])
        );
    }

    /// The file is the user's to edit and this runs at launch, on the main
    /// thread, before there is a menu bar item. A save folder on a network
    /// volume that is not mounted answers every `is_file` at the speed of a
    /// mount timeout, so the number of them a stored file can provoke has to be
    /// bounded by the code rather than by the file.
    #[test]
    fn a_stored_list_is_capped_before_the_filesystem_is_asked() {
        let stored: Vec<PathBuf> = (0..5_000)
            .map(|index| PathBuf::from(format!("/shots/{index}.png")))
            .collect();
        let asked = std::cell::Cell::new(0_usize);
        let kept = shortlist(stored, RECENT_CAPTURE_LIMIT, |_| {
            asked.set(asked.get() + 1);
            true
        });
        assert!(
            asked.get() <= RECENT_CAPTURE_LIMIT * STORED_CAPTURES_PER_MENU_LINE,
            "asked the filesystem about {} paths",
            asked.get()
        );
        assert_eq!(kept.len(), RECENT_CAPTURE_LIMIT);
        assert_eq!(kept.first(), Some(&PathBuf::from("/shots/0.png")));
    }

    /// The same file under two names is one entry. `commands::resolve_save_target`
    /// canonicalises an edited save's directory and the capture path does not,
    /// so one symlink above the save folder is all it takes for the two record
    /// sites to disagree about what a capture is called.
    #[test]
    fn the_same_file_reached_by_two_paths_is_listed_once() {
        let directory = temp_dir("canonical");
        let real = directory.join("shots");
        std::fs::create_dir_all(&real).expect("the real directory");
        let capture = real.join("a.png");
        std::fs::write(&capture, b"not really a png").expect("the capture");
        let link = directory.join("link");
        std::os::unix::fs::symlink(&real, &link).expect("the symlink");
        let through_link = link.join("a.png");
        assert_ne!(capture, through_link, "two names for one file");

        let captures = remember(
            &[canonical(&capture)],
            &canonical(&through_link),
            RECENT_CAPTURE_LIMIT,
        );
        std::fs::remove_dir_all(&directory).ok();

        assert_eq!(captures.len(), 1, "{captures:?}");
    }

    /// And the other half of the same rule: two files really are two entries.
    /// The editor writes a JPEG beside the PNG it was opened on when the format
    /// is changed, so this is what an ordinary edit-and-save produces.
    #[test]
    fn an_edited_save_into_the_other_format_is_a_second_entry() {
        let captures = remember(
            &paths(&["/shots/a.png"]),
            Path::new("/shots/a.jpg"),
            RECENT_CAPTURE_LIMIT,
        );
        assert_eq!(captures, paths(&["/shots/a.jpg", "/shots/a.png"]));
    }

    /// A corrupt file is a missing file, all the way through `read`, and a file
    /// this build wrote reads back as what it wrote or the list would never
    /// survive a restart.
    #[test]
    fn a_corrupt_file_reads_as_an_empty_list_and_a_written_one_reads_back() {
        let directory = std::env::temp_dir().join(format!(
            "snapdeck-recent-captures-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&directory).expect("the test's own temporary directory");
        let path = directory.join(RECENTS_FILE_NAME);

        assert_eq!(
            read(&path),
            Vec::<PathBuf>::new(),
            "a file that is not there"
        );

        for corrupt in ["", "{", "not json at all", r#"{"somethingElse": []}"#] {
            std::fs::write(&path, corrupt).expect("write the corrupt file");
            assert_eq!(read(&path), Vec::<PathBuf>::new(), "{corrupt:?}");
        }

        let captures = paths(&["/shots/a.png", "/shots/b.png"]);
        write(&path, &captures).expect("write the list");
        assert_eq!(read(&path), captures);

        std::fs::remove_dir_all(&directory).ok();
    }

    /// Paths only. The whole privacy claim of this module is that it writes
    /// down where a screenshot is and never what is in it, and the file is the
    /// place that claim is either true or not.
    #[test]
    fn the_file_holds_paths_and_nothing_else() {
        let json = serde_json::to_string(&Recents {
            captures: paths(&["/shots/a.png"]),
        })
        .expect("render");
        assert_eq!(json, r#"{"captures":["/shots/a.png"]}"#);
    }
}
